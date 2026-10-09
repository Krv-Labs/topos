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
/// A configured `TOPOS_MCP_FILE_ROOT` is an optional *maximum* boundary. In
/// its absence, the requested absolute path supplies the project identity; this
/// is what makes a user-level stdio server usable when its process cwd is not
/// the editor workspace.
///
/// A **relative** path needs a base before it can be anything, and there are two:
/// the configured root, or failing that the project the process was started in.
/// Previously the second case was refused outright, which made every agent that
/// writes `src/lib.rs` rather than `/Users/you/repo/src/lib.rs` fail — even
/// when the server's boundary was already correct. Falling back to the cwd's
/// project fixes that, and is *stricter* than what it replaced: previously an
/// absolute path with no configured root was checked only for having a project
/// marker somewhere above it, whereas a relative one is now resolved against a
/// known root and held inside it.
pub fn resolve_project_path(path: &str) -> Result<(PathBuf, PathBuf), String> {
    let requested = PathBuf::from(path);
    let configured_root = configured_file_root()?;

    // Where a relative path is anchored. `None` for an absolute path with no
    // configured root: there is no boundary to hold it to, and inventing one
    // from the cwd would silently re-pin the server to a folder the host chose.
    let base = match (&configured_root, requested.is_absolute()) {
        (Some(root), _) => Some(root.clone()),
        (None, false) => Some(startup_project_root()?),
        (None, true) => None,
    };

    let resolved = match &base {
        Some(base) => base.join(&requested),
        None => requested,
    }
    .canonicalize()
    .map_err(|e| match &base {
        // The base is the actionable part: without it the reader cannot tell
        // whether the path was simply absent or resolved somewhere they did not
        // expect, which is the whole failure when a relative path is used
        // against the wrong project.
        Some(base) => format!(
            "Path is not readable: {e} (resolved against {})",
            base.display()
        ),
        None => format!("Path is not readable: {e}"),
    })?;

    // Canonicalization above resolves symlinks, so this is a real containment
    // check rather than a lexical one: a link out of the root fails here.
    if let Some(boundary) = &base {
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
    if let Some(boundary) = &base {
        if !project_root.starts_with(boundary) {
            return Err(format!(
                "Access denied: project root must be inside {}. Got: {}",
                boundary.display(),
                project_root.display()
            ));
        }
    }
    Ok((resolved, project_root))
}

/// The explicitly configured boundary, if any. Not an error when unset.
fn configured_file_root() -> Result<Option<PathBuf>, String> {
    std::env::var("TOPOS_MCP_FILE_ROOT")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|root| {
            root.canonicalize()
                .map_err(|e| format!("TOPOS_MCP_FILE_ROOT is not a readable directory: {e}"))
        })
        .transpose()
}

/// The project the process was started in, used as the base for relative paths
/// when nothing is configured.
///
/// This is the one place the server's cwd decides anything, and it is a
/// deliberate fallback rather than the primary source of identity: the host
/// chooses the cwd, so it may be the wrong project entirely. Absolute paths
/// never consult it.
fn startup_project_root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot determine cwd: {e}"))?;
    auto_detect_root(&cwd).ok_or_else(|| {
        format!(
            "A relative path needs a project to resolve against, and none could be found \
             above the working directory {}. Either pass an absolute path, or set \
             TOPOS_MCP_FILE_ROOT to the project root before starting the MCP server.",
            cwd.display()
        )
    })
}

/// Where the server's own file root came from.
///
/// Reported by `topos://build` so a server pinned to the wrong project is
/// visible instead of silently reporting another repository's boundary. This is
/// a diagnostic, not a decision: it never gates a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootSource {
    /// `TOPOS_MCP_FILE_ROOT` is set.
    Configured,
    /// Derived by walking up from the process working directory.
    StartupCwd,
    /// No project could be found at all.
    None,
}

impl RootSource {
    pub fn describe(self) -> &'static str {
        match self {
            RootSource::Configured => "TOPOS_MCP_FILE_ROOT",
            RootSource::StartupCwd => "the server's startup working directory",
            RootSource::None => "nowhere — no project marker found",
        }
    }
}

