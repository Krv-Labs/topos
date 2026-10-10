//! `topos compare` — structural distance between two program files.
//!
//! Ported from `topos/cli/commands/quality.py::compare`, calling
//! straight into [`topos_engine::functors::profunctors::ast::compare`].

use std::path::PathBuf;

use clap::Args;
use console::Style;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::functors::profunctors::ast::compare::calculate_ast_distance;
use topos_engine::graphs::ast::languages::shared_language;

use super::render::{guide, guide_line, paint, RenderOptions};

#[derive(Args)]
pub struct CompareArgs {
    /// The first file.
    pub source: PathBuf,
    /// The second file.
    pub target: PathBuf,
    /// Show detailed operation-count breakdown.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

pub fn run(args: CompareArgs) -> Result<(), String> {
    let language = shared_language(
        &args.source.to_string_lossy(),
        &args.target.to_string_lossy(),
    )?;
    let source_morph = ProgramMorphism::from_file(&args.source, language)
        .map_err(|e| format!("reading {}: {e}", args.source.display()))?;
    let target_morph = ProgramMorphism::from_file(&args.target, language)
        .map_err(|e| format!("reading {}: {e}", args.target.display()))?;

    let (Some(source_ast), Some(target_ast)) = (&source_morph.ast, &target_morph.ast) else {
        return Err("failed to parse one or both files".to_string());
    };

    let result = calculate_ast_distance(source_ast, target_ast);

    let options = RenderOptions::stdout();
    let similarity = (1.0 - result.normalized_distance) * 100.0;
    println!(
        "{}",
        paint("◇  Compared 2 files", Style::new().bold(), options)
    );
    println!(
        "{}",
        guide_line(
            format!("{} → {}", args.source.display(), args.target.display()),
            Style::new().dim(),
            options,
        )
    );
    println!("{}", guide('│', options));
    println!(
        "{}",
        guide_line(
            format!("Similarity   {similarity:>5.1}%"),
            Style::new().bold(),
            options,
        )
    );
    println!(
        "{}",
        guide_line(
            format!("Edit distance  {}", result.raw_distance),
            Style::new(),
            options,
        )
    );

    if args.verbose {
        println!("{}", guide('│', options));
        println!(
            "{}",
            guide_line("OPERATIONS", Style::new().cyan().bold(), options)
        );
        for kind in ["insertions", "deletions", "substitutions"] {
            let count = result.operations.get(kind).copied().unwrap_or(0);
            println!(
                "{}",
                guide_line(format!("{kind:<14} {count}"), Style::new(), options)
            );
        }
    }
    println!("{}", guide('└', options));
    Ok(())
}
