//! `topos evaluate` — run the characteristic morphism over one or more
//! files and print each verdict plus a directory-wide rollup.
//!
//! Ported from `topos/cli/commands/quality.py::evaluate` and
//! `topos/cli/evaluation.py`, scoped down per issue #147: this prints
//! scores + raw metrics only. The Python original's colored gauges,
//! "Lowest-hanging fruit"/"Needs attention" ranking tables, and the
//! per-file "Suggestions"/"Security Findings" sections
//! (`topos/cli/diagnostics.py`) all read from
//! `evaluation::{suggestions,suppression,security_guidance}`, which are
//! not yet ported to `topos-core` — follow-up work once those land.
//!
//! # Deviation from the Python original
//!
//! The Python CLI always attempts to attach a [`ModuleDependencyGraph`]
//! (COMPOSABLE generator) via `--gitnexus-dir`, shelling out to the
//! `gitnexus` CLI. This command builds the same three UAST-intrinsic
//! representations Python's `_intrinsic_representations` always attaches
//! — CFG, [`ProgramDependenceGraph`] (diagnostic-only; feeds no `Φᵢ` but
//! still contributes `pdg.*` to `raw_metrics`, see
//! [`ProgramDependenceGraph`]'s doc comment), and CPG — but not MDG,
//! since that needs an external `gitnexus` process.
//! `combine_dimensions`/`classify_detailed` already treat a missing
//! dimension as "not measured" rather than "failed", so the COMPOSABLE
//! row simply never appears in this pass's output. Wiring
//! `--gitnexus-dir` through to `adapters::gitnexus` is real follow-up
//! work, not a bug: the `depgraph` subcommand it depends on is
//! explicitly out of scope for issue #147.
//!
//! [`ModuleDependencyGraph`]: topos_core::graphs::mdg::object::ModuleDependencyGraph
//! [`ProgramDependenceGraph`]: topos_core::graphs::pdg::object::ProgramDependenceGraph

use std::path::PathBuf;

use clap::Args;
use topos_core::adapters::discovery::collect_source_files;
use topos_core::core::morphism::ProgramMorphism;
use topos_core::evaluation::characteristic_morphism::{
    CharacteristicMorphism, ClassificationResult,
};
use topos_core::evaluation::policies::base::Priority;
use topos_core::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};
use topos_core::graphs::base::Representation;

#[derive(Args)]
pub struct EvaluateArgs {
    /// Files or directories to evaluate.
    #[arg(required = true)]
    pub paths: Vec<PathBuf>,
    /// Recursively evaluate directories.
    #[arg(short = 'r', long)]
    pub recursive: bool,
    /// Source language for parsing and file discovery when paths are directories.
    #[arg(long, default_value = "python")]
    pub language: String,
}

pub fn run(args: EvaluateArgs) -> Result<(), String> {
    if !SUPPORTED_LANGUAGES.contains(&args.language.as_str()) {
        return Err(format!(
            "unsupported language '{}' (expected one of: {})",
            args.language,
            SUPPORTED_LANGUAGES.join(", ")
        ));
    }
    let suffixes =
        language_file_suffixes(&args.language).expect("checked against SUPPORTED_LANGUAGES above");
    let files = collect_source_files(&args.paths, suffixes, args.recursive);
    if files.is_empty() {
        return Err(format!(
            "no {} source files found (expected suffixes: {})",
            args.language,
            suffixes.join(", ")
        ));
    }

    let classifier = CharacteristicMorphism;
    let mut results = Vec::with_capacity(files.len());
    for file in &files {
        let mut morphism = ProgramMorphism::from_file(file, args.language.clone())
            .map_err(|e| format!("reading {}: {e}", file.display()))?;
        let result = classify_with_representations(&classifier, &mut morphism);
        println!("{}", file.display());
        print_classification(&result);
        println!();
        results.push(result);
    }

    if results.len() > 1 {
        println!("Directory rollup ({} files)", results.len());
        println!("{}", "-".repeat(40));
        let overall = classifier.combine_dimensions(&results);
        for dim in ["simple", "composable", "secure"] {
            if let Some(val) = overall.get(dim) {
                println!("  {dim}: {val}");
            }
        }
    }
    Ok(())
}

/// Build the CFG (SIMPLE), PDG (diagnostic), and CPG (SECURE)
/// representations for `morphism` and classify it — the same three
/// UAST-intrinsic representations Python's `_intrinsic_representations`
/// always attaches. See this module's "Deviation" note for why MDG
/// (COMPOSABLE) is not attached here.
pub(crate) fn classify_with_representations(
    classifier: &CharacteristicMorphism,
    morphism: &mut ProgramMorphism,
) -> ClassificationResult {
    let cfg = morphism.build_cfg().cloned();
    let pdg = morphism.build_pdg().cloned();
    let cpg = morphism.build_cpg().cloned();
    let mut representations: Vec<&dyn Representation> = Vec::new();
    if let Some(cfg) = &cfg {
        representations.push(cfg);
    }
    if let Some(pdg) = &pdg {
        representations.push(pdg);
    }
    if let Some(cpg) = &cpg {
        representations.push(cpg);
    }
    classifier.classify_detailed(morphism, &representations, Priority::default())
}

/// Print a verdict, per-generator scores, and raw metrics for one result.
pub(crate) fn print_classification(result: &ClassificationResult) {
    if !result.is_parseable {
        println!("  {}", result.summary());
        return;
    }
    println!("  Verdict: {}", result.summary());
    for dim in ["simple", "composable", "secure"] {
        let Some(val) = result.dimensions.get(dim) else {
            continue;
        };
        let score = result.scores.get(dim).copied().unwrap_or(0.0) * 100.0;
        println!("    {dim}: {val} [{score:.0}%]");
    }
    if !result.raw_metrics.is_empty() {
        println!("  Raw metrics:");
        let mut keys: Vec<&String> = result.raw_metrics.keys().collect();
        keys.sort();
        for key in keys {
            let value = result.raw_metrics[key];
            println!("    {key}: {value:.3}");
        }
    }
}
