//! Build ranked refactor targets from existing evaluation evidence.
//!
//! Targets are derived from the same canonical sources the evaluation
//! itself uses: gate decisions come from
//! `topos_engine::evaluation::policies::gates` (so a target can never
//! contradict the score, including entrypoint exemptions) and security
//! operations from `topos_engine::evaluation::security_guidance` (the same
//! suffix-matched table the suggestion engine renders as prose).
//!
//! Ranking honors the same distinction the gate table makes: a metric
//! whose failure cannot cost its pillar's `achieved` (`gates_achieved:
//! false`) is labeled `"improve"` and sorted behind every real gate
//! failure, however large its excess. Agents route off the first target,
//! so an advisory metric leading the list is a wrong turn.
//!
//! That ordering yields to one thing only: an explicit
//! `preferences.ranking`, which is a caller instruction rather than a
//! default. Absent one, the gating tier leads across pillars too — see
//! [`rank_key`].

use std::collections::HashMap;

use serde_json::Value;
use sha1::{Digest, Sha1};
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::evaluation::policies::composable::coupling_gate_input;
use topos_engine::evaluation::policies::gates::evaluate_gates;
use topos_engine::evaluation::security_guidance::remediation_for;

use crate::schemas::{FunctionEntry, GeneratorInput, RefactorTarget, SecurityFinding};

const LOCATION_CONSTRAINTS: [&str; 1] = ["preserve public behavior"];
const MODULE_METRIC_CONSTRAINTS: [&str; 1] =
    ["preserve module API unless the caller requested an API change"];
const SECURITY_CONSTRAINTS: [&str; 1] =
    ["do not allowlist unless the risk is intentional and documented"];

fn default_pillar_rank(pillar: &str) -> usize {
    match pillar {
        "simple" => 0,
        "secure" => 1,
        "composable" => 2,
        _ => 99,
    }
}

/// Rank concrete edit targets without rerunning classification.
pub fn build_refactor_targets(
    filepath: &str,
    result: &ClassificationResult,
    security_findings: &[SecurityFinding],
    locations: &HashMap<String, Vec<FunctionEntry>>,
    ranking: Option<&[GeneratorInput]>,
    max_targets: usize,
) -> Vec<RefactorTarget> {
    let mut candidates: Vec<RefactorTarget> = Vec::new();
    for (metric, entries) in locations {
        for entry in entries {
            candidates.push(location_target(filepath, metric, entry));
        }
    }
    candidates.extend(module_metric_targets(filepath, result));
    candidates.extend(security_targets(filepath, security_findings));

    let pillar_rank: HashMap<&str, usize> = match ranking {
        Some(ranking) => ranking
            .iter()
            .enumerate()
            .map(|(i, g)| (g.as_str(), i))
            .collect(),
        None => HashMap::new(),
    };
    // Read off the derived map rather than `ranking.is_some()`: a ranking
    // that ranked nothing leaves every target on `default_pillar_rank`,
    // which is the internal default the tier is meant to outrank, so it
    // must take the no-preference branch. `topos_evaluate_file` cannot
    // reach that shape today — `UserPreferencesInput::to_preferences`
    // rejects anything but a full permutation before targets are built —
    // but this function is public and the map, not the slice, is what the
    // key actually consults.
    let tier_first = pillar_rank.is_empty();
    candidates.sort_by(|a, b| {
        rank_key(a, &pillar_rank, tier_first)
            .partial_cmp(&rank_key(b, &pillar_rank, tier_first))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.truncate(max_targets);
    candidates
}

/// Threshold for a metric from the canonical gate table (upper bound).
fn gate_high(metric: &str) -> Option<f64> {
    topos_engine::evaluation::policies::gates::GATE_SPECS
        .iter()
        .find(|spec| spec.metric == metric)
        .and_then(|spec| spec.high)
}

fn gate_pillar(metric: &str) -> &'static str {
    topos_engine::evaluation::policies::gates::GATE_SPECS
        .iter()
        .find(|spec| spec.metric == metric)
        .map(|spec| spec.pillar)
        .unwrap_or("simple")
}

/// `"fix"` when failing this metric actually costs its pillar's
/// `achieved`, `"improve"` when the gate is advisory.
///
/// Three specs are advisory (`gates_achieved: false`). `cfg.cyclomatic`
/// (issue #193) is a whole-file merged-CFG sum that scales with function
/// count, so it is still scored and surfaced but cannot fail SIMPLE —
/// `ast.max_function_complexity` gates that concern directly.
/// `mdg.instability` and `mdg.main_sequence_distance` are ratios whose
/// resolution is `1 / (Ca + Ce)`, which at file granularity is too coarse
/// for the calibrated band to be a fair test. Labeling any of them `"fix"`
/// sends agents to rewrite a metric no verdict depends on. Metrics with no
/// registered spec default to gating, matching `gate_pillar`'s defensive
/// fallback.
///
/// This is the single `gates_achieved` → severity mapping in this module;
/// [`rank_key`] derives its gating tier from the severity string so the
/// label an agent reads and the order it is served in cannot diverge.
fn gate_severity(metric: &str) -> &'static str {
    let gating = topos_engine::evaluation::policies::gates::GATE_SPECS
        .iter()
        .find(|spec| spec.metric == metric)
        .map(|spec| spec.gates_achieved)
        .unwrap_or(true);
    if gating {
        "fix"
    } else {
        "improve"
    }
}

