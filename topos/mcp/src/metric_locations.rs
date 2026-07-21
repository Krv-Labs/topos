//! Map failing SIMPLE complexity gates to concrete source locations.
//!
//! `topos_evaluate_file` can report a failing `ast.max_function_complexity`
//! without telling the agent *where* to edit. This module derives the
//! offending function spans from the same AST probe that produces the gate
//! metric, so the location and the metric never disagree.

use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::policies::calibration::SIMPLE;
use topos_engine::functors::probes::ast::complexity::{
    calculate_function_complexity_entries, FunctionComplexityEntry,
};

use crate::schemas::FunctionEntry;
use std::collections::HashMap;

/// Lift the probe struct into the MCP wire model.
pub fn function_entry_from_complexity(
    fc: &FunctionComplexityEntry,
    metric_source: &str,
) -> FunctionEntry {
    FunctionEntry {
        name: fc.name.clone(),
        line: fc.start_line,
        complexity: fc.complexity as i64,
        qualified_name: Some(fc.qualified_name.clone()),
        kind: Some(fc.kind.to_string()),
        start_line: Some(fc.start_line),
        end_line: Some(fc.end_line),
        metric_source: Some(metric_source.to_string()),
        includes_nested: None,
    }
}

/// Explicit 'not attributable to a function' marker for module-level gates.
fn module_marker(metric_source: &str, complexity: i64) -> FunctionEntry {
    FunctionEntry {
        name: "<module>".to_string(),
        line: 1,
        complexity,
        qualified_name: Some("<module>".to_string()),
        kind: Some("module".to_string()),
        start_line: Some(1),
        end_line: None,
        metric_source: Some(metric_source.to_string()),
        includes_nested: Some(true),
    }
}

/// Source locations for each failing SIMPLE complexity gate.
///
/// - `ast.max_function_complexity` resolves to the offending functions
///   (complexity above the per-function gate), sorted worst-first.
/// - `cfg.cyclomatic` is a whole-module count, so it gets a
///   `kind='module'` marker rather than a misleading function span.
pub fn build_metric_locations(
    source: &str,
    language: &str,
    result: &ClassificationResult,
) -> HashMap<String, Vec<FunctionEntry>> {
    let mut locations = HashMap::new();

    if let Some(&max_func) = result.raw_metrics.get("ast.max_function_complexity") {
        if max_func > SIMPLE.max_function_complexity {
            let offending = offending_functions(source, language);
            if !offending.is_empty() {
                locations.insert("ast.max_function_complexity".to_string(), offending);
            }
        }
    }

    if let Some(&cyclomatic) = result.raw_metrics.get("cfg.cyclomatic") {
        if cyclomatic > SIMPLE.max_cyclomatic {
            locations.insert(
                "cfg.cyclomatic".to_string(),
                vec![module_marker("cfg", cyclomatic as i64)],
            );
        }
    }

    locations
}

fn offending_functions(source: &str, language: &str) -> Vec<FunctionEntry> {
    let morphism = ProgramMorphism::new(source, language);
    let Some(ast) = morphism.ast.as_ref() else {
        return Vec::new();
    };
    if !morphism.is_valid() {
        return Vec::new();
    }
    let mut entries: Vec<FunctionComplexityEntry> =
        calculate_function_complexity_entries(&ast.uast_root, source)
            .into_iter()
            .filter(|e| e.complexity as f64 > SIMPLE.max_function_complexity)
            .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.complexity));
    entries
        .iter()
        .map(|e| function_entry_from_complexity(e, "ast"))
        .collect()
}