/// The server's file root and how it was arrived at.
pub fn file_root_with_source() -> (Option<PathBuf>, RootSource) {
    match configured_file_root() {
        Ok(Some(root)) => (Some(root), RootSource::Configured),
        Ok(None) => match std::env::current_dir().map(|cwd| auto_detect_root(&cwd)) {
            Ok(Some(root)) => (Some(root), RootSource::StartupCwd),
            _ => (None, RootSource::None),
        },
        // A configured root that cannot be read is reported as "not
        // configured" here; the call sites that care surface the error.
        Err(_) => (None, RootSource::None),
    }
}

/// Determine a root from the explicitly configured boundary or process cwd.
///
/// Kept for diagnostics and legacy callers. New filesystem tools must use
/// [`resolve_project_path`] so a user-level MCP server is not pinned to its
/// startup cwd.
pub fn resolve_file_root() -> Result<PathBuf, String> {
    file_root_with_source()
        .0
        .ok_or_else(|| "no project marker (.git / pyproject.toml / Cargo.toml) was found; set TOPOS_MCP_FILE_ROOT to the repository root".to_string())
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
    // Read through the one resolver rather than repeating the lookup: two
    // copies of this read were how `composable_default_root` and
    // `resolve_project_path` could come to disagree about whether a boundary
    // was configured.
    let boundary = configured_file_root().ok().flatten();
    detected_project
        .ancestors()
        .take_while(|dir| boundary.as_ref().is_none_or(|b| dir.starts_with(b)))
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
        .or(boundary)
        .unwrap_or_else(|| detected_project.to_path_buf())
}

