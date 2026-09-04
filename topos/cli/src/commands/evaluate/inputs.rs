//! Source-file discovery and language resolution for `topos evaluate`.
//!
//! Default behaviour (no `--language`): discover every supported suffix and
//! parse each file with [`crate::commands::lang::detect_language`] — the same
//! multi-language shape MCP project evaluate already uses.
//!
//! `--language X` is an optional **filter** (monorepo “just the Python”), not
//! a parse requirement. Explicitly named paths that exist but fail the filter
//! are hard errors instead of silent drops (issue #289).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use topos_engine::adapters::discovery::collect_source_files;
use topos_engine::graphs::ast::languages::{
    all_source_suffixes, language_file_suffixes, SUPPORTED_LANGUAGES,
};

use crate::commands::lang::detect_language;

/// One discovered source file plus the language used to parse it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInput {
    pub path: PathBuf,
    pub language: String,
}

/// Human-readable language label for summary chrome (single lang, or count).
pub fn language_label(inputs: &[SourceInput]) -> String {
    let mut langs: BTreeSet<&str> = BTreeSet::new();
    for input in inputs {
        langs.insert(input.language.as_str());
    }
    match langs.len() {
        0 => "unknown".to_string(),
        1 => langs.into_iter().next().unwrap_or("unknown").to_string(),
        n => format!("{n} languages"),
    }
}

/// Resolve `--language` into discovery suffixes.
///
/// `None` → all supported suffixes. `Some(lang)` → that language only.
pub fn discovery_suffixes(language: Option<&str>) -> Result<Vec<&'static str>, String> {
    match language {
        None => Ok(all_source_suffixes()),
        Some(lang) => {
            if !SUPPORTED_LANGUAGES.contains(&lang) {
                return Err(format!(
                    "unsupported language '{lang}' (expected one of: {})",
                    SUPPORTED_LANGUAGES.join(", ")
                ));
            }
            Ok(language_file_suffixes(lang)
                .expect("checked against SUPPORTED_LANGUAGES")
                .to_vec())
        }
    }
}

/// Collect evaluate inputs from CLI paths.
///
/// * Directory / glob-style discovery uses `suffixes` + `recursive`.
/// * Explicit file paths that exist but are filtered out become errors
///   naming the real cause (wrong `--language`, or unsupported suffix when
///   no filter is set).
/// * Missing paths are reported explicitly (not “no python sources found”).
pub fn resolve_evaluate_inputs(
    paths: &[PathBuf],
    language_filter: Option<&str>,
    recursive: bool,
) -> Result<Vec<SourceInput>, String> {
    let suffixes = discovery_suffixes(language_filter)?;
    let mut missing = Vec::new();
    let mut filtered_out = Vec::new();
    let mut unsupported = Vec::new();

    for path in paths {
        if path.is_file() {
            if has_any_suffix(path, &suffixes) {
                continue;
            }
            if supported_suffix(path).is_some() {
                // Exists, supported language, but outside the active filter.
                filtered_out.push(path.clone());
            } else if language_filter.is_none() {
                unsupported.push(path.clone());
            } else {
                filtered_out.push(path.clone());
            }
            continue;
        }
        if path.is_dir() {
            continue;
        }
        // Neither file nor directory — typically does not exist.
        missing.push(path.clone());
    }

    if !missing.is_empty() {
        return Err(format!(
            "path{} not found: {}",
            if missing.len() == 1 { "" } else { "s" },
            join_paths(&missing)
        ));
    }
    if !unsupported.is_empty() {
        return Err(format!(
            "unsupported source suffix for {}: expected one of {}",
            join_paths(&unsupported),
            all_source_suffixes().join(", ")
        ));
    }
    if !filtered_out.is_empty() {
        let filter = language_filter.unwrap_or("python");
        let expected = language_file_suffixes(filter)
            .map(|s| s.join(", "))
            .unwrap_or_else(|| suffixes.join(", "));
        return Err(format!(
            "{} skipped: --language is '{filter}' (expected suffixes: {expected}); \
pass the matching --language or omit --language to evaluate all supported languages",
            join_paths(&filtered_out)
        ));
    }

    let files = collect_source_files(paths, &suffixes, recursive);

    Ok(files
        .into_iter()
        .map(|path| {
            let language = match language_filter {
                Some(lang) => lang.to_string(),
                None => detect_language(&path),
            };
            SourceInput { path, language }
        })
        .collect())
}

pub(crate) fn empty_discovery_message(
    language_filter: Option<&str>,
    suffixes: &[&str],
    paths: &[PathBuf],
    recursive: bool,
) -> String {
    let has_dir = paths.iter().any(|p| p.is_dir());
    let recursive_hint = if has_dir && !recursive {
        " or add --recursive"
    } else {
        ""
    };
    match language_filter {
        Some(lang) => format!(
            "no {lang} source files found (expected suffixes: {}){recursive_hint}",
            suffixes.join(", ")
        ),
        None => format!(
            "no supported source files found (expected suffixes: {}){recursive_hint}",
            suffixes.join(", ")
        ),
    }
}

