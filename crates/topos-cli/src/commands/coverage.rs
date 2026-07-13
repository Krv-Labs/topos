//! `topos coverage` — structural (UAST) test coverage between a
//! program-under-test and its test suite.
//!
//! Ported from `topos/cli/commands/coverage.py`, calling straight into
//! [`structural_test_coverage::declaration_coverage`] and
//! [`policies::coverage::score_declaration_coverage`]. The Python
//! original's JSON output mode and MCP-schema-shaped payload are not
//! ported here — this pass is plain-text only (see issue #147 scope).

use std::path::PathBuf;

use clap::Args;
use topos_core::evaluation::policies::coverage::score_declaration_coverage;
use topos_core::functors::profunctors::uast::structural_test_coverage::declaration_coverage;
use topos_core::graphs::ast::dispatch::parse_source;
use topos_core::graphs::ast::languages::SUPPORTED_LANGUAGES;
use topos_core::graphs::uast::models::UASTNode;

#[derive(Args)]
pub struct CoverageArgs {
    /// Program-under-test file(s).
    #[arg(required = true)]
    pub put_paths: Vec<PathBuf>,
    /// Test file path (repeat for multiple test modules).
    #[arg(long = "tests", required = true)]
    pub test_paths: Vec<PathBuf>,
    /// Language for tree-sitter / UAST parsing of all listed files.
    #[arg(long, default_value = "python")]
    pub language: String,
    /// Length of each DFS kind n-gram for path recall.
    #[arg(long = "k", default_value_t = 3)]
    pub kgram_length: usize,
    /// Count Unknown UAST kinds in histograms and k-grams.
    #[arg(long)]
    pub include_unknown: bool,
    /// Minimum threshold for coverage policies to pass.
    #[arg(long, default_value_t = 0.5)]
    pub coverage_threshold: f64,
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
    if !SUPPORTED_LANGUAGES.contains(&args.language.as_str()) {
        return Err(format!(
            "unsupported language '{}' (expected one of: {})",
            args.language,
            SUPPORTED_LANGUAGES.join(", ")
        ));
    }

    let put_roots = parse_uast_roots(&args.put_paths, &args.language)?;
    let test_roots = parse_uast_roots(&args.test_paths, &args.language)?;
    let put_refs: Vec<&UASTNode> = put_roots.iter().collect();
    let test_refs: Vec<&UASTNode> = test_roots.iter().collect();

    let report = declaration_coverage(
        &put_refs,
        &test_refs,
        args.kgram_length,
        args.include_unknown,
    )
    .map_err(|e| e.to_string())?;
    let decision = score_declaration_coverage(&report, args.coverage_threshold);

    println!("Topos Structural Test Coverage");
    println!("Language: {}", args.language);
    println!(
        "PUT files ({}): {}",
        args.put_paths.len(),
        display_paths(&args.put_paths)
    );
    println!(
        "Test files ({}): {}",
        args.test_paths.len(),
        display_paths(&args.test_paths)
    );
    println!();

    println!("UAST Declaration-Level Coverage");
    println!("{}", "-".repeat(52));
    println!(
        "  Mean declaration coverage:  {:.4}",
        report.mean_declaration_coverage
    );
    println!(
        "  Declaration coverage rate:  {:.4}",
        decision.coverage_rate
    );
    println!("  Coverage threshold:         {:.2}", decision.threshold);
    println!(
        "  PUT declarations:           {}",
        report.put_declaration_count
    );
    println!(
        "  Test declarations:          {}",
        report.test_declaration_count
    );
    println!();

    println!("Category-Stratified Recall (disjoint)");
    println!("{}", "-".repeat(52));
    println!("  Statement recall:           {:.4}", report.stmt_recall);
    println!("  Expression recall:          {:.4}", report.expr_recall);
    println!();

    println!("Precision and F-score");
    println!("{}", "-".repeat(52));
    println!(
        "  Mean test precision:        {:.4}",
        report.mean_test_precision
    );
    println!("  F2 score (beta=2):          {:.4}", decision.f2_score);
    println!();

    println!("Path Recall (declaration-scoped k={} grams)", report.k);
    println!("{}", "-".repeat(52));
    println!(
        "  Decl path recall:           {:.4}",
        report.declaration_path_recall_kgram
    );

    if !decision.uncovered_declarations.is_empty() {
        println!();
        println!(
            "Uncovered PUT declarations (below {:.0}%)",
            decision.threshold * 100.0
        );
        println!("{}", "-".repeat(52));
        let mut uncovered = decision.uncovered_declarations.clone();
        uncovered.sort_by(|a, b| a.1.total_cmp(&b.1));
        for (loc, score) in &uncovered {
            println!("  {loc}  (best score: {score:.3})");
        }
    } else {
        println!();
        println!("All measured declarations meet the threshold.");
    }

    Ok(())
}

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