/// A target for one offending function span (or whole-module marker).
fn location_target(filepath: &str, metric: &str, entry: &FunctionEntry) -> RefactorTarget {
    let is_module = entry.kind.as_deref() == Some("module");
    let operations: Vec<String> = if is_module {
        vec!["split_module".into(), "extract_cohesive_unit".into()]
    } else {
        vec!["extract_helper".into(), "split_decision_logic".into()]
    };
    let symbol = entry
        .qualified_name
        .clone()
        .unwrap_or_else(|| entry.name.clone());
    RefactorTarget {
        target_id: target_id(filepath, metric, Some(&symbol), Some(entry.line)),
        kind: if is_module { "module" } else { "function" }.to_string(),
        filepath: filepath.to_string(),
        symbol: Some(symbol),
        line_start: entry.start_line.or(Some(entry.line)),
        line_end: entry.end_line,
        failing_generators: vec![gate_pillar(metric).to_string()],
        metric: metric.to_string(),
        current_value: Some(entry.complexity as f64),
        threshold: gate_high(metric),
        severity: gate_severity(metric).to_string(),
        recommended_operations: operations,
        constraints: LOCATION_CONSTRAINTS.iter().map(|s| s.to_string()).collect(),
        evidence: HashMap::from([
            ("complexity".to_string(), Value::from(entry.complexity)),
            (
                "metric_source".to_string(),
                entry
                    .metric_source
                    .clone()
                    .map(Value::from)
                    .unwrap_or(Value::Null),
            ),
            (
                "includes_nested".to_string(),
                entry
                    .includes_nested
                    .map(Value::from)
                    .unwrap_or(Value::Null),
            ),
        ]),
    }
}

