//! Why a file ranks where it does: the `worst_files` entry.
//!
//! The pillar is the failing gate's pillar with the lowest score, else the
//! lowest-scoring pillar. The metric is the engine's binding failure on that
//! pillar (`binding_failure`, the lowest desirability), the same metric
//! `topos_evaluate_file` names for a single file. The gates are evaluated
//! again here, for the handful of rows that are listed, not for every row.

use std::collections::BTreeMap;

use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::evaluation::policies::gates::{binding_failure, evaluate_gates, GateResult};

use super::ScoredProjectRow;
use crate::schemas::WorstFileEntry;
use crate::shaping;

pub(super) fn worst_entry(row: &ScoredProjectRow) -> WorstFileEntry {
    let entry = &row.entry;
    let gates = failing_gates(&row.result);
    let mut worst = WorstFileEntry::plain(entry.filepath.clone(), entry.lattice_element);
    worst.gates_failed = gates.len();
    worst.parse_failure = !entry.is_parseable;
    match binding_gate(&gates, &entry.scores) {
        Some(gate) => name_gate(&mut worst, gate, &entry.scores),
        None => name_weakest_pillar(&mut worst, &entry.scores),
    }
    worst
}

fn failing_gates(result: &ClassificationResult) -> Vec<GateResult> {
    evaluate_gates(&result.raw_metrics, None, result.is_entrypoint_module)
        .into_iter()
        .filter(|gate| !gate.passed())
        .collect()
}

/// The binding failure of the lowest-scoring pillar that fails a gate.
fn binding_gate<'a>(
    failing: &'a [GateResult],
    scores: &BTreeMap<String, f64>,
) -> Option<&'a GateResult> {
    let score = |gate: &GateResult| {
        scores
            .get(gate.spec.pillar)
            .copied()
            .unwrap_or(f64::INFINITY)
    };
    let lowest = failing
        .iter()
        .min_by(|a, b| score(a).total_cmp(&score(b)))?;
    binding_failure(failing, lowest.spec.pillar)
}

fn name_gate(worst: &mut WorstFileEntry, gate: &GateResult, scores: &BTreeMap<String, f64>) {
    let limit = gate.threshold();
    worst.pillar = Some(gate.spec.pillar.to_string());
    worst.score = scores.get(gate.spec.pillar).copied();
    worst.metric = Some(gate.spec.metric.to_string());
    worst.value = Some(gate.value);
    worst.limit = limit;
    worst.bound = limit.and_then(|limit| shaping::bound(gate.value, limit));
}

fn name_weakest_pillar(worst: &mut WorstFileEntry, scores: &BTreeMap<String, f64>) {
    let weakest = scores.iter().min_by(|a, b| a.1.total_cmp(b.1));
    if let Some((pillar, score)) = weakest {
        worst.pillar = Some(pillar.clone());
        worst.score = Some(*score);
    }
}
