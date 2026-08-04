//! Map failing per-function gates to concrete source locations.
//!
//! `topos_evaluate_file` can report a failing
//! `ast.max_function_complexity` or `nav.max_function_divergence` without
//! telling the agent *where* to edit. This module derives the offending
//! function spans from the same AST probes that produce those gate
//! metrics, so the location and the metric never disagree.

use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::policies::calibration::{NAVIGABLE, SIMPLE};
use topos_engine::functors::probes::ast::complexity::{
    calculate_function_complexity_entries, FunctionComplexityEntry,
};
use topos_engine::functors::probes::ast::divergence::{
    calculate_function_divergence_entries, FunctionDivergenceEntry,
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

/// Source locations for each failing per-function gate.
///
/// - `ast.max_function_complexity` resolves to the offending functions
///   (complexity above the per-function gate), sorted worst-first.
/// - `cfg.cyclomatic` is a whole-module count, so it gets a
///   `kind='module'` marker rather than a misleading function span.
/// - `nav.max_function_divergence` resolves to the most deeply nested
///   functions, worst-first. Without this a NAVIGABLE failure would have
///   no location, never become a refactor target, and so never be
///   fixable.
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

    if let Some(&divergence) = result.raw_metrics.get("nav.max_function_divergence") {
        if divergence > NAVIGABLE.max_function_divergence {
            let offending = diverging_functions(source, language);
            if !offending.is_empty() {
                locations.insert("nav.max_function_divergence".to_string(), offending);
            }
        }
    }

    locations
}

/// Functions whose nesting divergence is over the NAVIGABLE gate,
/// worst-first.
///
/// `FunctionEntry.complexity` carries the divergence rounded to an integer
/// — the field is the wire's generic "how bad is this" number, and a
/// fractional log-weighted score has no better home in the existing shape.
/// The exact value stays available in `raw_metrics`.
fn diverging_functions(source: &str, language: &str) -> Vec<FunctionEntry> {
    let morphism = ProgramMorphism::new(source, language);
    let Some(ast) = morphism.ast.as_ref() else {
        return Vec::new();
    };
    if !morphism.is_valid() {
        return Vec::new();
    }
    let mut entries: Vec<FunctionDivergenceEntry> =
        calculate_function_divergence_entries(&ast.uast_root, source)
            .into_iter()
            .filter(|e| e.divergence > NAVIGABLE.max_function_divergence)
            .collect();
    entries.sort_by(|a, b| b.divergence.total_cmp(&a.divergence));
    entries
        .iter()
        .map(|e| FunctionEntry {
            name: e.name.clone(),
            line: e.start_line,
            complexity: e.divergence.round() as i64,
            qualified_name: Some(e.qualified_name.clone()),
            kind: Some(e.kind.to_string()),
            start_line: Some(e.start_line),
            end_line: Some(e.end_line),
            metric_source: Some("nav".to_string()),
            includes_nested: None,
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use topos_engine::core::characteristic_morphism::CharacteristicMorphism;
    use topos_engine::evaluation::policies::base::Priority;

    /// Nine levels of nesting, one branch per level — well past the
    /// NAVIGABLE gate while staying inside SIMPLE's.
    fn deeply_nested() -> String {
        let mut source = String::from("def deep(xs):\n");
        for depth in 0..9 {
            source.push_str(&"    ".repeat(depth + 1));
            source.push_str(&format!("if xs[{depth}]:\n"));
        }
        source.push_str(&"    ".repeat(10));
        source.push_str("return 1\n");
        source
    }

    fn locations_for(source: &str) -> HashMap<String, Vec<FunctionEntry>> {
        let morphism = ProgramMorphism::new(source, "python");
        let result = CharacteristicMorphism.classify_detailed(&morphism, &[], Priority::default());
        build_metric_locations(source, "python", &result)
    }

    /// The regression that matters: a failing NAVIGABLE gate must resolve
    /// to a real span, or the gate is un-targetable and so un-fixable.
    #[test]
    fn a_failing_divergence_gate_resolves_to_the_offending_function() {
        let source = deeply_nested();
        let entries = locations_for(&source)
            .remove("nav.max_function_divergence")
            .expect("a failing divergence gate must have a location");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "deep");
        assert_eq!(entries[0].start_line, Some(1));
        assert_eq!(entries[0].metric_source.as_deref(), Some("nav"));
    }

    #[test]
    fn flat_code_produces_no_divergence_location() {
        let locations = locations_for("def f(x):\n    return x\n");
        assert!(!locations.contains_key("nav.max_function_divergence"));
    }

    /// Worst-first ordering, so an agent fixing one target picks the one
    /// that actually moves the gate.
    #[test]
    fn diverging_functions_are_sorted_worst_first() {
        let mut source = deeply_nested();
        // A second, even deeper function.
        source.push_str("\n\ndef deeper(xs):\n");
        for depth in 0..12 {
            source.push_str(&"    ".repeat(depth + 1));
            source.push_str(&format!("if xs[{depth}]:\n"));
        }
        source.push_str(&"    ".repeat(13));
        source.push_str("return 1\n");

        let entries = locations_for(&source)
            .remove("nav.max_function_divergence")
            .expect("both functions fail the gate");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "deeper");
        assert!(entries[0].complexity >= entries[1].complexity);
    }
}
