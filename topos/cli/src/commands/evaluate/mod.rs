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
//! whether the COMPOSABLE project root's `.gitnexus` (default
//! `<cwd>/.gitnexus`, or `--gitnexus-dir` whose parent becomes the project
//! root) is present and fresh, and if it's missing or stale, generates it
//! by shelling out to `gitnexus analyze --skip-agents-md` behind a stable
//! spinner. That resolve-or-generate decision is shared with the MCP
//! evaluate tools via `topos_mcp::evaluation::ensure_gitnexus_dir`, so the
//! CLI and MCP server standardize on one policy. Any failure here —
//! GitNexus not installed, generation failing, a schema mismatch —
//! degrades gracefully to SIMPLE/SECURE only with a one-line `stderr`
//! notice; it never fails the whole evaluate run, matching how the MCP
//! tools treat COMPOSABLE as "not measured" rather than "failed" when
//! coupling data is unavailable.
//!
//! [`ModuleDependencyGraph`]: topos_engine::graphs::mdg::object::ModuleDependencyGraph
//! [`ProgramDependenceGraph`]: topos_engine::graphs::pdg::object::ProgramDependenceGraph

pub(crate) mod info;
pub(crate) mod info_render;
pub(crate) mod summary;

use std::path::PathBuf;

use clap::Args;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use topos_engine::adapters::discovery::collect_source_files;
use topos_engine::config::{load_topos_config, ToposConfig};
use topos_engine::core::characteristic_morphism::CharacteristicMorphism;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::preferences::Generator;
use topos_engine::graphs::ast::languages::{language_file_suffixes, SUPPORTED_LANGUAGES};

use super::classify::classify_with_representations;
use super::composable::resolve_composable_mdg;
use super::config::{parse_priority, parse_priority_input, priority_for_generator, PriorityInput};
use super::render::{print_classification, print_raw_metrics, spinner};

use self::info::{show_evaluation_info, show_pillar_failures};
use self::summary::{json_output, pillar_measured, print_summary};

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
    /// `.gitnexus` store path (default: `<cwd>/.gitnexus`). When set, COMPOSABLE
    /// freshness and `gitnexus analyze` use the store's parent directory as the
    /// project root — so `topos evaluate … --gitnexus-dir ~/repo/.gitnexus` from
    /// `$HOME` fingerprints `~/repo`, not `$HOME`.
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
    /// List files that fail one pillar; combine with --info to inspect them.
    #[arg(long, value_name = "PILLAR")]
    pub failures: Option<String>,
    /// Pillar to prioritize (simple, composable, secure), or a full
    /// comma-separated ranking, most important first.
    #[arg(long, value_name = "PILLAR|SIMPLE,COMPOSABLE,SECURE")]
    pub priority: Option<String>,
}

pub fn run(args: EvaluateArgs) -> Result<(), String> {
    if args.info && args.json {
        return Err("--info cannot be combined with --json".to_string());
    }
    if args.failures.is_some() && args.json {
        return Err("--failures cannot be combined with --json".to_string());
    }
    let failure_pillar = args
        .failures
        .as_deref()
        .map(parse_priority)
        .transpose()?
        .map(Priority::top_generator);
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
        let spinner = spinner(args.json, "Checking dependency graph freshness");
        match std::env::current_dir() {
            Ok(cwd) => {
                let project_root = topos_mcp::evaluation::resolve_composable_project_root(
                    args.gitnexus_dir.as_deref(),
                    &cwd,
                );
                // Resolved to an absolute path against `cwd` — must be used
                // here instead of `args.gitnexus_dir`, since `project_root`
                // above already absorbed a relative override's subdirectory;
                // rejoining the original relative string against it a second
                // time would double that subdirectory.
                let resolved_override = topos_mcp::evaluation::resolve_override_for_root(
                    args.gitnexus_dir.as_deref(),
                    &cwd,
                );
                let mut on_phase = |msg: &'static str| {
                    spinner.set_message(msg);
                };
                let graph = resolve_composable_mdg(
                    &project_root,
                    resolved_override.as_deref(),
                    true,
                    &mut on_phase,
                );
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

    if let Some(pillar) = failure_pillar {
        if !pillar_measured(&results, pillar.as_str()) {
            return Err(format!(
                "{} was not measured; check the evaluation inputs and COMPOSABLE availability",
                pillar.as_str().to_ascii_uppercase()
            ));
        }
    }

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
        print_summary(
            &files,
            &results,
            &args.language,
            !args.no_composable,
            !args.info && failure_pillar.is_none(),
        );
        if args.verbose && results.len() == 1 {
            print_raw_metrics(&results[0]);
        }
        if let Some(pillar) = failure_pillar {
            let focused = focused_ranking(pillar, target_ranking.as_ref());
            show_pillar_failures(
                &files,
                &results,
                &args.language,
                pillar.as_str(),
                args.info,
                Some(&focused),
            )?;
        } else if args.info {
            show_evaluation_info(&files, &results, &args.language, target_ranking.as_ref())?;
        }
    }
    Ok(())
}

