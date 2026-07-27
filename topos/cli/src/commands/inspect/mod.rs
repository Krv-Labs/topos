//! `topos inspect` — detailed single-file metrics.
//!
//! Human output follows the same compact summary-first grammar as
//! [`super::evaluate`], then keeps the full metric evidence below it.

use std::path::PathBuf;

use clap::Args;
use topos_engine::config::load_topos_config;
use topos_engine::core::characteristic_morphism::CharacteristicMorphism;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::functors::probes::ast::complexity::calculate_function_complexity_entries;

mod detail;

use super::classify::classify_with_representations;
use super::composable::resolve_composable_mdg;
use super::evaluate::info::details_for_source;
use super::evaluate::summary::print_inspection_summary;
use super::lang::detect_language;
use super::render::{paint, print_lines, spinner, RenderOptions};

use self::detail::inspection_detail_lines;

#[derive(Args)]
pub struct InspectArgs {
    /// The file to inspect.
    pub path: PathBuf,
    /// Output a machine-readable inspection object. Human-only recommendations
    /// and security findings are not included.
    #[arg(long)]
    pub json: bool,
    /// Skip GitNexus detection/generation; inspect SIMPLE/SECURE only.
    #[arg(long)]
    pub no_composable: bool,
    /// Override the `.gitnexus` directory (default: `<cwd>/.gitnexus`).
    #[arg(long)]
    pub gitnexus_dir: Option<String>,
}

pub fn run(args: InspectArgs) -> Result<(), String> {
    let language = detect_language(&args.path);
    let config = load_topos_config(&args.path);
    let priority = config.effective_priority();
    let mut morphism = ProgramMorphism::from_file(&args.path, language.clone())
        .map_err(|e| format!("reading {}: {e}", args.path.display()))?;
    let classifier = CharacteristicMorphism;
    // Attach the COMPOSABLE MDG the same way `evaluate` does (auto-detect /
    // generate `<cwd>/.gitnexus`, or `--gitnexus-dir`), so `inspect` reports
    // COMPOSABLE too. Python's `inspect` fed `--gitnexus-dir` through the same
    // `classify_file` pipeline as evaluate; `--no-composable` opts out.
    let mut mdg = if args.no_composable {
        None
    } else {
        let progress = spinner(args.json, "Indexing dependency graph");
        match std::env::current_dir() {
            Ok(project_root) => {
                let graph =
                    resolve_composable_mdg(&project_root, args.gitnexus_dir.as_deref(), true);
                progress.finish_and_clear();
                graph
            }
            Err(e) => {
                progress.finish_and_clear();
                eprintln!(
                    "gitnexus: could not resolve current directory ({e}); inspecting SIMPLE/SECURE only."
                );
                None
            }
        }
    };
    if let Some(g) = mdg.as_mut() {
        g.target_file = args.path.to_string_lossy().into_owned();
    }
    let result = classify_with_representations(&classifier, &mut morphism, mdg.as_ref(), priority);

    if args.json {
        return print_json(&args.path, &result);
    }

    if !result.is_parseable {
        // Match Python's `print(...)` + `sys.exit(1)`: emit the SLOP line to
        // stdout, then exit non-zero — a parse failure is a CLI failure, so
        // `topos inspect broken.py` must fail a shell gate. (JSON mode above
        // returns 0 for an unparseable file, matching Python too.)
        let options = RenderOptions::stdout();
        println!(
            "{}",
            paint(
                format!("◇  Inspected {}", args.path.display()),
                console::Style::new().bold(),
                options,
            )
        );
        println!(
            "└  {} SLOP · parse failure",
            paint("X", console::Style::new().red().bold(), options)
        );
        std::process::exit(1);
    }

    let mut functions = morphism
        .ast
        .as_ref()
        .map(|ast| calculate_function_complexity_entries(&ast.uast_root, &morphism.source))
        .unwrap_or_default();
    functions.sort_by(|a, b| {
        b.complexity
            .cmp(&a.complexity)
            .then_with(|| a.qualified_name.cmp(&b.qualified_name))
    });
    let details = details_for_source(
        &args.path,
        &result,
        &morphism.source,
        &language,
        config.preferences.as_ref(),
    );
    print_inspection_summary(&args.path, &result, &language);
    print_lines(inspection_detail_lines(
        &result,
        &functions,
        &details,
        RenderOptions::stdout(),
    ));

    Ok(())
}

/// Field names match Python's `topos inspect --json` where this pass
/// of the CLI has the data to fill them; see this module's doc comment
/// for the fields intentionally omitted.
fn print_json(
    path: &std::path::Path,
    result: &topos_engine::core::characteristic_morphism::ClassificationResult,
) -> Result<(), String> {
    let dimensions: serde_json::Map<String, serde_json::Value> = result
        .dimensions
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.name().to_string())))
        .collect();
    let scores: serde_json::Map<String, serde_json::Value> = result
        .scores
        .iter()
        .map(|(k, s)| {
            // Python emits `round(s * 100.0, 1)` (0–100, one decimal); the
            // engine stores 0–1, so scale to match for parity/machine consumers.
            let scaled = (*s * 1000.0).round() / 10.0;
            let value = serde_json::Number::from_f64(scaled)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null);
            (k.clone(), value)
        })
        .collect();
    let payload = serde_json::json!({
        "file": path.display().to_string(),
        "is_parseable": result.is_parseable,
        "lattice_element": result.lattice_element.name(),
        "dimensions": dimensions,
        "scores": scores,
        "raw_metrics": result.raw_metrics,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?
    );
    Ok(())
}
