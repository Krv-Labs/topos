//! `topos coverage` — structural (UAST) test coverage between a
//! program-under-test and its test suite.
//!
//! Calls straight into [`structural_test_coverage::declaration_coverage`] and
//! [`policies::coverage::score_declaration_coverage`]. Structured coverage is
//! available through MCP; this CLI command intentionally stays human-readable.

use std::path::PathBuf;

use clap::Args;
use console::Style;
use topos_engine::evaluation::policies::coverage::score_declaration_coverage;
use topos_engine::functors::profunctors::uast::structural_test_coverage::declaration_coverage;
use topos_engine::graphs::ast::dispatch::parse_source;
use topos_engine::graphs::ast::languages::SUPPORTED_LANGUAGES;
use topos_engine::graphs::uast::models::UASTNode;

use super::render::{guide, guide_line, paint, RenderOptions};

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

    let options = RenderOptions::stdout();
    let pass = decision.coverage_rate >= decision.threshold;
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
                args.language,
                args.put_paths.len(),
                args.test_paths.len(),
                report.k
            ),
            Style::new().dim(),
            options,
        )
    );
    println!(
        "{}",
        guide_line(
            format!("sources  {}", display_paths(&args.put_paths)),
            Style::new().dim(),
            options,
        )
    );
    println!(
        "{}",
        guide_line(
            format!("tests    {}", display_paths(&args.test_paths)),
            Style::new().dim(),
            options,
        )
    );
    println!("{}", guide('│', options));
    println!(
        "{}",
        guide_line(
            format!(
                "{symbol} {status} · {:.1}% declaration coverage",
                decision.coverage_rate * 100.0
            ),
            status_style,
            options,
        )
    );
    println!(
        "{}",
        guide_line(
            format!(
                "threshold {:.0}% · F2 {:.4}",
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
