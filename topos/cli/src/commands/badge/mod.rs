mod model;
mod svg;

use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use topos_engine::core::characteristic_morphism::CharacteristicMorphism;

use super::evaluate::inputs::resolve_evaluate_inputs;
use super::evaluate::{evaluate_project, lease_inputs, EvaluateArgs};

use self::model::Badge;

#[derive(Clone, Copy, ValueEnum)]
pub enum BadgeStyle {
    /// One chip per pillar, colored when the pillar passes.
    Lattice,
    /// The Topos mark beside the medal.
    Verdict,
}

#[derive(Args)]
pub struct BadgeArgs {
    /// Files or directories to evaluate; directories are searched recursively.
    #[arg(default_value = ".")]
    pub paths: Vec<PathBuf>,
    /// Write the SVG here instead of printing it.
    #[arg(short, long, value_name = "PATH")]
    pub out: Option<PathBuf>,
    /// Write a shields.io endpoint JSON document here.
    #[arg(long, value_name = "PATH")]
    pub endpoint: Option<PathBuf>,
    /// Badge design.
    #[arg(long, value_enum, default_value = "lattice")]
    pub style: BadgeStyle,
    /// Print the medal and pillar results as JSON.
    #[arg(long)]
    pub json: bool,
    /// Skip GitNexus detection/generation; COMPOSABLE shows as not measured.
    #[arg(long)]
    pub no_composable: bool,
    /// `.gitnexus` store path (default: the project's own `.gitnexus`).
    #[arg(long)]
    pub gitnexus_dir: Option<String>,
}

pub fn run(args: BadgeArgs) -> Result<(), String> {
    let evaluate_args = EvaluateArgs {
        paths: args.paths.clone(),
        recursive: true,
        language: None,
        no_composable: args.no_composable,
        gitnexus_dir: args.gitnexus_dir.clone(),
        verbose: false,
        json: true,
        info: false,
        failures: None,
        priority: None,
    };
    let inputs = resolve_evaluate_inputs(&evaluate_args.paths, None, true)?;
    if inputs.is_empty() {
        return Err("no supported source files found; nothing to award a badge to".to_string());
    }
    let _cache_leases = lease_inputs(&inputs)?;
    let evaluation = evaluate_project(&evaluate_args, &inputs)?;
    for warning in &evaluation.warnings {
        eprintln!("warning: {warning}");
    }
    let badge = Badge::from_rollup(&CharacteristicMorphism.combine_dimensions(&evaluation.results));
    emit(&args, &badge, inputs.len(), &evaluation.warnings)
}

fn emit(args: &BadgeArgs, badge: &Badge, files: usize, warnings: &[String]) -> Result<(), String> {
    let image = match args.style {
        BadgeStyle::Lattice => svg::render_lattice(badge),
        BadgeStyle::Verdict => svg::render_verdict(badge),
    };
    if let Some(path) = &args.endpoint {
        write_output(path, &format!("{}\n", pretty(&badge.endpoint_json())?))?;
    }
    if let Some(path) = &args.out {
        write_output(path, &image)?;
    }
    if args.json {
        println!("{}", pretty(&badge.result_json(files, warnings))?);
    } else if args.out.is_none() && args.endpoint.is_none() {
        print!("{image}");
    } else {
        eprintln!(
            "{} · {} ({files} files)",
            badge.medal(),
            badge.pillar_summary()
        );
    }
    Ok(())
}

fn pretty(value: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|error| format!("serializing badge: {error}"))
}

fn write_output(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    std::fs::write(path, contents).map_err(|error| format!("writing {}: {error}", path.display()))
}
