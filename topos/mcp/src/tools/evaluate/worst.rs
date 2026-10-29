//! Why a file ranks where it does, and what binds it: the `worst_files`
//! entry.
//!
//! A file ranks by its failures and its lowest-scoring pillar, which the
//! entry names with that pillar's score. What binds the file is not that
//! pillar's concern alone: it is the `binding_constraint` that
//! `topos_evaluate_file` reports for the file, computed here by the same
//! code (`file_targets`), so the two never name different metrics.

use std::path::Path;

use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::policies::gates::{evaluate_gates, GateResult};

use super::{file_targets, ScoredProjectRow};
use crate::schemas::{EvaluateProjectInput, WorstFileEntry};

/// What the binding constraint of a listed file is computed from.
pub(super) struct BindingInputs<'a> {
    pub params: &'a EvaluateProjectInput,
    pub priority: Priority,
    pub gitnexus_dir: Option<&'a Path>,
}

/// The gates `row` fails, from the evaluation that produced its verdict: a
/// SECURE gate that the allowlist overlay cleared (`secure_adjusted`) is not
/// a failure.
pub(super) fn failing_gates(row: &ScoredProjectRow) -> Vec<GateResult> {
    let secure_cleared = row.entry.secure_adjusted == Some(true);
    evaluate_gates(
        &row.result.raw_metrics,
        None,
        row.result.is_entrypoint_module,
    )
    .into_iter()
    .filter(|gate| !gate.passed())
    .filter(|gate| !(secure_cleared && gate.spec.pillar == "secure"))
    .collect()
}

pub(super) fn worst_entry(row: &ScoredProjectRow, inputs: &BindingInputs<'_>) -> WorstFileEntry {
    let entry = &row.entry;
    let mut worst = WorstFileEntry::plain(entry.filepath.clone(), entry.lattice_element);
    worst.gates_failed = failing_gates(row).len();
    worst.parse_failure = !entry.is_parseable;
    let weakest = entry.scores.iter().min_by(|a, b| a.1.total_cmp(b.1));
    if let Some((pillar, score)) = weakest {
        worst.pillar = Some(pillar.clone());
        worst.score = Some(*score);
    }
    worst.binding_constraint = Some(file_targets::project_binding_constraint(
        Path::new(&entry.filepath),
        inputs.priority,
        inputs.gitnexus_dir,
        inputs.params,
    ));
    worst
}
