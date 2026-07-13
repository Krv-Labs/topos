//! Detect a `topos-core` language identifier from a file extension.
//!
//! Ported from `topos.mcp.evaluation.detect_language`: the CLI's
//! `compare`/`inspect` commands take single file paths without a
//! `--language` flag, so the language has to come from the file's own
//! suffix. Falls back to `"python"` when the suffix is unrecognized,
//! matching the Python original's default.

use std::path::Path;

use topos_core::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};

pub fn detect_language(path: &Path) -> String {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return "python".to_string();
    };
    let dotted = format!(".{ext}");
    for language in SUPPORTED_LANGUAGES {
        if language_file_suffixes(language).is_some_and(|suf| suf.contains(&dotted.as_str())) {
            return language.to_string();
        }
    }
    "python".to_string()
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
