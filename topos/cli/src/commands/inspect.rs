//! `topos inspect` — detailed single-file metrics.
//!
//! Ported from `topos/cli/commands/quality.py::inspect`, scoped down
//! per issue #147: the security-findings / suggestions / suppression
//! sections are out of scope here (same rationale as [`super::evaluate`]).

use std::path::PathBuf;

use clap::Args;
use topos_engine::core::characteristic_morphism::CharacteristicMorphism;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::policies::simple::describe_entropy_ratio;
use topos_engine::functors::probes::ast::entropy::calculate_kolmogorov_proxy;

use super::classify::classify_with_representations;
use super::lang::detect_language;

#[derive(Args)]
pub struct InspectArgs {
    /// The file to inspect.
    pub path: PathBuf,
    /// Output the inspection as a single JSON object, matching the
    /// field names of the pure-Python `topos inspect --json` (a subset:
    /// `secure_raw`/`suggestions`/etc. depend on
    /// suggestions/suppression/security_guidance rendering, which this
    /// pass of issue #147 doesn't wire into the CLI — see this crate's
    /// module docs). Intended for machine comparison (e.g. Python/Rust
    /// parity tests), not primarily human reading.
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: InspectArgs) -> Result<(), String> {
    let language = detect_language(&args.path);
    let mut morphism = ProgramMorphism::from_file(&args.path, language)
        .map_err(|e| format!("reading {}: {e}", args.path.display()))?;
    let classifier = CharacteristicMorphism;
    let result = classify_with_representations(&classifier, &mut morphism, None);

    if args.json {
        return print_json(&args.path, &result);
    }

    println!("File: {}", args.path.display());
    println!();

    println!("Classification");
    println!("{}", "-".repeat(40));
    if !result.is_parseable {
        // Match Python's `print(...)` + `sys.exit(1)`: emit the SLOP line to
        // stdout, then exit non-zero — a parse failure is a CLI failure, so
        // `topos inspect broken.py` must fail a shell gate. (JSON mode above
        // returns 0 for an unparseable file, matching Python too.)
        println!("⊥ SLOP — parse failure");
        std::process::exit(1);
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
