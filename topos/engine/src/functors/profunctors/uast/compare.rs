//! UAST Comparison — cross-language structural comparison built on top
//! of UAST `kind` values.
//!
//! The AST distance in [`crate::functors::profunctors::ast::compare`]
//! is grammar-specific because it consumes raw tree-sitter node types.
//! This module is the language-agnostic counterpart: every distance
//! here operates on normalized UAST kinds, so two implementations of
//! the same algorithm in different languages can be compared on a
//! single axis.

use std::collections::{HashMap, HashSet};

use crate::functors::probes::uast::signature::{
    control_flow_profile, structural_summary, uast_dfs_kind_sequence, uast_kind_histogram,
    StructuralSummary, CONTROL_FLOW_KINDS,
};
use crate::functors::profunctors::ast::compare::{compute_sequence_distance, DistanceResult};
use crate::graphs::uast::models::UASTNode;

/// Aggregate cross-language comparison between two UAST roots.
#[derive(Debug, Clone, PartialEq)]
pub struct UASTComparison {
    pub kind_distance: f64,
    pub edit_distance: DistanceResult,
    pub control_flow_delta: HashMap<String, i64>,
    pub summary_delta: HashMap<String, i64>,
    pub source_summary: StructuralSummary,
    pub target_summary: StructuralSummary,
}

impl UASTComparison {
    /// True if any structural difference was found.
    pub fn detects_difference(&self) -> bool {
        self.kind_distance > 0.0 || self.edit_distance.raw_distance > 0
    }
}

/// L1 distance between normalized UAST kind histograms.
///
/// Both histograms are normalized to probability distributions so the
/// result lies in `[0, 1]` regardless of program size. A return value
/// of `0.0` means both programs use the same mix of UAST kinds; `1.0`
/// means they share no kinds at all.
pub fn uast_kind_distance(source: &UASTNode, target: &UASTNode, include_unknown: bool) -> f64 {
    let a = uast_kind_histogram(source, include_unknown);
    let b = uast_kind_histogram(target, include_unknown);

    let total_a = a.values().sum::<usize>().max(1) as f64;
    let total_b = b.values().sum::<usize>().max(1) as f64;

    let kinds: HashSet<&String> = a.keys().chain(b.keys()).collect();
    let l1: f64 = kinds
        .iter()
        .map(|k| {
            let av = *a.get(*k).unwrap_or(&0) as f64 / total_a;
            let bv = *b.get(*k).unwrap_or(&0) as f64 / total_b;
            (av - bv).abs()
        })
        .sum();
    // L1 distance between two probability distributions is bounded by 2;
    // halve to get a [0, 1] range matching the rest of the codebase.
    l1 / 2.0
}

/// Tree edit distance over UAST kind sequences (DFS pre-order).
///
/// Reuses the Wagner-Fischer implementation from
/// [`crate::functors::profunctors::ast::compare`] so the operation
/// accounting stays consistent with the tree-sitter variant.
pub fn uast_edit_distance(
    source: &UASTNode,
    target: &UASTNode,
    include_unknown: bool,
) -> DistanceResult {
    let source_kinds = uast_dfs_kind_sequence(source, include_unknown);
    let target_kinds = uast_dfs_kind_sequence(target, include_unknown);

    let (distance, operations) = compute_sequence_distance(&source_kinds, &target_kinds);
    let max_size = source_kinds.len().max(target_kinds.len()).max(1) as f64;
    let normalized = (distance as f64 / max_size).min(1.0);

    DistanceResult {
        raw_distance: distance,
        normalized_distance: normalized,
        operations,
    }
}

fn control_flow_delta(source: &UASTNode, target: &UASTNode) -> HashMap<String, i64> {
    let src_profile = control_flow_profile(source);
    let tgt_profile = control_flow_profile(target);
    CONTROL_FLOW_KINDS
        .iter()
        .map(|&kind| {
            let d = *tgt_profile.get(kind).unwrap_or(&0) as i64
                - *src_profile.get(kind).unwrap_or(&0) as i64;
            (kind.to_string(), d)
        })
        .collect()
}

fn summary_delta(source: &StructuralSummary, target: &StructuralSummary) -> HashMap<String, i64> {
    HashMap::from([
        (
            "node_count".to_string(),
            target.node_count as i64 - source.node_count as i64,
        ),
        (
            "depth".to_string(),
            target.depth as i64 - source.depth as i64,
        ),
        (
            "declaration_count".to_string(),
            target.declaration_count as i64 - source.declaration_count as i64,
        ),
        (
            "expression_count".to_string(),
            target.expression_count as i64 - source.expression_count as i64,
        ),
        (
            "statement_count".to_string(),
            target.statement_count as i64 - source.statement_count as i64,
        ),
    ])
}

/// Run the full UAST comparison suite for a single pair of roots.
pub fn compare_uast(source: &UASTNode, target: &UASTNode, include_unknown: bool) -> UASTComparison {
    let source_summary = structural_summary(source);
    let target_summary = structural_summary(target);
    UASTComparison {
        kind_distance: uast_kind_distance(source, target, include_unknown),
        edit_distance: uast_edit_distance(source, target, include_unknown),
        control_flow_delta: control_flow_delta(source, target),
        summary_delta: summary_delta(&source_summary, &target_summary),
        source_summary,
        target_summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;
    use crate::graphs::uast::models::{NativeRef, SourceSpan};

    fn uast(source: &str, language: &str) -> UASTNode {
        parse_source(source, language, None).unwrap().uast_root
    }

    #[test]
    fn identical_uast_has_zero_distance() {
        let root = uast("def add(a, b):\n    return a + b\n", "python");

        assert_eq!(uast_kind_distance(&root, &root, true), 0.0);
        let edit = uast_edit_distance(&root, &root, true);
        assert_eq!(edit.raw_distance, 0);
        assert_eq!(edit.normalized_distance, 0.0);

        let comparison = compare_uast(&root, &root, true);
        assert!(!comparison.detects_difference());
        assert!(comparison.control_flow_delta.values().all(|&v| v == 0));
        assert!(comparison.summary_delta.values().all(|&v| v == 0));
    }

    #[test]
    fn compare_uast_handles_empty_uast_node() {
        let span = SourceSpan {
            file: None,
            start_byte: 0,
            end_byte: 0,
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 0,
        };
        let native = NativeRef {
            parser: "test".to_string(),
            parser_version: "0".to_string(),
            node_kind: "module".to_string(),
        };
        let empty = UASTNode {
            kind: "File".to_string(),
            lang: "python".to_string(),
            span,
            native,
            attributes: HashMap::new(),
            children: Vec::new(),
            id: String::new(),
        };
        let other = UASTNode {
            kind: "Unknown".to_string(),
            ..empty.clone()
        };

        let comparison = compare_uast(&empty, &other, true);
        assert_eq!(comparison.kind_distance, 1.0);
        assert_eq!(comparison.edit_distance.raw_distance, 1);
    }
}
