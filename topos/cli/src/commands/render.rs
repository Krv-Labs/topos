//! Human-readable rendering of a [`ClassificationResult`], shared by
//! `evaluate`'s per-file + rollup output.
//!
//! Split out of `evaluate.rs` -- printing is a separate concern from
//! `run`'s file-discovery/classification orchestration, and bundling
//! both pushed the file's cyclomatic total over the SIMPLE gate.

use topos_engine::core::characteristic_morphism::ClassificationResult;

/// Print a verdict, per-generator scores, and raw metrics for one result.
pub(crate) fn print_classification(result: &ClassificationResult) {
    if !result.is_parseable {
        println!("  {}", result.summary());
        return;
    }
    println!("  Verdict: {}", result.summary());
    for dim in ["simple", "composable", "secure"] {
        let Some(val) = result.dimensions.get(dim) else {
            continue;
        };
        let score = result.scores.get(dim).copied().unwrap_or(0.0) * 100.0;
        println!("    {dim}: {val} [{score:.0}%]");
    }
    if !result.raw_metrics.is_empty() {
        println!("  Raw metrics:");
        let mut keys: Vec<&String> = result.raw_metrics.keys().collect();
        keys.sort();
        for key in keys {
            let value = result.raw_metrics[key];
            println!("    {key}: {value:.3}");
        }
    }
}
