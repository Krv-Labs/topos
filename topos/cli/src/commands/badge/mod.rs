mod coverage;
mod model;
mod svg;

use std::path::{Path, PathBuf};

use super::evaluate::inputs::resolve_evaluate_inputs;
use super::evaluate::{evaluate_project, lease_inputs, EvaluateArgs};
use clap::{Args, ValueEnum};

use self::coverage::measured_rollup;
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
    /// Write the SVG here instead of printing it (`-` prints it).
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
    check_outputs(&args)?;
    let evaluate_args = EvaluateArgs {
        paths: args.paths.clone(),
        recursive: true,
        language: None,
        no_composable: args.no_composable,
        gitnexus_dir: args.gitnexus_dir.clone(),
        verbose: false,
        json: args.json,
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
    let rollup = measured_rollup(&evaluation.results);
    let warnings = [evaluation.warnings, rollup.warnings].concat();
    for warning in &warnings {
        eprintln!("warning: {warning}");
    }
    emit(
        &args,
        &Badge::from_rollup(&rollup.overall),
        inputs.len(),
        &warnings,
    )
}

fn svg_to_stdout(args: &BadgeArgs) -> bool {
    match &args.out {
        Some(path) => path.as_os_str() == "-",
        None => args.endpoint.is_none() && !args.json,
    }
}

fn svg_file(args: &BadgeArgs) -> Option<&Path> {
    args.out.as_deref().filter(|path| path.as_os_str() != "-")
}

fn check_outputs(args: &BadgeArgs) -> Result<(), String> {
    if args.json && svg_to_stdout(args) {
        return Err(
            "--out - cannot be combined with --json; both print to standard output".to_string(),
        );
    }
    let Some(endpoint) = &args.endpoint else {
        return Ok(());
    };
    if endpoint.as_os_str() == "-" {
        return Err("--endpoint needs a file path; only --out can print with `-`".to_string());
    }
    match svg_file(args) {
        Some(out) if same_file(out, endpoint) => Err(format!(
            "--out and --endpoint name the same file ({}); give each its own path",
            out.display()
        )),
        _ => Ok(()),
    }
}

fn same_file(left: &Path, right: &Path) -> bool {
    normalized(left) == normalized(right)
}

fn normalized(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if let Ok(existing) = absolute.canonicalize() {
        return existing;
    }
    match (absolute.parent(), absolute.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .map_or(absolute.clone(), |parent| parent.join(name)),
        _ => absolute,
    }
}

fn emit(args: &BadgeArgs, badge: &Badge, files: usize, warnings: &[String]) -> Result<(), String> {
    let image = match args.style {
        BadgeStyle::Lattice => svg::render_lattice(badge),
        BadgeStyle::Verdict => svg::render_verdict(badge),
    };
    let endpoint = format!("{}\n", pretty(&badge.endpoint_json())?);
    let written = [
        (args.endpoint.as_deref(), endpoint.as_str()),
        (svg_file(args), image.as_str()),
    ];
    for (path, contents) in written {
        let Some(path) = path else { continue };
        write_output(path, contents)?;
        eprintln!("Wrote {}", path.display());
    }
    if args.json {
        println!("{}", pretty(&badge.result_json(files, warnings))?);
    } else if svg_to_stdout(args) {
        print!("{image}");
    }
    if written.iter().any(|(path, _)| path.is_some()) {
        eprintln!(
            "{} · {} ({files} file{})",
            badge.medal(),
            badge.pillar_summary(),
            if files == 1 { "" } else { "s" }
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
