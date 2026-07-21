//! Security diagnostic overlay helpers for MCP tools.

use std::collections::HashSet;
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
    let allow_set: HashSet<String> = allows.iter().cloned().collect();
    let findings = security_findings(
        cpg.as_ref(),
        20,
        (!allow_set.is_empty()).then_some(&allow_set),
        file_path,
    );
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
