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
//! with no registered gate (an advisory metric such as the whole-file
//! `cfg.cyclomatic` location) cannot cost its pillar's `achieved`, so it
//! is labeled `"improve"` and sorted behind every real gate failure,
//! however large its excess. Agents route off the first target, so an
//! advisory metric leading the list is a wrong turn.
//!
//! Ordering follows the gate-score ascent: a pillar the active goal still
//! requires comes first, then `"fix"` before `"improve"`, then the pillar
//! whose gate score is closest to passing, then the lowest desirability
//! inside that pillar. Preference rank only breaks a tie. See [`rank_key`].

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use sha1::{Digest, Sha1};
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::omega::{EvaluationValue, Generator};
use topos_engine::evaluation::policies::gates::{evaluate_gates, metric_desirability};
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
        "navigable" => 1,
        "secure" => 2,
        "composable" => 3,
        _ => 99,
    }
}

/// Rank concrete edit targets without rerunning classification.
pub fn build_refactor_targets(
    filepath: &str,
    result: &ClassificationResult,
    security_findings: &[SecurityFinding],
    locations: &BTreeMap<String, Vec<FunctionEntry>>,
    ranking: Option<&[GeneratorInput]>,
    goal: EvaluationValue,
    max_targets: usize,
) -> Vec<RefactorTarget> {
    let mut candidates: Vec<RefactorTarget> = Vec::new();
    for (metric, entries) in locations {
        for entry in entries {
            candidates.push(location_target(filepath, metric, entry));
        }
    }
    candidates.extend(structural_metric_targets(filepath, result));
    candidates.extend(security_targets(filepath, security_findings));

    let pillar_rank: HashMap<&str, usize> = match ranking {
        Some(ranking) if !ranking.is_empty() => ranking
            .iter()
            .enumerate()
            .map(|(i, g)| (g.as_str(), i))
            .collect(),
        _ => HashMap::new(),
    };
    let pillar_gate = pillar_gate_scores(result, &candidates);
    candidates.sort_by(|a, b| {
        rank_key(a, result.lattice_element, goal, &pillar_rank, &pillar_gate).cmp(&rank_key(
            b,
            result.lattice_element,
            goal,
            &pillar_rank,
            &pillar_gate,
        ))
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
/// `achieved`, `"improve"` when it is advisory.
///
/// Every registered `GATE_SPECS` entry gates its pillar; advisory metrics
/// are not registered at all. The one advisory metric that still reaches
/// this module is the whole-file `cfg.cyclomatic` location (issue #193): a
/// merged-CFG sum that scales with function count, so it is surfaced but
/// cannot fail SIMPLE — `ast.max_function_complexity` gates that concern
/// directly. Labeling it `"fix"` sends agents to rewrite a metric no
/// verdict depends on.
///
/// This is the single gate → severity mapping in this module;
/// [`rank_key`] derives its gating tier from the severity string so the
/// label an agent reads and the order it is served in cannot diverge.
fn gate_severity(metric: &str) -> &'static str {
    let gating = topos_engine::evaluation::policies::gates::GATE_SPECS
        .iter()
        .any(|spec| spec.metric == metric);
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
        evidence: BTreeMap::from([
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

/// Targets for failing whole-file or module-context structural gates.
fn structural_metric_targets(filepath: &str, result: &ClassificationResult) -> Vec<RefactorTarget> {
    // Same gate inputs and entrypoint exemption the scorers used, so a
    // target can never contradict the score this module claims to be
    // derived from.
    evaluate_gates(&result.raw_metrics, None, result.is_entrypoint_module)
        .into_iter()
        .filter(|r| {
            !r.passed()
                && matches!(r.spec.granularity, "file" | "module")
                && r.spec.pillar != "secure"
        })
        .map(|r| {
            let scope = if r.spec.granularity == "file" {
                "<file>"
            } else {
                "<module>"
            };
            RefactorTarget {
                target_id: target_id(filepath, r.spec.metric, Some(scope), Some(1)),
                kind: r.spec.granularity.to_string(),
                filepath: filepath.to_string(),
                symbol: Some(scope.to_string()),
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
                evidence: BTreeMap::from([(
                    "interpretation".to_string(),
                    result
                        .interpretation
                        .get(r.spec.metric)
                        .cloned()
                        .map(Value::from)
                        .unwrap_or(Value::Null),
                )]),
            }
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
                evidence: BTreeMap::from([
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

/// Sort key: in the active goal, `"fix"` before `"improve"`, closest
/// pillar gate score first, then lowest desirability, then preference rank.
///
/// Gate score is comparable across metrics because every gate is anchored
/// at 0.5. Preference rank only breaks an equal score. An advisory metric
/// cannot lead a gate failure inside the same goal: the tier sits ahead of
/// the score.
fn rank_key(
    target: &RefactorTarget,
    current: EvaluationValue,
    goal: EvaluationValue,
    pillar_rank: &HashMap<&str, usize>,
    pillar_gate: &HashMap<String, f64>,
) -> (usize, usize, i64, i64, usize, usize, String) {
    let pillar = target
        .failing_generators
        .first()
        .map(String::as_str)
        .unwrap_or("simple");
    let outside = usize::from(!pillar_missing(goal, current, pillar));
    let tier = usize::from(target.severity != "fix");
    let desirability = target
        .current_value
        .and_then(|value| metric_desirability(&target.metric, value))
        .unwrap_or(0.0);
    let gate = pillar_gate.get(pillar).copied().unwrap_or(desirability);
    let rank = pillar_rank
        .get(pillar)
        .copied()
        .unwrap_or_else(|| default_pillar_rank(pillar));
    (
        outside,
        tier,
        -scale(gate),
        scale(if tier == 0 { desirability } else { 0.0 }),
        rank,
        target.line_start.unwrap_or(0),
        target.target_id.clone(),
    )
}

fn scale(value: f64) -> i64 {
    (value * 10_000.0).round() as i64
}

fn pillar_missing(goal: EvaluationValue, current: EvaluationValue, pillar: &str) -> bool {
    let Some(generator) = Generator::ALL
        .into_iter()
        .find(|generator| generator.as_str() == pillar)
    else {
        return false;
    };
    let bit = generator.value().bits();
    goal.bits() & bit != 0 && current.bits() & bit == 0
}

/// Shared gate score per pillar: the stored `gate_scores` entry, or the
/// minimum desirability among this pillar's `"fix"` targets when the
/// result has no score yet.
fn pillar_gate_scores(
    result: &ClassificationResult,
    targets: &[RefactorTarget],
) -> HashMap<String, f64> {
    let mut scores: HashMap<String, f64> = result.gate_scores.clone().into_iter().collect();
    for target in targets.iter().filter(|target| target.severity == "fix") {
        let Some(pillar) = target.failing_generators.first() else {
            continue;
        };
        if scores.contains_key(pillar) {
            continue;
        }
        let Some(desirability) = target
            .current_value
            .and_then(|value| metric_desirability(&target.metric, value))
        else {
            continue;
        };
        scores
            .entry(pillar.clone())
            .and_modify(|score| *score = score.min(desirability))
            .or_insert(desirability);
    }
    scores
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
        let locations = BTreeMap::from([
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
            EvaluationValue::Ideal,
            5,
        );

        assert_eq!(targets.len(), 2, "both SIMPLE signals stay on the list");
        assert_eq!(
            targets[0].metric, "ast.max_function_complexity",
            "ast.max_function_complexity (14 vs 10, excess 4) gates SIMPLE, so it must \
             rank ahead of cfg.cyclomatic (80, no gate), which is advisory \
             (issue #193) and cannot fail the pillar. The agent \
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
        let locations = BTreeMap::from([("cfg.cyclomatic".to_string(), vec![module_entry(80)])]);
        let targets = build_refactor_targets(
            "a.py",
            &ClassificationResult::default(),
            &[],
            &locations,
            None,
            EvaluationValue::Ideal,
            5,
        );

        assert_eq!(targets.len(), 1, "an advisory metric still yields a target");
        assert_eq!(targets[0].metric, "cfg.cyclomatic");
        assert_eq!(targets[0].kind, "module");
        assert_eq!(targets[0].severity, "improve");
        assert_eq!(targets[0].failing_generators, vec!["simple"]);
    }

    /// Advisory metrics are not gates: fan-in and instability, however
    /// atypical, never become refactor targets here (they surface as
    /// advisory suggestions instead).
    #[test]
    fn advisory_metrics_yield_no_gate_targets() {
        let mut result = ClassificationResult::default();
        result.raw_metrics.extend([
            ("mdg.fan_in".to_string(), 30.0),
            ("mdg.instability".to_string(), 0.95),
            ("mdg.abstractness".to_string(), 0.0),
            ("mdg.coupling".to_string(), 6.0),
            ("mdg.fan_out".to_string(), 5.0),
        ]);
        let targets = build_refactor_targets(
            "a.py",
            &result,
            &[],
            &BTreeMap::new(),
            None,
            EvaluationValue::Ideal,
            5,
        );
        assert!(
            targets.is_empty(),
            "got {:?}",
            targets.iter().map(|t| &t.metric).collect::<Vec<_>>()
        );
    }

    /// The live-server shape observed on `topos/mcp/src/formatting.rs`: a
    /// huge advisory `cfg.cyclomatic` in SIMPLE next to a small, genuine
    /// COMPOSABLE gate failure. `default_pillar_rank` puts SIMPLE first,
    /// so this is the fixture that separates "the caller ranked pillars"
    /// from "we fell back to an internal default".
    fn cross_pillar_fixture() -> (BTreeMap<String, Vec<FunctionEntry>>, ClassificationResult) {
        let locations = BTreeMap::from([("cfg.cyclomatic".to_string(), vec![module_entry(118)])]);
        let mut result = ClassificationResult::default();
        // `mdg.fan_out` is the gating COMPOSABLE metric (an absolute count,
        // so it has no resolution limit and still fails hard). At 11 against
        // a cap of 10 its excess is 1 -- a hundredth of cyclomatic's 103 --
        // which is the point: the gating tier must win on tier, not size.
        result.raw_metrics.insert("mdg.fan_out".to_string(), 11.0);
        (locations, result)
    }

    /// With no `preferences.ranking`, `pillar_rank` is an internal default,
    /// not a caller choice — so the gating tier must lead.
    #[test]
    fn gating_tier_leads_when_no_pillar_preference_is_supplied() {
        let (locations, result) = cross_pillar_fixture();
        let targets = build_refactor_targets(
            "a.py",
            &result,
            &[],
            &locations,
            None,
            EvaluationValue::Ideal,
            5,
        );

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

    /// A stated ranking no longer puts an advisory ahead of another pillar's
    /// failed gate. Rank only breaks an equal gate score.
    #[test]
    fn explicit_pillar_preference_does_not_outrank_a_gate() {
        let (locations, result) = cross_pillar_fixture();
        let targets = build_refactor_targets(
            "a.py",
            &result,
            &[],
            &locations,
            Some(&[
                GeneratorInput::Simple,
                GeneratorInput::Navigable,
                GeneratorInput::Secure,
                GeneratorInput::Composable,
            ]),
            EvaluationValue::Ideal,
            5,
        );

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].metric, "mdg.fan_out");
        assert_eq!(targets[0].severity, "fix");
        assert_eq!(targets[1].metric, "cfg.cyclomatic");
        assert_eq!(targets[1].severity, "improve");
    }

    #[test]
    fn security_targets_rank_by_pillar_preference() {
        let findings = vec![SecurityFinding {
            kind: "dangerous_call".to_string(),
            line: 5,
            snippet: "os.system(cmd)".to_string(),
            callee: Some("os.system".to_string()),
            ..Default::default()
        }];
        let result = ClassificationResult::default();
        let targets = build_refactor_targets(
            "a.py",
            &result,
            &findings,
            &BTreeMap::new(),
            Some(&[
                GeneratorInput::Secure,
                GeneratorInput::Simple,
                GeneratorInput::Navigable,
                GeneratorInput::Composable,
            ]),
            EvaluationValue::Ideal,
            5,
        );
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].kind, "security_call");
        assert!(targets[0].target_id.starts_with("rt_"));
        assert_eq!(targets[0].failing_generators, vec!["secure"]);
    }
}
