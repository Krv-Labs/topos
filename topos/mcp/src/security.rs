//! Path-safety helpers for the Topos MCP server.
//!
//! The server refuses to read files outside the file-access root.
//! Resolution order:
//!
//! 1. `TOPOS_MCP_FILE_ROOT` env var, if set.
//! 2. The nearest ancestor of `cwd` that contains `.git` or
//!    `pyproject.toml`/`Cargo.toml` (auto-detect project root).
//! 3. Fail closed: tools return an error explaining how to configure the
//!    root.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

const PROJECT_MARKERS: &[&str] = &[".git", "pyproject.toml", "Cargo.toml"];

static FILE_ROOT: OnceLock<Result<PathBuf, String>> = OnceLock::new();

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

/// Determine the canonical file-access root, caching the result for the
/// process lifetime (stdio servers are single-project).
pub fn resolve_file_root() -> Result<PathBuf, String> {
    FILE_ROOT.get_or_init(compute_file_root).clone()
}

/// Lexically normalize `..` / `.` components without touching the
/// filesystem, so a non-existent path can still be checked against the root.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Resolve symlinks on the longest existing prefix, then re-append any
/// missing tail — matching Python `Path.resolve(strict=False)`.
///
/// A plain `canonicalize().unwrap_or_else(normalize)` is unsafe: when the
/// leaf is missing, lexical normalize does not follow symlinks on the
/// existing prefix, so `/proj/link/newfile` with `link → /etc` would be
/// accepted under root `/proj`.
fn resolve_existing_prefix(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    while !existing.as_os_str().is_empty() && !existing.exists() {
        match existing.file_name() {
            Some(name) => {
                missing.push(name.to_os_string());
                if !existing.pop() {
                    break;
                }
            }
            None => break,
        }
    }

    let mut resolved = if existing.as_os_str().is_empty() || !existing.exists() {
        normalize(path)
    } else {
        existing
            .canonicalize()
            .unwrap_or_else(|_| normalize(&existing))
    };

    for part in missing.into_iter().rev() {
        if part == "." {
            continue;
        }
        if part == ".." {
            resolved.pop();
            continue;
        }
        resolved.push(part);
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
    let root = resolve_file_root()?;
    resolve_path_within(filepath, &root)
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
    fn normalize_collapses_parent_components() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
    }

    #[test]
    fn escape_via_dotdot_is_denied() {
        // The root is this repo (auto-detected or env-provided); a
        // sufficiently deep ../ chain always escapes it.
        let err = resolve_within_root("../../../../../../../../etc/passwd").unwrap_err();
        assert!(err.contains("Access denied"), "{err}");
    }

    #[test]
    fn missing_in_root_leaf_is_allowed() {
        let dir = std::env::temp_dir().join(format!(
            "topos-security-missing-{}",
            std::process::id()
        ));
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
        let dir = std::env::temp_dir().join(format!(
            "topos-security-symlink-{}",
            std::process::id()
        ));
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
}
