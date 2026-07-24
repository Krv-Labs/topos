//! Security diagnostic overlay helpers for MCP tools.

use std::path::Path;

use topos_engine::config::{load_topos_config, merge_cli_allows, ToposConfig};
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::suppression::{apply_allowlist, AdjustedVerdict};

use crate::schemas::{AcknowledgedRisk, SecurityFinding};
use crate::security_findings::security_findings;

/// Allowlist-aware security diagnostics for one evaluation.
///
/// Always carries the true active findings — routing (agent contracts,
/// suggestions, refactor targets) must never be blinded by an output-size
/// preference. Payload gating (`include_security_findings`) is applied
/// where results are shaped, e.g. `to_evaluation_result`.
pub struct SecurityOverlay {
    pub active_findings: Vec<SecurityFinding>,
    pub acknowledged_risks: Vec<AcknowledgedRisk>,
    pub verdict: AdjustedVerdict,
}

fn secure_failed(result: &ClassificationResult) -> bool {
    result
        .raw_metrics
        .get("cpg.dangerous_calls")
        .copied()
        .unwrap_or(0.0)
        > 0.0
        || result
            .raw_metrics
            .get("cpg.taint_flows")
            .copied()
            .unwrap_or(0.0)
            > 0.0
}

fn config_for(path: Option<&Path>, allows: &[String]) -> ToposConfig {
    let config = match path {
        Some(p) => load_topos_config(p),
        None => ToposConfig::default(),
    };
    let allow_refs: Vec<&str> = allows.iter().map(String::as_str).collect();
    merge_cli_allows(config, &allow_refs)
}

fn acknowledged_to_models(verdict: &AdjustedVerdict) -> Vec<AcknowledgedRisk> {
    verdict
        .acknowledged
        .iter()
        .map(|(finding, entry)| AcknowledgedRisk {
            callee: finding.callee.clone(),
            kind: finding.kind.clone(),
            line: finding.line,
            snippet: finding.snippet.clone(),
            reason: entry.reason.clone(),
            scope: entry.scope.clone(),
        })
        .collect()
}

fn overlay(
    morphism: &mut ProgramMorphism,
    result: &ClassificationResult,
    file_path: Option<&Path>,
    allows: &[String],
) -> Option<SecurityOverlay> {
    if !result.is_parseable {
        return None;
    }
    let config = config_for(file_path, allows);
    if !secure_failed(result) {
        return None;
    }

    let cpg = morphism.build_cpg().cloned();
    // Pass the *raw* findings (full registry — `allow: None`) so that
    // `apply_allowlist` performs the acknowledged/active partition itself
    // against the merged config, which already folds in the one-off `allows`
    // via `config_for`. Filtering the findings here would strip one-off
    // `--allow` callees *before* the partition, leaving `acknowledged` empty:
    // that silently drops the mandatory risk disclosure and lets an
    // acknowledged risk buy an uncapped IDEAL grade (the grade cap in
    // `apply_allowlist` only fires when `acknowledged` is non-empty). Matches
    // the Python original's argument-less `security_findings(cpg)`.
    let findings = security_findings(cpg.as_ref(), 20, None, file_path);
    let core_findings: Vec<_> = findings.iter().map(|f| f.to_core()).collect();
    let verdict = apply_allowlist(result, &core_findings, &config, file_path, cpg.as_ref());
    let active_findings = verdict
        .active_findings
        .iter()
        .map(SecurityFinding::from_core)
        .collect();
    let acknowledged_risks = acknowledged_to_models(&verdict);
    Some(SecurityOverlay {
        active_findings,
        acknowledged_risks,
        verdict,
    })
}

/// Apply the project/one-off allowlist over a file classification.
pub fn overlay_for_file(
    path: &Path,
    result: &ClassificationResult,
    allows: &[String],
) -> Option<SecurityOverlay> {
    let language = crate::evaluation::detect_language(path);
    let mut morphism = ProgramMorphism::from_file(path, language).ok()?;
    overlay(&mut morphism, result, Some(path), allows)
}

/// Apply the project/one-off allowlist over an in-memory classification.
pub fn overlay_for_source(
    source: &str,
    language: &str,
    result: &ClassificationResult,
    file_path: Option<&Path>,
    allows: &[String],
) -> Option<SecurityOverlay> {
    let mut morphism = ProgramMorphism::new(source, language);
    overlay(&mut morphism, result, file_path, allows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::classify_code_string;
    use topos_engine::evaluation::policies::base::Priority;

    // `eval(...)` is a dangerous call, so SECURE fails and the overlay engages.
    const EVAL_SRC: &str = "def f(expr):\n    return eval(expr)\n";

    #[test]
    fn one_off_allow_acknowledges_risk_rather_than_stripping_it() {
        let result = classify_code_string(EVAL_SRC, "python", Priority::Simple)
            .expect("classification runs");

        // No allow: the eval finding is active and nothing is acknowledged.
        let bare = overlay_for_source(EVAL_SRC, "python", &result, None, &[])
            .expect("a secure-failing file produces an overlay");
        assert!(
            bare.acknowledged_risks.is_empty(),
            "nothing is acknowledged without an allow"
        );
        assert!(
            !bare.active_findings.is_empty(),
            "the eval finding is active"
        );

        // Regression guard: a one-off `--allow eval` must move the finding into
        // `acknowledged` (so the disclosure is emitted and the grade cap can
        // fire), not silently strip it before the partition.
        let allow = vec!["eval".to_string()];
        let allowed = overlay_for_source(EVAL_SRC, "python", &result, None, &allow)
            .expect("a secure-failing file produces an overlay");
        assert_eq!(
            allowed.acknowledged_risks.len(),
            1,
            "one-off --allow must acknowledge the eval risk, not strip it"
        );
        assert_eq!(
            allowed.acknowledged_risks[0].callee.as_deref(),
            Some("eval")
        );
        assert!(
            allowed
                .active_findings
                .iter()
                .all(|f| f.callee.as_deref() != Some("eval")),
            "the acknowledged eval finding is no longer active"
        );
    }
}
