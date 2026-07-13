//! `topos inspect` — detailed single-file metrics.
//!
//! Ported from `topos/cli/commands/quality.py::inspect`, scoped down
//! per issue #147: the security-findings / suggestions / suppression
//! sections are out of scope here (same rationale as [`super::evaluate`]).

use std::path::PathBuf;

use clap::Args;
use topos_core::core::morphism::ProgramMorphism;
use topos_core::evaluation::characteristic_morphism::CharacteristicMorphism;
use topos_core::evaluation::policies::simple::describe_entropy_ratio;
use topos_core::functors::probes::ast::entropy::calculate_kolmogorov_proxy;

use super::evaluate::classify_with_representations;
use super::lang::detect_language;

#[derive(Args)]
pub struct InspectArgs {
    /// The file to inspect.
    pub path: PathBuf,
}

pub fn run(args: InspectArgs) -> Result<(), String> {
    let language = detect_language(&args.path);
    let mut morphism = ProgramMorphism::from_file(&args.path, language)
        .map_err(|e| format!("reading {}: {e}", args.path.display()))?;
    let classifier = CharacteristicMorphism;
    let result = classify_with_representations(&classifier, &mut morphism);

    println!("File: {}", args.path.display());
    println!();

    println!("Classification");
    println!("{}", "-".repeat(40));
    if !result.is_parseable {
        println!("⊥ SLOP — parse failure");
        return Ok(());
    }
    for dim in ["simple", "composable", "secure"] {
        if let Some(val) = result.dimensions.get(dim) {
            println!("  {dim}: {val}");
        }
    }
    println!("  Valid syntax: {}", result.is_parseable);
    println!();

    println!("Raw Metrics");
    println!("{}", "-".repeat(40));
    let mut keys: Vec<&String> = result.raw_metrics.keys().collect();
    keys.sort();
    for key in keys {
        let value = result.raw_metrics[key];
        let interp = result
            .interpretation
            .get(key)
            .map(String::as_str)
            .unwrap_or("");
        let suffix = if interp.is_empty() {
            String::new()
        } else {
            format!("  ({interp})")
        };
        println!("  {key}: {value:.3}{suffix}");
    }

    println!();
    println!("Entropy Analysis");
    println!("{}", "-".repeat(40));
    let ratio = calculate_kolmogorov_proxy(&morphism.source);
    println!("  Compression ratio: {ratio:.3}");
    println!("  Interpretation: {}", describe_entropy_ratio(ratio));

    Ok(())
}
