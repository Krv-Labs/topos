//! Lightweight language constants for CLI option wiring and source
//! discovery (no parser backends).

pub const SUPPORTED_LANGUAGES: &[&str] =
    &["python", "rust", "javascript", "typescript", "cpp", "go"];

/// Filename suffixes associated with `language`, for source discovery.
///
/// `None` if `language` isn't in [`SUPPORTED_LANGUAGES`] — the Python
/// original raises `ValueError`; this crate's public API prefers `Option`/
/// `Result` over panics (see the workspace-layout rationale in
/// `crate::core::omega`'s `OmegaError`).
pub fn language_file_suffixes(language: &str) -> Option<&'static [&'static str]> {
    let suffixes: &[&str] = match language {
        "python" => &[".py"],
        "rust" => &[".rs"],
        "javascript" => &[".js", ".mjs", ".cjs"],
        "typescript" => &[".ts", ".tsx"],
        "cpp" => &[".cpp", ".cc", ".cxx", ".hpp", ".hh", ".hxx"],
        "go" => &[".go"],
        _ => return None,
    };
    Some(suffixes)
}

/// Every supported source suffix, sorted and deduped.
///
/// Used by multi-language discovery (`evaluate` default, MCP project
/// evaluate, GitNexus fingerprinting) so CLI and MCP share one list.
pub fn all_source_suffixes() -> Vec<&'static str> {
    let mut all: Vec<&str> = SUPPORTED_LANGUAGES
        .iter()
        .filter_map(|lang| language_file_suffixes(lang))
        .flat_map(|group| group.iter().copied())
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_language_returns_suffixes() {
        assert_eq!(language_file_suffixes("python"), Some([".py"].as_slice()));
    }

    #[test]
    fn unknown_language_returns_none() {
        assert_eq!(language_file_suffixes("cobol"), None);
    }

    #[test]
    fn all_source_suffixes_covers_each_supported_language() {
        let all = all_source_suffixes();
        for language in SUPPORTED_LANGUAGES {
            for suffix in language_file_suffixes(language).unwrap() {
                assert!(
                    all.contains(suffix),
                    "missing {suffix} from all_source_suffixes"
                );
            }
        }
        assert!(all.windows(2).all(|w| w[0] < w[1]));
    }
}