/// Targets for failing module-granularity gates (entropy, coupling).
fn module_metric_targets(filepath: &str, result: &ClassificationResult) -> Vec<RefactorTarget> {
    // Reproduce the exact gate inputs the scorers used, or a target would
    // contradict the score this module claims to be derived from:
    // `Φ_COMPOSABLE` drops raw `mdg.instability` in favor of
    // `mdg.main_sequence_distance` whenever abstractness and a real
    // coupling signal are present, and `distance_stable_leaf_exempt` reads
    // the stable-leaf flag plus raw instability from the gate context.
    // Gating `result.raw_metrics` verbatim re-fired the superseded
    // instability gate and left the stable-leaf carve-out unreachable.
    // `coupling_gate_input` is the shared source of that swap (it also
    // computes `mdg.main_sequence_distance`, which is never in
    // `raw_metrics`); keep this block in sync with its sibling caller
    // `topos_engine::evaluation::suggestions::suggest_refactors`.
    let instability = result.raw_metrics.get("mdg.instability").copied();
    let mut gate_metrics = result.raw_metrics.clone();
    gate_metrics.remove("mdg.instability");
    gate_metrics.extend(coupling_gate_input(
        instability,
        result.raw_metrics.get("mdg.fan_in").copied(),
        result.raw_metrics.get("mdg.fan_out").copied(),
        result.raw_metrics.get("mdg.abstractness").copied(),
        result.raw_metrics.get("mdg.coupling").copied(),
    ));
    evaluate_gates(
        &gate_metrics,
        None,
        result.is_entrypoint_module,
        result.is_stable_leaf_module,
        instability,
    )
    .into_iter()
    .filter(|r| !r.passed() && r.spec.granularity == "module" && r.spec.pillar != "secure")
    .map(|r| RefactorTarget {
        target_id: target_id(filepath, r.spec.metric, Some("<module>"), Some(1)),
        kind: "module".to_string(),
        filepath: filepath.to_string(),
        symbol: Some("<module>".to_string()),
        line_start: Some(1),
        line_end: None,
        failing_generators: vec![r.spec.pillar.to_string()],
        metric: r.spec.metric.to_string(),
        current_value: Some(r.value),
        threshold: r.threshold(),
        severity: gate_severity(r.spec.metric).to_string(),
        recommended_operations: r.operations().iter().map(|s| s.to_string()).collect(),
        constraints: MODULE_METRIC_CONSTRAINTS
            .iter()
            .map(|s| s.to_string())
            .collect(),
        evidence: HashMap::from([(
            "interpretation".to_string(),
            result
                .interpretation
                .get(r.spec.metric)
                .cloned()
                .map(Value::from)
                .unwrap_or(Value::Null),
        )]),
    })
    .collect()
}

fn security_targets(filepath: &str, findings: &[SecurityFinding]) -> Vec<RefactorTarget> {
    findings
        .iter()
        .map(|finding| {
            let (_, operations) = remediation_for(&finding.to_core());
            let symbol_or_snippet = finding
                .callee
                .clone()
                .unwrap_or_else(|| finding.snippet.clone());
            RefactorTarget {
                target_id: target_id(
                    filepath,
                    &finding.kind,
                    Some(&symbol_or_snippet),
                    Some(finding.line as usize),
                ),
                kind: "security_call".to_string(),
                filepath: filepath.to_string(),
                symbol: finding.callee.clone(),
                line_start: Some(finding.line as usize),
                line_end: Some(finding.line as usize),
                failing_generators: vec!["secure".to_string()],
                metric: finding
                    .callee
                    .clone()
                    .unwrap_or_else(|| finding.kind.clone()),
                current_value: Some(1.0),
                threshold: Some(0.0),
                severity: "fix".to_string(),
                recommended_operations: operations.iter().map(|s| s.to_string()).collect(),
                constraints: SECURITY_CONSTRAINTS.iter().map(|s| s.to_string()).collect(),
                evidence: HashMap::from([
                    ("kind".to_string(), Value::from(finding.kind.clone())),
                    ("snippet".to_string(), Value::from(finding.snippet.clone())),
                    (
                        "source".to_string(),
                        finding
                            .source
                            .clone()
                            .map(Value::from)
                            .unwrap_or(Value::Null),
                    ),
                    (
                        "sink".to_string(),
                        finding.sink.clone().map(Value::from).unwrap_or(Value::Null),
                    ),
                ]),
            }
        })
        .collect()
}

