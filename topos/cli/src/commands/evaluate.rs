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
//! # COMPOSABLE / GitNexus
//!
//! Unless `--no-composable` is passed, this command also attempts to
//! attach a [`ModuleDependencyGraph`] (COMPOSABLE generator): it checks
//! whether `<cwd>/.gitnexus` (or `--gitnexus-dir`) is present and fresh,
//! and if it's missing or stale, generates it by shelling out to
//! `gitnexus analyze --skip-agents-md` (streaming its output live, since
//! generation can take a while). That resolve-or-generate decision is
//! shared with the MCP evaluate tools via
//! `topos_mcp::evaluation::ensure_gitnexus_dir`, so the CLI and MCP
//! server standardize on one policy. Any failure here — GitNexus not
//! installed, generation failing, a schema mismatch — degrades
//! gracefully to SIMPLE/SECURE only with a one-line `stderr` notice; it
//! never fails the whole evaluate run, matching how the MCP tools treat
//! COMPOSABLE as "not measured" rather than "failed" when coupling data
//! is unavailable.
//!
//! [`ModuleDependencyGraph`]: topos_engine::graphs::mdg::object::ModuleDependencyGraph
//! [`ProgramDependenceGraph`]: topos_engine::graphs::pdg::object::ProgramDependenceGraph

use std::path::PathBuf;

use clap::Args;
use topos_engine::adapters::discovery::collect_source_files;
use topos_engine::core::characteristic_morphism::CharacteristicMorphism;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};

use super::classify::classify_with_representations;
use super::composable::resolve_composable_mdg;
use super::render::print_classification;

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
    /// Skip GitNexus detection/generation; score SIMPLE/SECURE only.
    #[arg(long)]
    pub no_composable: bool,
    /// Override the `.gitnexus` directory (default: `<cwd>/.gitnexus`).
    #[arg(long)]
    pub gitnexus_dir: Option<String>,
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

    let mut mdg = if args.no_composable {
        None
    } else {
        match std::env::current_dir() {
            Ok(project_root) => resolve_composable_mdg(&project_root, args.gitnexus_dir.as_deref()),
            Err(e) => {
                eprintln!("gitnexus: could not resolve current directory ({e}); evaluating SIMPLE/SECURE only.");
                None
            }
        }
    };

    let classifier = CharacteristicMorphism;
    let mut results = Vec::with_capacity(files.len());
    for file in &files {
        let mut morphism = ProgramMorphism::from_file(file, args.language.clone())
            .map_err(|e| format!("reading {}: {e}", file.display()))?;
        if let Some(g) = mdg.as_mut() {
            g.target_file = file.to_string_lossy().into_owned();
        }
        let result = classify_with_representations(&classifier, &mut morphism, mdg.as_ref());
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
