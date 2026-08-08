//! Path-safety helpers for the Topos MCP server.
//!
//! The server refuses to read files outside the file-access root.
//! Resolution order:
//!
//! Filesystem tools derive a project boundary from their requested absolute
//! path. `TOPOS_MCP_FILE_ROOT`, when set, is an optional maximum boundary.
//! Calls fail closed when the path is not inside that boundary or no project
//! marker can be found.

use std::path::{Component, Path, PathBuf};

const PROJECT_MARKERS: &[&str] = &[".git", "pyproject.toml", "Cargo.toml"];

fn auto_detect_root(start: &Path) -> Option<PathBuf> {
    let start = start.canonicalize().ok()?;
    for candidate in std::iter::once(start.as_path()).chain(start.ancestors().skip(1)) {
        for marker in PROJECT_MARKERS {
            if candidate.join(marker).exists() {
                return Some(candidate.to_path_buf());
            }
        }
    }
    None
}

/// Resolve an existing file or directory and the repository that contains it.
///
/// A configured `TOPOS_MCP_FILE_ROOT` is an optional *maximum* boundary.  In
/// its absence, the requested absolute path supplies the project identity;
/// this is what makes a user-level stdio server usable when its process cwd is
/// not the editor workspace.
pub fn resolve_project_path(path: &str) -> Result<(PathBuf, PathBuf), String> {
    let requested = PathBuf::from(path);
    let configured_root = std::env::var("TOPOS_MCP_FILE_ROOT")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|root| {
            root.canonicalize()
                .map_err(|e| format!("TOPOS_MCP_FILE_ROOT is not a readable directory: {e}"))
        })
        .transpose()?;

    if configured_root.is_none() && !requested.is_absolute() {
        return Err(
            "An absolute file or directory path is required when TOPOS_MCP_FILE_ROOT is unset. \
             Pass the current workspace path from the MCP host."
                .to_string(),
        );
    }

    let resolved = if requested.is_absolute() {
        requested
    } else {
        configured_root
            .as_ref()
            .expect("checked above")
            .join(requested)
    }
    .canonicalize()
    .map_err(|e| format!("Path is not readable: {e}"))?;

    if let Some(boundary) = &configured_root {
        if !resolved.starts_with(boundary) {
            return Err(format!(
                "Access denied: path must be inside {}. Got: {}",
                boundary.display(),
                resolved.display()
            ));
        }
    }

    let start = if resolved.is_dir() {
        resolved.as_path()
    } else {
        resolved
            .parent()
            .ok_or_else(|| "Path has no parent directory".to_string())?
    };
    let project_root = auto_detect_root(start).ok_or_else(|| {
        format!(
            "No project marker (.git / pyproject.toml / Cargo.toml) was found above {}",
            start.display()
        )
    })?;
    if let Some(boundary) = configured_root {
        if !project_root.starts_with(&boundary) {
            return Err(format!(
                "Access denied: project root must be inside {}. Got: {}",
                boundary.display(),
                project_root.display()
            ));
        }
    }
    Ok((resolved, project_root))
}

fn compute_file_root() -> Result<PathBuf, String> {
    if let Ok(env_value) = std::env::var("TOPOS_MCP_FILE_ROOT") {
        if !env_value.is_empty() {
            let path = PathBuf::from(env_value);
            return path
                .canonicalize()
                .map_err(|e| format!("TOPOS_MCP_FILE_ROOT is not a readable directory: {e}"));
        }
    }
    let cwd = std::env::current_dir().map_err(|e| format!("cannot determine cwd: {e}"))?;
    auto_detect_root(&cwd).ok_or_else(|| {
        "TOPOS_MCP_FILE_ROOT is unset and no project marker (.git / pyproject.toml / \
         Cargo.toml) was found by walking up from cwd. Set TOPOS_MCP_FILE_ROOT to the \
         repository root before starting the MCP server."
            .to_string()
    })
}

/// Determine a root from the explicitly configured boundary or process cwd.
///
/// Kept for diagnostics and legacy callers. New filesystem tools must use
/// [`resolve_project_path`] so a user-level MCP server is not pinned to its
/// startup cwd.
pub fn resolve_file_root() -> Result<PathBuf, String> {
    compute_file_root()
}