/// Sort key over pillar, gating tier, excess, then position — with pillar
/// and tier swapped depending on whether the caller ranked pillars.
///
/// The gating tier always sits *ahead* of excess so an advisory metric can
/// never outrank a real gate failure by sheer magnitude. It used to: a
/// whole-file `cfg.cyclomatic` of 80 (excess 65, and `gates_achieved:
/// false`, so it cannot fail SIMPLE) buried a genuine
/// `ast.max_function_complexity` of 14 (excess 4), and since the agent
/// contract routes off `targets.first()` agents were sent to rewrite the
/// one SIMPLE metric no verdict depends on.
///
/// What `tier_first` decides is which of pillar and tier leads:
///
/// - `false` — the caller passed `preferences.ranking`, so the key is
///   `(pillar_rank, tier, ...)`. A stated pillar order is an instruction;
///   an agent that asked for SECURE first gets SECURE first even when a
///   different pillar has the failing gate.
/// - `true` — no ranking, so `pillar_rank` is only `default_pillar_rank`,
///   an internal tie-breaker nobody asked for. The key becomes
///   `(tier, pillar_rank, ...)` and correctness leads. Otherwise the
///   cross-pillar version of the same bug survives: on this repo's own
///   `formatting.rs`, an advisory `cfg.cyclomatic` of 118 outranked a
///   gating `mdg.instability` purely because SIMPLE sorts before
///   COMPOSABLE by default.
fn rank_key(
    target: &RefactorTarget,
    pillar_rank: &HashMap<&str, usize>,
    tier_first: bool,
) -> (usize, usize, i64, usize, String) {
    let pillar = target
        .failing_generators
        .first()
        .map(String::as_str)
        .unwrap_or("simple");
    let rank = pillar_rank
        .get(pillar)
        .copied()
        .unwrap_or_else(|| default_pillar_rank(pillar));
    // Derived from the severity string rather than a second GATE_SPECS
    // lookup, so the label and the ordering share one decision.
    let tier = if target.severity == "fix" { 0 } else { 1 };
    let current = target.current_value.unwrap_or(0.0);
    let threshold = target.threshold.unwrap_or(current);
    let excess = ((current - threshold).abs() * 100.0) as i64;
    let (primary, secondary) = if tier_first {
        (tier, rank)
    } else {
        (rank, tier)
    };
    (
        primary,
        secondary,
        -excess,
        target.line_start.unwrap_or(0),
        target.target_id.clone(),
    )
}

