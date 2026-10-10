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

    // Same rule as `topos_compare_files`: an error-recovered tree is not a
    // successful parse, and the message names the file and the language.
    if !source_morph.is_valid() || !target_morph.is_valid() {
        let failed = [
            (!source_morph.is_valid()).then(|| args.source.display().to_string()),
            (!target_morph.is_valid()).then(|| args.target.display().to_string()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" and ");
        return Err(format!("Failed to parse {failed} as {language}."));
    }
    let (Some(source_ast), Some(target_ast)) = (&source_morph.ast, &target_morph.ast) else {
        return Err(format!(
            "Failed to parse {} and {} as {language}.",
            args.source.display(),
            args.target.display()
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_error_names_the_file_and_language() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.rs");
        let broken = dir.path().join("broken.rs");
        std::fs::write(&ok, "fn ok() {}\n").unwrap();
        std::fs::write(&broken, "fn broken( {\n").unwrap();
        let err = run(CompareArgs {
            source: ok,
            target: broken,
            verbose: false,
        })
        .unwrap_err();
        assert!(
            err.contains("broken.rs") && err.contains("as rust"),
            "{err}"
        );
    }
}
