//! Detect a `topos-core` language identifier from a file extension.
//!
//! Ported from `topos.mcp.evaluation.detect_language`: the CLI's
//! `inspect` command takes a single file path without a
//! `--language` flag, so the language has to come from the file's own
//! suffix. Falls back to `"python"` when the suffix is unrecognized,
//! matching the Python original's default.

use std::path::Path;

use topos_engine::graphs::ast::languages::language_for_path;

pub fn detect_language(path: &Path) -> String {
    language_for_path(&path.to_string_lossy())
        .unwrap_or("python")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_extensions() {
        assert_eq!(detect_language(Path::new("main.rs")), "rust");
        assert_eq!(detect_language(Path::new("app.py")), "python");
        assert_eq!(detect_language(Path::new("widget.tsx")), "typescript");
    }

    #[test]
    fn unknown_extension_defaults_to_python() {
        assert_eq!(detect_language(Path::new("PROGRAM.cbl")), "python");
        assert_eq!(detect_language(Path::new("noext")), "python");
    }
}
