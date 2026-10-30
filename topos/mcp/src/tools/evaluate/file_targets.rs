//! The refactor targets of one file, and the binding constraint they give.
//!
//! `topos_evaluate_file` ranks a file's targets and reports the top gating
//! one as `binding_constraint`. The worst files of `topos_evaluate_project`
//! report the same field, so both go through the functions here, from the
//! classification to the target ranking: the two cannot name different
//! metrics for the same file.

use std::collections::BTreeMap;
use std::path::Path;

use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::omega::EvaluationValue;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::preferences::UserPreferences;

use crate::diagnostics::{overlay_for_file, SecurityOverlay};
use crate::evaluation::{classify_file, detect_language};
use crate::formatting::binding_constraint;
use crate::metric_locations::build_metric_locations;
use crate::refactor_targets::build_refactor_targets;
use crate::schemas::{
    default_refactor_targets, BindingConstraint, EvaluateProjectInput, FunctionEntry,
    RefactorTarget, UserPreferencesInput,
};
use crate::security::read_resolved_utf8;

/// Where each failing function metric is, read from `source`; empty when
/// the source could not be read.
pub(super) fn locations(
    path: &Path,
    source: Option<&str>,
    result: &ClassificationResult,
) -> BTreeMap<String, Vec<FunctionEntry>> {
    match source {
        Some(source) => build_metric_locations(source, detect_language(path), result),
        None => BTreeMap::new(),
    }
}

/// The best `count` targets of the file (at most 25), ranked.
pub(super) fn targets(
    path: &Path,
    result: &ClassificationResult,
    overlay: Option<&SecurityOverlay>,
    locations: &BTreeMap<String, Vec<FunctionEntry>>,
    preferences: Option<&UserPreferencesInput>,
    resolved_preferences: Option<&UserPreferences>,
    count: usize,
) -> Vec<RefactorTarget> {
    build_refactor_targets(
        &path.to_string_lossy(),
        result,
        overlay.map_or(&[], |o| o.active_findings.as_slice()),
        locations,
        preferences.map(|p| p.ranking.as_slice()),
        resolved_preferences.map_or(EvaluationValue::Ideal, UserPreferences::aspirational_target),
        count.min(25),
    )
}

/// What `topos_evaluate_file` reports as `binding_constraint` for `path` with
/// the same defaults: the project's allowlist and preferences applied, and
/// the default number of targets.
///
/// `scanned` is the classification the project scan already made of a file
/// that no security overlay adjusted: its targets read only the raw metrics,
/// the lattice element and the gate scores, which the scan leaves as
/// `topos_evaluate_file` would. For a file the overlay adjusted (`None`),
/// the unadjusted classification and the overlay are made again. Only the
/// source is always read again, for the function locations.
pub(super) fn project_binding_constraint(
    path: &Path,
    scanned: Option<&ClassificationResult>,
    priority: Priority,
    gitnexus_dir: Option<&Path>,
    params: &EvaluateProjectInput,
) -> Option<BindingConstraint> {
    let fresh;
    let (result, overlay) = match scanned {
        Some(result) => (result, None),
        None => {
            fresh = classify_file(path, priority, gitnexus_dir).ok()?.0;
            (&fresh, overlay_for_file(path, &fresh, &params.allow))
        }
    };
    let source = read_resolved_utf8(path).ok();
    let locations = locations(path, source.as_deref(), result);
    let resolved_preferences = params
        .preferences
        .as_ref()
        .and_then(|p| p.to_preferences().ok());
    let targets = targets(
        path,
        result,
        overlay.as_ref(),
        &locations,
        params.preferences.as_ref(),
        resolved_preferences.as_ref(),
        default_refactor_targets(),
    );
    binding_constraint(&targets)
}
