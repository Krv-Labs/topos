//! `topos evaluate` — run the characteristic morphism over one or more
//! files and print either per-file detail or a compact directory summary.
//!
//! The cumulative pillar table and bounded "Weak spots" list restore
//! the useful v0.3.12 summary shape. Suggestion and security-finding prose
//! remain separate engine capabilities rather than renderer concerns.
//!
//! # COMPOSABLE / GitNexus
//!
//! Unless `--no-composable` is passed, this command also attempts to
//! attach a [`ModuleDependencyGraph`] (COMPOSABLE generator): it checks
//! whether `<cwd>/.gitnexus` (or `--gitnexus-dir`) is present and fresh,
//! and if it's missing or stale, generates it by shelling out to
//! `gitnexus analyze --skip-agents-md` behind a stable spinner. That
//! resolve-or-generate decision is
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
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use topos_engine::adapters::discovery::collect_source_files;
use topos_engine::config::load_topos_config;
use topos_engine::core::characteristic_morphism::CharacteristicMorphism;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};

use super::classify::classify_with_representations;
use super::composable::resolve_composable_mdg;
use super::config::{parse_priority, parse_ranking, priority_for_generator};
use super::evaluate_info::show_evaluation_info;
use super::render::{print_classification, print_raw_metrics, spinner};
use super::summary::{json_output, print_summary};

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
    /// Show the full per-file classification and raw metrics.
    #[arg(short, long)]
    pub verbose: bool,
    /// Emit a machine-readable JSON document.
    #[arg(long)]
    pub json: bool,
    /// Select one of the five weakest files and show actionable line-level details.
    #[arg(long)]
    pub info: bool,
    /// Emphasize one quality pillar for result metadata and guidance.
    #[arg(long)]
    pub priority: Option<String>,
    /// Rank all pillars, most important first.
    #[arg(long, value_name = "SIMPLE,COMPOSABLE,SECURE")]
    pub preferences: Option<String>,
}

pub fn run(args: EvaluateArgs) -> Result<(), String> {
    if args.info && args.json {
        return Err("--info cannot be combined with --json".to_string());
    }
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

    let project_config = load_topos_config(&files[0]);
    let priority = resolve_priority(&args, &project_config)?;
    let target_ranking = resolve_target_ranking(&args, &project_config)?;

    let mut mdg = if args.no_composable {
        None
    } else {
        let spinner = spinner(args.json, "Indexing dependency graph");
        match std::env::current_dir() {
            Ok(project_root) => {
                let graph =
                    resolve_composable_mdg(&project_root, args.gitnexus_dir.as_deref(), true);
                spinner.finish_and_clear();
                graph
            }
            Err(e) => {
                spinner.finish_and_clear();
                eprintln!("gitnexus: could not resolve current directory ({e}); evaluating SIMPLE/SECURE only.");
                None
            }
        }
    };

    let classifier = CharacteristicMorphism;
    let mut results = Vec::with_capacity(files.len());
    let progress = progress_bar(files.len(), args.json);
    for file in &files {
        progress.set_message(file.file_name().map_or_else(
            || file.display().to_string(),
            |name| name.to_string_lossy().into(),
        ));
        let mut morphism = ProgramMorphism::from_file(file, args.language.clone())
            .map_err(|e| format!("reading {}: {e}", file.display()))?;
        if let Some(g) = mdg.as_mut() {
            g.target_file = file.to_string_lossy().into_owned();
        }
        let result =
            classify_with_representations(&classifier, &mut morphism, mdg.as_ref(), priority);
        results.push(result);
        progress.inc(1);
    }
    progress.finish_and_clear();

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json_output(&files, &results))
                .map_err(|e| format!("serializing evaluation: {e}"))?
        );
    } else {
        if args.verbose && results.len() > 1 {
            for (file, result) in files.iter().zip(&results) {
                println!("{}", file.display());
                print_classification(result);
                println!();
            }
        }
        print_summary(&files, &results, &args.language, !args.info);
        if args.verbose && results.len() == 1 {
            print_raw_metrics(&results[0]);
        }
        if args.info {
            show_evaluation_info(&files, &results, &args.language, target_ranking.as_ref())?;
        }
    }
    Ok(())
}

fn resolve_target_ranking(
    args: &EvaluateArgs,
    config: &topos_engine::config::ToposConfig,
) -> Result<Option<[topos_engine::evaluation::preferences::Generator; 3]>, String> {
    if let Some(ranking) = &args.preferences {
        return parse_ranking(ranking).map(Some);
    }
    if args.priority.is_some() {
        return Ok(None);
    }
    Ok(config.preferences)
}

fn resolve_priority(
    args: &EvaluateArgs,
    config: &topos_engine::config::ToposConfig,
) -> Result<Priority, String> {
    if let Some(ranking) = &args.preferences {
        return parse_ranking(ranking).map(|ranking| priority_for_generator(ranking[0]));
    }
    if let Some(priority) = &args.priority {
        return parse_priority(priority);
    }
    Ok(config.effective_priority())
}

fn progress_bar(len: usize, hidden: bool) -> ProgressBar {
    if hidden || len <= 1 {
        return ProgressBar::hidden();
    }
    let progress = ProgressBar::new(len as u64);
    progress.set_draw_target(ProgressDrawTarget::stderr());
    progress.set_style(
        ProgressStyle::with_template("Evaluating {bar:24.cyan/dim} {pos}/{len} {msg}")
            .expect("static progress template")
            .progress_chars("█▓░"),
    );
    progress
}
