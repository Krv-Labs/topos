//! Security diagnostic overlay helpers for MCP tools.

use std::path::Path;

use topos_engine::config::{load_topos_config, merge_cli_allows, ToposConfig};
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::suppression::{apply_allowlist, AdjustedVerdict};

use crate::schemas::{AcknowledgedRisk, SecurityAdvisory, SecurityFinding};
use crate::security_findings::{all_security_findings, SecurityReport, MAX_SECURITY_FINDINGS};
use crate::sighthound::sighthound_all_findings;

/// Allowlist-aware security diagnostics for one evaluation.
///
/// Always carries the true active findings — routing (agent contracts,
/// suggestions, refactor targets) must never be blinded by an output-size
/// preference. Payload gating (`include_security_findings`) is applied
/// where results are shaped, e.g. `to_evaluation_result`.
pub struct SecurityOverlay {
    /// Top [`MAX_SECURITY_FINDINGS`] active findings, most urgent first.
    pub active_findings: Vec<SecurityFinding>,
    /// Sighthound summary over every active finding; reporting only.
    pub advisory: Option<SecurityAdvisory>,
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

pub(crate) fn overlay(
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
    //
    // The full (uncapped) list goes in, and the display cap is taken only
    // afterwards. Two reasons: the advisory counts every active finding, and
    // the acknowledged/active partition must see every finding. When the cap
    // ran first, a file whose only allowlisted finding sat past position 20
    // came back with `acknowledged` empty, so the grade cap never fired and
    // an allowlist-bought IDEAL went uncapped. `acknowledged_risks` is
    // therefore uncapped; only `active_findings` is cut to 20.
    let (findings, scanned) = match cpg.as_ref() {
        Some(cpg) => all_security_findings(cpg, None, file_path),
        None => (Vec::new(), false),
    };
    let core_findings: Vec<_> = findings.iter().map(|f| f.to_core()).collect();
    let verdict = apply_allowlist(result, &core_findings, &config, file_path, cpg.as_ref());
    let report = SecurityReport::from_full(
        with_metadata(findings, &verdict),
        scanned,
        MAX_SECURITY_FINDINGS,
    );
    let acknowledged_risks = acknowledged_to_models(&verdict);
    Some(SecurityOverlay {
        active_findings: report.findings,
        advisory: report.advisory,
        acknowledged_risks,
        verdict,
    })
}

/// The wire findings that `apply_allowlist` kept active, with their scanner
/// metadata intact (the engine's lean mirror drops it). The partition is an
/// order-preserving filter, so a single forward walk matches them up.
fn with_metadata(
    findings: Vec<SecurityFinding>,
    verdict: &AdjustedVerdict,
) -> Vec<SecurityFinding> {
    let mut active = verdict.active_findings.iter().peekable();
    findings
        .into_iter()
        .filter(|f| active.next_if(|a| **a == f.to_core()).is_some())
        .collect()
}

/// Opt-in coverage pass (`security_scan`): the Sighthound report for a file
/// the overlay skipped because the CPG SECURE gate passed.
///
/// Reporting only. The verdict from `apply_allowlist` is discarded; it is
/// used solely to drop acknowledged findings. `None` when the scanner does
/// not apply (unparseable source, unsupported language, disabled, or a scan
/// error).
pub fn opt_in_security_report(
    source: &str,
    language: &str,
    result: &ClassificationResult,
    file_path: Option<&Path>,
    allows: &[String],
) -> Option<SecurityReport> {
    if !result.is_parseable {
        return None;
    }
    let findings = sighthound_all_findings(source, language, None, file_path)?;
    let config = config_for(file_path, allows);
    let core_findings: Vec<_> = findings.iter().map(|f| f.to_core()).collect();
    let verdict = apply_allowlist(result, &core_findings, &config, file_path, None);
    Some(SecurityReport::from_full(
        with_metadata(findings, &verdict),
        true,
        MAX_SECURITY_FINDINGS,
    ))
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

    /// The overlay carries the advisory when Sighthound ran; the opt-in pass
    /// reports on a SECURE-passing file, and neither applies to Rust.
    #[test]
    fn advisory_rides_the_overlay_and_the_opt_in_pass() {
        let result = classify_code_string(EVAL_SRC, "python", Priority::Simple)
            .expect("classification runs");
        let overlay = overlay_for_source(EVAL_SRC, "python", &result, None, &[])
            .expect("a secure-failing file produces an overlay");
        let advisory = overlay.advisory.expect("python is scanned");
        assert_ne!(advisory.max_severity, "none");
        assert!(overlay.active_findings.iter().all(|f| f.severity.is_some()));

        let clean = "def f(x):\n    return x + 1\n";
        let clean_result =
            classify_code_string(clean, "python", Priority::Simple).expect("classification runs");
        let report = opt_in_security_report(clean, "python", &clean_result, None, &[])
            .expect("the opt-in pass scans python");
        assert_eq!(report.advisory.expect("scanner ran").max_severity, "none");

        let rust = "fn f() {}\n";
        let rust_result =
            classify_code_string(rust, "rust", Priority::Simple).expect("classification runs");
        assert!(opt_in_security_report(rust, "rust", &rust_result, None, &[]).is_none());
    }

    /// Regression: the acknowledged/active partition runs over every finding,
    /// not the 20 shown. An allowlisted risk past the display cap must still
    /// be acknowledged, or the grade cap cannot fire for it.
    #[test]
    fn allowlisted_finding_past_the_display_cap_is_still_acknowledged() {
        let mut src = String::from("import os\n\ndef f(cmd, expr):\n");
        for _ in 0..25 {
            src.push_str("    os.system(cmd)\n");
        }
        src.push_str("    return eval(expr)\n");
        let result =
            classify_code_string(&src, "python", Priority::Simple).expect("classification runs");
        let allow = vec!["eval".to_string()];
        let overlay = overlay_for_source(&src, "python", &result, None, &allow)
            .expect("a secure-failing file produces an overlay");
        assert!(
            overlay.active_findings.len() <= MAX_SECURITY_FINDINGS,
            "the displayed list stays capped"
        );
        assert!(
            overlay
                .acknowledged_risks
                .iter()
                .any(|r| r.callee.as_deref() == Some("eval")),
            "the allowlisted eval past position 20 is acknowledged"
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