/// Root that owns `.gitnexus`: the nearest ancestor holding `.git`.
///
/// [`resolve_project_path`] returns the *innermost* project marker, which is
/// right for file access but wrong for COMPOSABLE: in a workspace, a file
/// under `topos/mcp/` resolves to `topos/mcp` (its `Cargo.toml`), while the
/// store lives at the repo root. Deriving `.gitnexus` from that sub-package
/// makes every call report `missing` and shell out `gitnexus analyze` on a
/// sub-crate — which is why COMPOSABLE worked for some files and not others
/// (#293 follow-up).
///
/// A `.gitnexus` store is git-scoped anyway (branch-scoped stores, HEAD-sha
/// fingerprints), so the git root is the only root it can mean. The walk
/// stops at `TOPOS_MCP_FILE_ROOT` so an enclosing repo above the configured
/// boundary is never analyzed.
pub fn composable_default_root(detected_project: &Path) -> PathBuf {
    let boundary = std::env::var("TOPOS_MCP_FILE_ROOT")
        .ok()
        .filter(|value| !value.is_empty())
        .and_then(|value| PathBuf::from(value).canonicalize().ok());
    detected_project
        .ancestors()
        .take_while(|dir| boundary.as_ref().is_none_or(|b| dir.starts_with(b)))
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
        .or(boundary)
        .unwrap_or_else(|| detected_project.to_path_buf())
}

/// Resolve symlinks incrementally, one path component at a time, matching
/// Python `Path.resolve(strict=False)`.
///
/// A plain `canonicalize().unwrap_or_else(normalize)` is unsafe: when the
/// leaf is missing, lexical normalize does not follow symlinks on the
/// existing prefix, so `/proj/link/newfile` with `link → /etc` would be
/// accepted under root `/proj`.
///
/// The previous fix for that walked the path *backwards* from the leaf,
/// popping components until it found one that existed. That breaks as soon
/// as a `..` component is involved: `Path::file_name()` returns `None` when
/// the last component is `..`, so the walk bailed out to the same unsafe
/// whole-path lexical normalize it was meant to replace — e.g.
/// `/proj/link/subdir/../newfile` (an existing `link` symlink, missing
/// `subdir`) was silently accepted even though it really resolves outside
/// `/proj`. Walking *forwards* instead avoids that: once a component is
/// found not to exist, nothing after it can be a symlink either, so the
/// remaining components are safe to apply lexically against the
/// already-resolved real prefix. The unresolved depth is tracked so that a
/// `..` which removes every missing component resumes symlink resolution.
pub(crate) fn resolve_existing_prefix(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    let mut missing_components: usize = 0;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
                missing_components = missing_components.saturating_sub(1);
            }
            Component::Prefix(_) | Component::RootDir => resolved.push(component),
            Component::Normal(name) => {
                if missing_components > 0 {
                    resolved.push(name);
                    missing_components += 1;
                    continue;
                }
                match resolved.join(name).canonicalize() {
                    Ok(real) => resolved = real,
                    Err(_) => {
                        missing_components = 1;
                        resolved.push(name);
                    }
                }
            }
        }
    }
    resolved
}

/// Resolve `filepath` against `root` and reject paths that escape it.
pub(crate) fn resolve_path_within(filepath: &str, root: &Path) -> Result<PathBuf, String> {
    let path = Path::new(filepath);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let resolved = resolve_existing_prefix(&joined);
    if resolved.starts_with(root) {
        Ok(resolved)
    } else {
        Err(format!(
            "Access denied: path must be inside {}. Got: {}",
            root.display(),
            resolved.display()
        ))
    }
}

/// Resolve a path (absolute or root-relative) and check it's inside the
/// root, without reading it. Symlinks on an existing prefix are resolved
/// even when the final component is missing.
pub fn resolve_within_root(filepath: &str) -> Result<PathBuf, String> {
    resolve_project_path(filepath).map(|(path, _)| path)
}