fn has_any_suffix(path: &Path, suffixes: &[&str]) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    suffixes.iter().any(|suffix| {
        name.len() >= suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
    })
}

fn supported_suffix(path: &Path) -> Option<&'static str> {
    let name = path.file_name().and_then(|n| n.to_str())?;
    for language in SUPPORTED_LANGUAGES {
        if let Some(suffixes) = language_file_suffixes(language) {
            for suffix in suffixes {
                if name.len() >= suffix.len()
                    && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
                {
                    return Some(*suffix);
                }
            }
        }
    }
    None
}

fn join_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("topos-eval-inputs-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn named_rust_file_infers_language_without_filter() {
        let dir = tmp_dir("rs");
        let path = dir.join("main.rs");
        fs::write(&path, "fn main() {}\n").unwrap();

        let inputs = resolve_evaluate_inputs(std::slice::from_ref(&path), None, false).unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].language, "rust");
        assert_eq!(inputs[0].path, path);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn named_typescript_file_infers_language_without_filter() {
        let dir = tmp_dir("ts");
        let path = dir.join("app.ts");
        fs::write(&path, "const x: number = 1;\n").unwrap();

        let inputs = resolve_evaluate_inputs(std::slice::from_ref(&path), None, false).unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].language, "typescript");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mixed_named_files_both_scored_with_inferred_languages() {
        let dir = tmp_dir("mixed");
        let py = dir.join("a.py");
        let rs = dir.join("b.rs");
        fs::write(&py, "def f():\n    return 1\n").unwrap();
        fs::write(&rs, "fn f() -> i32 { 1 }\n").unwrap();

        let inputs = resolve_evaluate_inputs(&[py, rs], None, false).unwrap();
        assert_eq!(inputs.len(), 2);
        let langs: BTreeSet<_> = inputs.iter().map(|i| i.language.as_str()).collect();
        assert_eq!(langs, BTreeSet::from(["python", "rust"]));
        assert_eq!(language_label(&inputs), "2 languages");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn language_filter_keeps_only_matching_suffixes() {
        let dir = tmp_dir("filter");
        let py = dir.join("a.py");
        let rs = dir.join("b.rs");
        fs::write(&py, "x = 1\n").unwrap();
        fs::write(&rs, "fn main() {}\n").unwrap();

        let err = resolve_evaluate_inputs(&[py.clone(), rs], Some("python"), false).unwrap_err();
        assert!(err.contains("b.rs"), "{err}");
        assert!(err.contains("--language is 'python'"), "{err}");

        let inputs =
            resolve_evaluate_inputs(std::slice::from_ref(&py), Some("python"), false).unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].language, "python");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn directory_discovers_all_supported_languages() {
        let dir = tmp_dir("dir");
        fs::write(dir.join("a.py"), "x = 1\n").unwrap();
        fs::write(dir.join("b.go"), "package main\n").unwrap();
        fs::write(dir.join("c.txt"), "ignore\n").unwrap();

        let inputs = resolve_evaluate_inputs(std::slice::from_ref(&dir), None, false).unwrap();
        assert_eq!(inputs.len(), 2);
        let langs: BTreeSet<_> = inputs.iter().map(|i| i.language.as_str()).collect();
        assert_eq!(langs, BTreeSet::from(["go", "python"]));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_path_is_named_explicitly() {
        let missing = PathBuf::from("/tmp/topos-definitely-missing-289.rs");
        let err = resolve_evaluate_inputs(&[missing], None, false).unwrap_err();
        assert!(err.contains("not found"), "{err}");
        assert!(!err.contains("no python source"), "{err}");
    }

    #[test]
    fn unsupported_suffix_on_named_file_errors_clearly() {
        let dir = tmp_dir("bad");
        let path = dir.join("notes.md");
        fs::write(&path, "# hi\n").unwrap();

        let err = resolve_evaluate_inputs(&[path], None, false).unwrap_err();
        assert!(err.contains("unsupported source suffix"), "{err}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn discovery_suffixes_rejects_unknown_language() {
        let err = discovery_suffixes(Some("cobol")).unwrap_err();
        assert!(err.contains("unsupported language"), "{err}");
    }

    #[test]
    fn empty_directory_returns_empty_inputs_without_error() {
        let dir = tmp_dir("empty_dir");
        fs::write(dir.join("config.json"), "{}").unwrap();
        fs::write(dir.join("README.md"), "# docs").unwrap();

        let inputs = resolve_evaluate_inputs(std::slice::from_ref(&dir), None, false).unwrap();
        assert!(inputs.is_empty());

        fs::remove_dir_all(&dir).ok();
    }
}
