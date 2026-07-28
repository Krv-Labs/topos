//! `topos coverage` — structural (UAST) test coverage between a
//! program-under-test and its test suite.
//!
//! Calls straight into [`structural_test_coverage::declaration_coverage`] and
//! [`policies::coverage::score_declaration_coverage`]. Structured coverage is
//! available through MCP; this CLI command intentionally stays human-readable.

use std::collections::BTreeSet;
use std::path::PathBuf;

use clap::Args;
use console::Style;
use topos_engine::adapters::discovery::collect_source_files;
use topos_engine::evaluation::policies::coverage::score_declaration_coverage;
use topos_engine::functors::profunctors::uast::structural_test_coverage::declaration_coverage;
use topos_engine::graphs::ast::dispatch::parse_source;
use topos_engine::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};
use topos_engine::graphs::uast::models::UASTNode;

use super::lang::detect_language;
use super::render::{guide, guide_line, paint, RenderOptions};

#[derive(Args)]
pub struct CoverageArgs {
    /// Source files or directories whose declarations should be covered.
    #[arg(required = true, value_name = "SOURCE_PATHS")]
    pub source_paths: Vec<PathBuf>,
    /// Test file or directory (repeat for multiple test paths).
    #[arg(long = "tests", required = true, value_name = "TEST_PATH")]
    pub test_paths: Vec<PathBuf>,
    /// Recursively discover source and test files in directories.
    #[arg(short = 'r', long)]
    pub recursive: bool,
    /// Language for parsing; inferred when every discovered file agrees.
    #[arg(long)]
    pub language: Option<String>,
    /// Length of each DFS kind n-gram for path recall.
    #[arg(long = "k", default_value_t = 3)]
    pub kgram_length: usize,
    /// Count Unknown UAST kinds in histograms and k-grams.
    #[arg(long)]
    pub include_unknown: bool,
    /// Recall threshold for declarations and the required mean score.
    #[arg(long, default_value_t = 0.5)]
    pub coverage_threshold: f64,
}

fn infer_language(paths: &[PathBuf], recursive: bool) -> Result<String, String> {
    let all_suffixes: Vec<&str> = SUPPORTED_LANGUAGES
        .iter()
        .flat_map(|language| language_file_suffixes(language).unwrap_or_default())
        .copied()
        .collect();
    let files = collect_source_files(paths, &all_suffixes, recursive);
    let languages: BTreeSet<String> = files.iter().map(|path| detect_language(path)).collect();

    match languages.len() {
        0 => Err(
            "could not infer a supported language from the input paths; pass --language"
                .to_string(),
        ),
        1 => Ok(languages
            .into_iter()
            .next()
            .expect("one language checked above")
            .to_string()),
        _ => Err(format!(
            "multiple source languages found ({}); pass --language to select one",
            languages.into_iter().collect::<Vec<_>>().join(", ")
        )),
    }
}

fn collect_inputs(
    paths: &[PathBuf],
    language: &str,
    recursive: bool,
    label: &str,
) -> Result<Vec<PathBuf>, String> {
    let suffixes = language_file_suffixes(language).expect("language validated by caller");
    let files = collect_source_files(paths, suffixes, recursive);
    if files.is_empty() {
        return Err(format!(
            "no {language} {label} files found; check the paths{}",
            if paths.iter().any(|path| path.is_dir()) && !recursive {
                " or add --recursive"
            } else {
                ""
            }
        ));
    }
    Ok(files)
}

fn parse_uast_roots(paths: &[PathBuf], language: &str) -> Result<Vec<UASTNode>, String> {
    paths
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .map_err(|e| format!("reading {}: {e}", path.display()))?;
            let file = path.to_string_lossy().into_owned();
            let result = parse_source(&source, language, Some(&file))
                .map_err(|e| format!("parsing {}: {e}", path.display()))?;
            Ok(result.uast_root)
        })
        .collect()
}

