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

/// Whether an overlay can exist at all — decidable from the classification
/// alone, without touching the source.
///
/// Both public entry points check this *before* building a `ProgramMorphism`,
/// so a SECURE-passing file never pays for a parse whose only consumer is the
/// `build_cpg` below. That matters most in the project loop
/// (`tools::evaluate::evaluate_single_file`), which calls `overlay_for_file`
/// once per file: on a clean codebase every one of those parses — and, via
/// `from_file`, a second read of a file `classify_file` already read — was
/// discarded unused. Keep the guard ahead of the parse.
fn overlay_applies(result: &ClassificationResult) -> bool {
    result.is_parseable && secure_failed(result)
}

fn overlay(
    morphism: &mut ProgramMorphism,
    result: &ClassificationResult,
    file_path: Option<&Path>,
    allows: &[String],
) -> Option<SecurityOverlay> {
    if !overlay_applies(result) {
        return None;
    }
    let config = config_for(file_path, allows);

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
    if !overlay_applies(result) {
        return None;
    }
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
    if !overlay_applies(result) {
        return None;
    }
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

    /// The guards now run ahead of the parse, so the passing path returns
    /// `None` having never built a morphism. Pins the half of the contract
    /// that hoist could have broken: no overlay for SECURE-clean source, and
    /// none for unparseable source either — the early `None` must still
    /// distinguish "nothing to report" from "could not look".
    #[test]
    fn secure_clean_and_unparseable_sources_produce_no_overlay() {
        let clean = "def f(x):\n    return x + 1\n";
        let result =
            classify_code_string(clean, "python", Priority::Simple).expect("classification runs");
        assert!(result.is_parseable);
        assert!(
            overlay_for_source(clean, "python", &result, None, &[]).is_none(),
            "a SECURE-passing file has no overlay to report"
        );

        // An allow list must not conjure an overlay where SECURE passed.
        assert!(
            overlay_for_source(clean, "python", &result, None, &["eval".to_string()]).is_none(),
            "an unused --allow does not manufacture an overlay"
        );

        let broken = "def f(:\n";
        let broken_result = classify_code_string(broken, "python", Priority::Simple)
            .expect("classification runs on unparseable source");
        assert!(!broken_result.is_parseable);
        assert!(
            overlay_for_source(broken, "python", &broken_result, None, &[]).is_none(),
            "unparseable source yields no overlay"
        );
    }

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
