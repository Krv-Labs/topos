//! Build ranked refactor targets from existing evaluation evidence.
//!
//! Targets are derived from the same canonical sources the evaluation
//! itself uses: gate decisions come from
//! `topos_core::evaluation::policies::gates` (so a target can never
//! contradict the score, including entrypoint exemptions) and security
//! operations from `topos_core::evaluation::security_guidance` (the same
//! suffix-matched table the suggestion engine renders as prose).

use std::collections::HashMap;

use serde_json::Value;
use sha1::{Digest, Sha1};
use topos_core::core::characteristic_morphism::ClassificationResult;
use topos_core::evaluation::policies::gates::evaluate_gates;
use topos_core::evaluation::security_guidance::remediation_for;

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
    candidates.sort_by(|a, b| {
        rank_key(a, &pillar_rank)
            .partial_cmp(&rank_key(b, &pillar_rank))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.truncate(max_targets);
    candidates
}

/// Threshold for a metric from the canonical gate table (upper bound).
fn gate_high(metric: &str) -> Option<f64> {
    topos_core::evaluation::policies::gates::GATE_SPECS
        .iter()
        .find(|spec| spec.metric == metric)
        .and_then(|spec| spec.high)
}

fn gate_pillar(metric: &str) -> &'static str {
    topos_core::evaluation::policies::gates::GATE_SPECS
        .iter()
        .find(|spec| spec.metric == metric)
        .map(|spec| spec.pillar)
        .unwrap_or("simple")
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
        severity: "fix".to_string(),
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
    evaluate_gates(
        &result.raw_metrics,
        None,
        result.is_entrypoint_module,
        false,
        None,
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
        severity: "fix".to_string(),
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

fn rank_key(
    target: &RefactorTarget,
    pillar_rank: &HashMap<&str, usize>,
) -> (usize, i64, usize, String) {
    let pillar = target
        .failing_generators
        .first()
        .map(String::as_str)
        .unwrap_or("simple");
    let rank = pillar_rank
        .get(pillar)
        .copied()
        .unwrap_or_else(|| default_pillar_rank(pillar));
    let current = target.current_value.unwrap_or(0.0);
    let threshold = target.threshold.unwrap_or(current);
    let excess = ((current - threshold).abs() * 100.0) as i64;
    (
        rank,
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