fn focused_ranking(focus: Generator, configured: Option<&[Generator; 3]>) -> [Generator; 3] {
    let mut rest = configured
        .copied()
        .unwrap_or(Generator::ALL)
        .into_iter()
        .filter(|generator| *generator != focus);
    [
        focus,
        rest.next()
            .expect("three generators include two non-focus values"),
        rest.next()
            .expect("three generators include two non-focus values"),
    ]
}

fn resolve_target_ranking(
    args: &EvaluateArgs,
    config: &ToposConfig,
) -> Result<Option<[Generator; 3]>, String> {
    let Some(raw) = &args.priority else {
        return Ok(config.preferences.or_else(|| {
            config
                .priority
                .map(|priority| focused_ranking(priority.top_generator(), None))
        }));
    };
    Ok(Some(match parse_priority_input(raw)? {
        PriorityInput::Ranking(ranking) => ranking,
        PriorityInput::Single(priority) => {
            focused_ranking(priority.top_generator(), config.preferences.as_ref())
        }
    }))
}

fn resolve_priority(args: &EvaluateArgs, config: &ToposConfig) -> Result<Priority, String> {
    let Some(raw) = &args.priority else {
        return Ok(config.effective_priority());
    };
    Ok(match parse_priority_input(raw)? {
        PriorityInput::Single(priority) => priority,
        PriorityInput::Ranking(ranking) => priority_for_generator(ranking[0]),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_focus_moves_the_requested_pillar_first() {
        assert_eq!(
            focused_ranking(
                Generator::Secure,
                Some(&[Generator::Composable, Generator::Simple, Generator::Secure,])
            ),
            [Generator::Secure, Generator::Composable, Generator::Simple,]
        );
    }

    fn args_with_priority(priority: &str) -> EvaluateArgs {
        EvaluateArgs {
            paths: Vec::new(),
            recursive: false,
            language: "rust".to_string(),
            no_composable: false,
            gitnexus_dir: None,
            verbose: false,
            json: false,
            info: false,
            failures: None,
            priority: Some(priority.to_string()),
        }
    }

    #[test]
    fn single_priority_flag_reorders_persisted_preferences_instead_of_dropping_them() {
        let args = args_with_priority("secure");
        let config = ToposConfig {
            preferences: Some([Generator::Composable, Generator::Simple, Generator::Secure]),
            ..Default::default()
        };

        assert_eq!(resolve_priority(&args, &config).unwrap(), Priority::Secure);
        assert_eq!(
            resolve_target_ranking(&args, &config).unwrap(),
            Some([Generator::Secure, Generator::Composable, Generator::Simple])
        );
    }

    #[test]
    fn ranking_priority_flag_is_used_verbatim() {
        let args = args_with_priority("secure,simple,composable");
        let config = ToposConfig::default();

        assert_eq!(resolve_priority(&args, &config).unwrap(), Priority::Secure);
        assert_eq!(
            resolve_target_ranking(&args, &config).unwrap(),
            Some([Generator::Secure, Generator::Simple, Generator::Composable])
        );
    }

    #[test]
    fn configured_single_priority_infers_a_focused_target_ranking() {
        let mut args = args_with_priority("secure");
        args.priority = None;
        let config = ToposConfig {
            priority: Some(Priority::Secure),
            ..Default::default()
        };

        assert_eq!(
            resolve_target_ranking(&args, &config).unwrap(),
            Some([Generator::Secure, Generator::Simple, Generator::Composable])
        );
    }
}