pub fn run(args: CoverageArgs) -> Result<(), String> {
    if args.kgram_length < 1 {
        return Err("--k must be >= 1".to_string());
    }
    let language = if let Some(language) = args.language.as_deref() {
        if !SUPPORTED_LANGUAGES.contains(&language) {
            return Err(format!(
                "unsupported language '{language}' (expected one of: {})",
                SUPPORTED_LANGUAGES.join(", ")
            ));
        }
        language.to_string()
    } else {
        let mut paths = args.source_paths.clone();
        paths.extend(args.test_paths.iter().cloned());
        infer_language(&paths, args.recursive)?
    };

    let source_files = collect_inputs(&args.source_paths, &language, args.recursive, "source")?;
    let test_files = collect_inputs(&args.test_paths, &language, args.recursive, "test")?;
    let put_roots = parse_uast_roots(&source_files, &language)?;
    let test_roots = parse_uast_roots(&test_files, &language)?;
    let put_refs: Vec<&UASTNode> = put_roots.iter().collect();
    let test_refs: Vec<&UASTNode> = test_roots.iter().collect();

    let report = declaration_coverage(
        &put_refs,
        &test_refs,
        args.kgram_length,
        args.include_unknown,
    )
    .map_err(|e| e.to_string())?;
    if report.put_declaration_count == 0 {
        return Err(
            "no function or method declarations found in source files; check --language and source paths"
                .to_string(),
        );
    }
    if report.test_declaration_count == 0 {
        return Err(
            "no function or method declarations found in test files; check --language and test paths"
                .to_string(),
        );
    }
    let decision = score_declaration_coverage(&report, args.coverage_threshold);

    let options = RenderOptions::stdout();
    let pass = decision.achieved;
    let (symbol, status, status_style) = if pass {
        ("✓", "PASS", Style::new().green().bold())
    } else {
        ("X", "FAIL", Style::new().red().bold())
    };
    println!(
        "{}",
        paint("◇  Structural coverage", Style::new().bold(), options)
    );
    println!(
        "{}",
        guide_line(
            format!(
                "{} · {} source · {} test · k={}",
                language,
                source_files.len(),
                test_files.len(),
                report.k
            ),
            Style::new().dim(),
            options,
        )
    );
    println!(
        "{}",
        guide_line(
            format!("sources  {}", display_paths(&source_files)),
            Style::new().dim(),
            options,
        )
    );
    println!(
        "{}",
        guide_line(
            format!("tests    {}", display_paths(&test_files)),
            Style::new().dim(),
            options,
        )
    );
    println!("{}", guide('│', options));
    println!(
        "{}",
        guide_line(
            format!(
                "{symbol} {status} · {:.1}% mean declaration coverage",
                decision.score * 100.0
            ),
            status_style,
            options,
        )
    );
    println!(
        "{}",
        guide_line(
            format!(
                "{:.1}% of declarations meet the {:.0}% threshold · F2 {:.4}",
                decision.coverage_rate * 100.0,
                decision.threshold * 100.0,
                decision.f2_score
            ),
            Style::new().dim(),
            options,
        )
    );
    println!("{}", guide('│', options));
    println!(
        "{}",
        guide_line("METRICS", Style::new().cyan().bold(), options)
    );
    for (label, value) in [
        (
            "Mean declaration coverage",
            report.mean_declaration_coverage,
        ),
        ("Statement recall", report.stmt_recall),
        ("Expression recall", report.expr_recall),
        ("Mean test precision", report.mean_test_precision),
        (
            "Declaration path recall",
            report.declaration_path_recall_kgram,
        ),
    ] {
        println!(
            "{}",
            guide_line(format!("{label:<28} {value:.4}"), Style::new(), options)
        );
    }
    println!(
        "{}",
        guide_line(
            format!(
                "Declarations                 {} source · {} test",
                report.put_declaration_count, report.test_declaration_count
            ),
            Style::new(),
            options,
        )
    );

    if !decision.uncovered_declarations.is_empty() {
        println!("{}", guide('│', options));
        println!(
            "{}",
            guide_line(
                format!("UNCOVERED · BELOW {:.0}%", decision.threshold * 100.0),
                Style::new().cyan().bold(),
                options,
            )
        );
        let mut uncovered = decision.uncovered_declarations.clone();
        uncovered.sort_by(|a, b| a.1.total_cmp(&b.1));
        for (loc, score) in &uncovered {
            println!(
                "{}",
                guide_line(format!("{score:.3}  {loc}"), Style::new(), options)
            );
        }
    } else {
        println!("{}", guide('│', options));
        println!(
            "{}",
            guide_line(
                "All measured declarations meet the threshold.",
                Style::new().green(),
                options,
            )
        );
    }
    println!("{}", guide('└', options));

    Ok(())
}

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_tmp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_coverage_test_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn infers_one_language_from_source_and_test_files() {
        let dir = unique_tmp_dir("infer");
        let source = dir.join("lib.ts");
        let test = dir.join("lib.test.ts");
        std::fs::write(&source, "export function value() { return 1; }").unwrap();
        std::fs::write(&test, "function testValue() { return value(); }").unwrap();

        assert_eq!(
            infer_language(&[source, test], false).unwrap(),
            "typescript"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mixed_languages_require_an_explicit_selection() {
        let dir = unique_tmp_dir("mixed");
        let rust = dir.join("lib.rs");
        let python = dir.join("test_lib.py");
        std::fs::write(&rust, "fn value() -> i32 { 1 }").unwrap();
        std::fs::write(&python, "def test_value():\n    return 1\n").unwrap();

        let error = infer_language(&[rust, python], false).unwrap_err();
        assert!(error.contains("multiple source languages found"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn directory_inputs_use_shared_recursive_discovery() {
        let dir = unique_tmp_dir("recursive");
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("lib.rs"), "fn value() -> i32 { 1 }").unwrap();

        assert!(collect_inputs(std::slice::from_ref(&dir), "rust", false, "source").is_err());
        assert_eq!(
            collect_inputs(std::slice::from_ref(&dir), "rust", true, "source")
                .unwrap()
                .len(),
            1
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn zero_declaration_sources_are_rejected() {
        let dir = unique_tmp_dir("empty");
        let source = dir.join("lib.rs");
        let test = dir.join("lib_test.rs");
        std::fs::write(&source, "pub mod nested;\n").unwrap();
        std::fs::write(&test, "fn test_value() { assert_eq!(1, 1); }\n").unwrap();

        let error = run(CoverageArgs {
            source_paths: vec![source],
            test_paths: vec![test],
            recursive: false,
            language: Some("rust".to_string()),
            kgram_length: 3,
            include_unknown: false,
            coverage_threshold: 0.5,
        })
        .unwrap_err();
        assert!(error.contains("no function or method declarations found in source files"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