/// A note to attach to a tool result when a relative path was anchored to the
/// server's startup directory.
///
/// A relative path carries no project identity: which repository `src/lib.rs`
/// means depends entirely on the base the resolver chose. When that base is the
/// startup directory it can be wrong — an MCP server's cwd is picked by the
/// host, and three of the four servers on the machine this was diagnosed
/// against pointed at a repository other than the one being edited — and when
/// the wrong base happens to contain the same relative path, the read
/// *succeeds* into the wrong repository.
///
/// That case is not decidable from inside the server, and the MCP feature that
/// would decide it (client roots) is deprecated (SEP-2577), so Topos does not
/// build on it. The case is made visible instead. There is nothing to say for
/// an absolute path (it names its own project) or when `TOPOS_MCP_FILE_ROOT` is
/// set (the user chose the base explicitly), so those return `None` and the
/// common case stays silent.
pub fn resolution_note(requested: &str, resolved: &Path) -> Option<String> {
    if Path::new(requested).is_absolute() || matches!(configured_file_root(), Ok(Some(_))) {
        return None;
    }
    let base = startup_project_root()
        .map(|root| root.display().to_string())
        .unwrap_or_else(|_| "the server's startup directory".to_string());
    Some(format!(
        "Resolved the relative path `{requested}` to {} against the server's startup \
         project ({base}). If that is not the repository you meant, pass an absolute path.",
        resolved.display()
    ))
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

    /// `cargo test` is threaded and `set_var` is process-global, so every test
    /// that reads or writes `TOPOS_MCP_FILE_ROOT` takes this. Without it one
    /// test's configured root silently becomes another's answer.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Run `body` with `TOPOS_MCP_FILE_ROOT` set to `value`, then restore it.
    ///
    /// The caller must already hold [`ENV_LOCK`] — std's `Mutex::try_lock`
    /// cannot be used to assert that, since it is undefined for a mutex the
    /// current thread already holds. This is a *writer*, and
    /// locking only writers is not enough: every test in this module resolves a
    /// real path, which reads the same process-global variable, so a reader
    /// running concurrently would observe the temporary root. Locking writers
    /// alone makes `cargo test` flaky rather than safe, which is exactly what
    /// happened the first time.
    fn with_file_root<T>(value: Option<&Path>, body: impl FnOnce() -> T) -> T {
        let saved = std::env::var_os("TOPOS_MCP_FILE_ROOT");
        match value {
            Some(value) => std::env::set_var("TOPOS_MCP_FILE_ROOT", value),
            None => std::env::remove_var("TOPOS_MCP_FILE_ROOT"),
        }
        let result = body();
        match saved {
            Some(saved) => std::env::set_var("TOPOS_MCP_FILE_ROOT", saved),
            None => std::env::remove_var("TOPOS_MCP_FILE_ROOT"),
        }
        result
    }

    /// A `..` escape out of the startup project is still refused. The message
    /// changed and says more: it used to be "pass an absolute path", which
    /// described the symptom rather than the refusal, whereas this names the
    /// boundary the path had to stay inside.
    #[test]
    fn escape_via_dotdot_is_denied() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let err = resolve_within_root(&format!(
            "{}etc/passwd",
            "../".repeat(startup_project_root().unwrap().components().count())
        ))
        .unwrap_err();
        assert!(err.contains("Access denied"), "{err}");
        assert!(err.contains("/etc/passwd"), "{err}");
    }

    /// The behaviour this change exists for: a relative path inside the project
    /// the server was started in resolves, instead of being refused for not
    /// being absolute.
    #[test]
    fn a_relative_path_inside_the_startup_project_resolves() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = crate::security::startup_project_root().expect("tests run inside a crate");
        let resolved = resolve_within_root("src/security.rs")
            .expect("a relative path inside the project should resolve");
        assert!(resolved.is_absolute(), "{resolved:?}");
        assert!(resolved.starts_with(&root), "{resolved:?} escaped {root:?}");
        assert!(resolved.is_file(), "{resolved:?}");
    }

    /// And a relative path that leaves it is refused, by containment rather
    /// than by shape.
    #[test]
    fn a_relative_path_that_escapes_the_startup_project_is_denied() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let err = resolve_within_root(&format!(
            "{}etc/hosts",
            "../".repeat(startup_project_root().unwrap().components().count())
        ))
        .unwrap_err();
        assert!(err.contains("Access denied"), "{err}");
        assert!(
            !err.contains("absolute path is required"),
            "the refusal should name the boundary, not demand an absolute path: {err}"
        );
    }

    /// A configured root still wins over the startup directory, and a path
    /// outside it is still refused — the configured boundary is a maximum, not
    /// a default.
    ///
    /// The root here is the git root itself, because that is what a configured
    /// root has to be: pointing it at a subdirectory of the repository makes
    /// the project-root check fire, since the project is legitimately above
    /// the boundary.
    #[test]
    fn a_configured_root_still_bounds_every_path() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let project = std::env::temp_dir().join(format!("topos-bound-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("topos-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&project);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::write(project.join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(outside.join(".git")).unwrap();
        std::fs::write(outside.join("other.rs"), "fn main() {}\n").unwrap();
        let project = project.canonicalize().unwrap();
        let outside = outside.canonicalize().unwrap();

        with_file_root(Some(&project), || {
            assert_eq!(file_root_with_source().1, RootSource::Configured);

            // Inside, as a relative path and as an absolute one: both fine.
            assert_eq!(
                resolve_within_root("main.rs").expect("inside the configured root"),
                project.join("main.rs")
            );
            let absolute = project.join("main.rs").to_string_lossy().into_owned();
            assert_eq!(
                resolve_within_root(&absolute).expect("absolute, inside"),
                project.join("main.rs")
            );

            // Outside: refused, even though it is itself a valid project.
            let escaped = outside.join("other.rs").to_string_lossy().into_owned();
            let err = resolve_within_root(&escaped)
                .expect_err("a configured root is a maximum, not a default");
            assert!(err.contains("Access denied"), "{err}");
            assert!(err.contains(&project.display().to_string()), "{err}");
        });

        let _ = std::fs::remove_dir_all(&project);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// The report has to distinguish "configured" from "guessed", because a
    /// guessed root can be the wrong repository and that is invisible otherwise.
    #[test]
    fn the_root_source_is_reported() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        with_file_root(None, || {
            assert_eq!(
                file_root_with_source().1,
                RootSource::StartupCwd,
                "tests run inside a crate, so there is always a project above cwd"
            );
        });
        assert_eq!(RootSource::Configured.describe(), "TOPOS_MCP_FILE_ROOT");
        assert!(RootSource::StartupCwd
            .describe()
            .contains("working directory"));
    }

    #[test]
    fn absolute_file_derives_its_containing_project() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        // Reads the env var, so it holds the lock like every other resolving
        // test here.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
