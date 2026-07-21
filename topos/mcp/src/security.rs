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

/// Resolve a path (absolute or root-relative) and check it's inside the
/// root, without reading it. Symlinks are resolved when the path exists.
pub fn resolve_within_root(filepath: &str) -> Result<PathBuf, String> {
    let root = resolve_file_root()?;
    let path = Path::new(filepath);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let resolved = joined.canonicalize().unwrap_or_else(|_| normalize(&joined));
    if resolved.starts_with(&root) {
        Ok(resolved)
    } else {
        Err(format!(
            "Access denied: path must be inside {}. Got: {}",
            root.display(),
            resolved.display()
        ))
    }
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
}