/// Read a UTF-8 file if it is within the configured root.
pub fn read_safe_utf8_file(filepath: &str) -> Result<String, String> {
    let resolved = resolve_within_root(filepath)?;
    if resolved.is_dir() {
        return Err(format!("Path is not a file: {filepath}"));
    }
    match std::fs::read(&resolved) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| format!("File is not valid UTF-8 text: {filepath}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("File not found: {filepath}"))
        }
        Err(e) => Err(format!("Unable to read file '{filepath}': {e}")),
    }
}

/// Read an already-root-checked path.
pub fn read_resolved_utf8(path: &Path) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| format!("File is not valid UTF-8 text: {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("File not found: {}", path.display()))
        }
        Err(e) => Err(format!("Unable to read file '{}': {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_via_dotdot_is_denied() {
        let err = resolve_within_root("../../../../../../../../etc/passwd").unwrap_err();
        assert!(err.contains("absolute"), "{err}");
    }

    #[test]
    fn absolute_file_derives_its_containing_project() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let (resolved, project_root) = resolve_project_path(&source.to_string_lossy()).unwrap();
        assert_eq!(resolved, source.canonicalize().unwrap());
        assert!(project_root.join("Cargo.toml").is_file());
    }

    /// The COMPOSABLE root must climb past a nested package marker to the
    /// git root that actually owns `.gitnexus` — otherwise a workspace file
    /// resolves to its sub-crate, reports `missing`, and re-runs
    /// `gitnexus analyze` on a directory with no store.
    #[test]
    fn composable_root_climbs_to_the_git_root_not_the_nested_package() {
        let dir =
            std::env::temp_dir().join(format!("topos-composable-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let member = dir.join("crates/member");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(member.join("Cargo.toml"), "[package]\n").unwrap();

        let detected = member.canonicalize().unwrap();
        assert_eq!(
            composable_default_root(&detected),
            dir.canonicalize().unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_in_root_leaf_is_allowed() {
        let dir =
            std::env::temp_dir().join(format!("topos-security-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.canonicalize().unwrap();
        let missing = root.join("does-not-exist-yet.rs");
        let resolved = resolve_path_within(missing.to_str().unwrap(), &root).unwrap();
        assert_eq!(resolved, missing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_via_missing_leaf_is_denied() {
        let dir =
            std::env::temp_dir().join(format!("topos-security-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root_path = dir.join("proj");
        std::fs::create_dir_all(&root_path).unwrap();
        let root = root_path.canonicalize().unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink("/etc", &link).unwrap();
        let request = link.join("newfile");
        let err = resolve_path_within(request.to_str().unwrap(), &root).unwrap_err();
        assert!(err.contains("Access denied"), "{err}");
        assert!(err.contains("/etc"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_via_dotdot_and_missing_intermediate_is_denied() {
        // Regression: the backward-walk implementation bailed to whole-path
        // lexical normalize as soon as it hit a `..` component, silently
        // accepting escapes like `link/subdir/../newfile` where `subdir`
        // doesn't exist under the symlink target.
        let dir = std::env::temp_dir().join(format!(
            "topos-security-dotdot-symlink-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let root_path = dir.join("proj");
        std::fs::create_dir_all(&root_path).unwrap();
        let root = root_path.canonicalize().unwrap();
        let outside = dir.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let outside = outside.canonicalize().unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let request = format!("{}/subdir/../newfile", link.display());
        let err = resolve_path_within(&request, &root).unwrap_err();
        assert!(err.contains("Access denied"), "{err}");
        assert!(
            err.contains(&outside.display().to_string()),
            "expected resolved path under {}, got: {err}",
            outside.display()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_checks_resume_after_dotdot_removes_a_missing_component() {
        let dir = std::env::temp_dir().join(format!(
            "topos-security-missing-dotdot-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let root_path = dir.join("proj");
        let outside = dir.join("outside");
        std::fs::create_dir_all(&root_path).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let root = root_path.canonicalize().unwrap();
        let outside = outside.canonicalize().unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

        let request = root.join("missing/../link/file");
        let err = resolve_path_within(request.to_str().unwrap(), &root).unwrap_err();

        assert!(err.contains("Access denied"), "{err}");
        assert!(err.contains(&outside.display().to_string()), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