fn target_id(filepath: &str, metric: &str, symbol: Option<&str>, line: Option<usize>) -> String {
    let posix = filepath.replace('\\', "/");
    let raw = format!(
        "{posix}:{metric}:{}:{}",
        symbol.unwrap_or(""),
        line.map(|l| l.to_string()).unwrap_or_default()
    );
    let digest = Sha1::digest(raw.as_bytes());
    format!("rt_{}", &hex::encode(digest)[..12])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors `metric_locations::module_marker` (the `cfg.cyclomatic`
    /// whole-file row) without depending on a parseable source file.
    fn module_entry(complexity: i64) -> FunctionEntry {
        FunctionEntry {
            name: "<module>".to_string(),
            line: 1,
            complexity,
            qualified_name: Some("<module>".to_string()),
            kind: Some("module".to_string()),
            start_line: Some(1),
            end_line: None,
            metric_source: Some("cfg".to_string()),
            includes_nested: Some(true),
        }
    }

    fn function_entry(name: &str, line: usize, complexity: i64) -> FunctionEntry {
        FunctionEntry {
            name: name.to_string(),
            line,
            complexity,
            qualified_name: Some(name.to_string()),
            kind: Some("function".to_string()),
            start_line: Some(line),
            end_line: Some(line + 40),
            metric_source: Some("ast".to_string()),
            includes_nested: Some(false),
        }
    }

    /// The dogfooding regression: an advisory metric with a huge excess
    /// must not outrank a small, genuine gate failure in the same pillar.
    #[test]
    fn gating_failure_outranks_larger_advisory_excess() {
        let locations = HashMap::from([
            ("cfg.cyclomatic".to_string(), vec![module_entry(80)]),
            (
                "ast.max_function_complexity".to_string(),
                vec![function_entry("handle_request", 42, 14)],
            ),
        ]);
        let targets = build_refactor_targets(
            "a.py",
            &ClassificationResult::default(),
            &[],
            &locations,
            None,
            5,
        );

        assert_eq!(targets.len(), 2, "both SIMPLE signals stay on the list");
        assert_eq!(
            targets[0].metric, "ast.max_function_complexity",
            "ast.max_function_complexity (14 vs 10, excess 4) gates SIMPLE, so it must \
             rank ahead of cfg.cyclomatic (80 vs 15, excess 65), which is advisory \
             (gates_achieved: false, issue #193) and cannot fail the pillar. The agent \
             contract routes off targets.first(), so ordering here is the routing."
        );
        assert_eq!(targets[0].severity, "fix");
        // Advisory, but deliberately not dropped: the whole-file signal is
        // still worth reading once the real gate failure is handled.
        assert_eq!(targets[1].metric, "cfg.cyclomatic");
        assert_eq!(targets[1].severity, "improve");
    }

    #[test]
    fn cyclomatic_only_target_is_advisory() {
        let locations = HashMap::from([("cfg.cyclomatic".to_string(), vec![module_entry(80)])]);
        let targets = build_refactor_targets(
            "a.py",
            &ClassificationResult::default(),
            &[],
            &locations,
            None,
            5,
        );

        assert_eq!(targets.len(), 1, "an advisory metric still yields a target");
        assert_eq!(targets[0].metric, "cfg.cyclomatic");
        assert_eq!(targets[0].kind, "module");
        assert_eq!(targets[0].severity, "improve");
        assert_eq!(targets[0].failing_generators, vec!["simple"]);
    }

    /// Distance mode: `Φ_COMPOSABLE` gates `mdg.main_sequence_distance` in
    /// place of raw instability, so no target may fire on the instability
    /// the scorer superseded.
    #[test]
    fn composable_targets_use_the_scorer_gate_inputs() {
        let mut result = ClassificationResult::default();
        // A = 0.2, I = 0.9 => D = 0.1, inside main_sequence_distance_max,
        // even though I = 0.9 is above the raw instability_high of 0.7.
        // Abstractness must be nonzero or the scorer keeps gating raw
        // instability instead (see `coupling_gate_input`).
        result.raw_metrics.extend([
            ("mdg.instability".to_string(), 0.9),
            ("mdg.abstractness".to_string(), 0.2),
            ("mdg.coupling".to_string(), 6.0),
            ("mdg.fan_in".to_string(), 1.0),
            ("mdg.fan_out".to_string(), 5.0),
        ]);
        let targets = build_refactor_targets("a.py", &result, &[], &HashMap::new(), None, 5);

        assert!(
            targets.is_empty(),
            "gating raw_metrics verbatim re-fires mdg.instability, which \
             Φ_COMPOSABLE replaced with mdg.main_sequence_distance = 0.1 (passing); \
             got {:?}",
            targets.iter().map(|t| &t.metric).collect::<Vec<_>>()
        );
    }

    /// The other direction of the same swap: distance fails while raw
    /// instability sits in band, so the target must name the metric the
    /// scorer actually gated.
    #[test]
    fn composable_target_names_the_metric_the_scorer_gated() {
        let mut result = ClassificationResult::default();
        // A = 1.0, I = 0.7 => D = 0.7, past main_sequence_distance_max,
        // while I = 0.7 is exactly on the raw instability high bound (in
        // band). A fully abstract module is a real reading — abstractness
        // must be nonzero for the scorer to gate distance at all.
        result.raw_metrics.extend([
            ("mdg.instability".to_string(), 0.7),
            ("mdg.abstractness".to_string(), 1.0),
            ("mdg.coupling".to_string(), 6.0),
            ("mdg.fan_in".to_string(), 1.0),
            ("mdg.fan_out".to_string(), 5.0),
        ]);
        let targets = build_refactor_targets("a.py", &result, &[], &HashMap::new(), None, 5);

        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].metric, "mdg.main_sequence_distance",
            "mdg.main_sequence_distance is never in raw_metrics — it only exists \
             once coupling_gate_input derives it, so gating raw_metrics verbatim \
             could never surface this failure at all"
        );
        // Distance is advisory (its resolution is `I`'s), so it is still
        // named and still actionable, but it cannot fail COMPOSABLE alone.
        assert_eq!(targets[0].severity, "improve");
        assert_eq!(targets[0].failing_generators, vec!["composable"]);
    }

    /// The live-server shape observed on `topos/mcp/src/formatting.rs`: a
    /// huge advisory `cfg.cyclomatic` in SIMPLE next to a small, genuine
    /// COMPOSABLE gate failure. `default_pillar_rank` puts SIMPLE first,
    /// so this is the fixture that separates "the caller ranked pillars"
    /// from "we fell back to an internal default".
    fn cross_pillar_fixture() -> (HashMap<String, Vec<FunctionEntry>>, ClassificationResult) {
        let locations = HashMap::from([("cfg.cyclomatic".to_string(), vec![module_entry(118)])]);
        let mut result = ClassificationResult::default();
        // `mdg.fan_out` is the gating COMPOSABLE metric (an absolute count,
        // so it has no resolution limit and still fails hard). At 16 against
        // a cap of 15 its excess is 1 -- a hundredth of cyclomatic's 103 --
        // which is the point: the gating tier must win on tier, not size.
        result.raw_metrics.insert("mdg.fan_out".to_string(), 16.0);
        (locations, result)
    }

    /// With no `preferences.ranking`, `pillar_rank` is an internal default,
    /// not a caller choice — so the gating tier must lead.
    #[test]
    fn gating_tier_leads_when_no_pillar_preference_is_supplied() {
        let (locations, result) = cross_pillar_fixture();
        let targets = build_refactor_targets("a.py", &result, &[], &locations, None, 5);

        assert_eq!(
            targets.len(),
            2,
            "fixture must produce both an advisory SIMPLE target and a gating \
             COMPOSABLE one; got {:?}",
            targets.iter().map(|t| &t.metric).collect::<Vec<_>>()
        );
        assert_eq!(
            targets[0].metric, "mdg.fan_out",
            "no ranking was supplied, so SIMPLE-before-COMPOSABLE is only \
             default_pillar_rank talking. A gating COMPOSABLE failure must beat an \
             advisory SIMPLE metric with a far larger excess (103 vs 1), or the \
             agent routes off a metric no verdict depends on."
        );
        assert_eq!(targets[0].severity, "fix");
        assert_eq!(targets[1].metric, "cfg.cyclomatic");
        assert_eq!(targets[1].severity, "improve");
    }

    /// Same fixture, but the caller ranked SIMPLE first: a stated
    /// preference outranks the gating tier.
    #[test]
    fn explicit_pillar_preference_outranks_the_gating_tier() {
        let (locations, result) = cross_pillar_fixture();
        let targets = build_refactor_targets(
            "a.py",
            &result,
            &[],
            &locations,
            Some(&[
                GeneratorInput::Simple,
                GeneratorInput::Secure,
                GeneratorInput::Composable,
            ]),
            5,
        );

        assert_eq!(targets.len(), 2);
        assert_eq!(
            targets[0].metric, "cfg.cyclomatic",
            "the caller asked for SIMPLE first, so the advisory SIMPLE target leads \
             even though the COMPOSABLE one gates — preferences.ranking is an \
             explicit instruction, unlike default_pillar_rank"
        );
        assert_eq!(targets[1].metric, "mdg.fan_out");
    }

    #[test]
    fn security_targets_rank_by_pillar_preference() {
        let findings = vec![SecurityFinding {
            kind: "dangerous_call".to_string(),
            line: 5,
            snippet: "os.system(cmd)".to_string(),
            callee: Some("os.system".to_string()),
            source: None,
            sink: None,
        }];
        let result = ClassificationResult::default();
        let targets = build_refactor_targets(
            "a.py",
            &result,
            &findings,
            &HashMap::new(),
            Some(&[
                GeneratorInput::Secure,
                GeneratorInput::Simple,
                GeneratorInput::Composable,
            ]),
            5,
        );
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].kind, "security_call");
        assert!(targets[0].target_id.starts_with("rt_"));
        assert_eq!(targets[0].failing_generators, vec!["secure"]);
    }
}
