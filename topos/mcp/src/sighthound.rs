//! Embedded [Sighthound](https://github.com/Corgea/Sighthound) adapter.
//!
//! The Python original (`topos/utils/sighthound.py`) shelled out to a
//! `sighthound` CLI discovered on `$PATH`. Per PR #159, the engine is now a
//! library dependency compiled into this crate: no subprocess, no JSON
//! round-trip, no PATH probing — `run_explicit_scan` and
//! `run_taint_analysis_with_verbosity` are called in-process with embedded
//! rules, and their `Finding`s are mapped straight into
//! [`crate::schemas::SecurityFinding`].
//!
//! Sighthound's rule packs cover python / javascript / typescript / go (of
//! Topos's languages); rust and cpp fall back to the local CPG probes in
//! [`crate::security_findings`]. Set `TOPOS_DISABLE_SIGHTHOUND=1` to force
//! the CPG-probe path everywhere.
//!
//! Finding classification mirrors the Python adapter exactly: taint-mode
//! findings are tagged `taint_analysis` / `data_flow` / `cross_file`
//! (search findings carry rule tags only), with a literal `finding_type`
//! fallback for legacy payloads missing tags.
//!
//! Callee/sink resolution and allowlist matching ([`finding_callee`],
//! [`finding_sink_text`], [`finding_matches_allowlist`]) port the
//! consistency fix the pre-migration Python adapter shipped in issue
//! #168/#174: a taint finding's actionable callee is its matched sink
//! operation (`sink_info.sink_type`), not the containing function
//! (`function`/`sink_info.function_name`), and allowlist matching resolves
//! that callee fresh per finding rather than through a pre-filtered
//! registry substitution.

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use sighthound::{run_explicit_scan, run_taint_analysis_with_verbosity, Cli, Finding};
use topos_engine::functors::probes::cpg::danger::match_registry_key;
use topos_engine::graphs::cpg::object::CodePropertyGraph;

use crate::schemas::SecurityFinding;

/// Tags Sighthound itself uses to count search vs taint findings.
const TAINT_TAGS: [&str; 3] = ["taint_analysis", "data_flow", "cross_file"];
/// Rare fallbacks when tags are missing (legacy / partial payloads).
const TAINT_FINDING_TYPES: [&str; 2] = ["taint", "taint flow"];

/// Topos language → Sighthound language, or `None` when Sighthound has no
/// rule pack for it.
fn sighthound_language(language: &str) -> Option<&'static str> {
    match language {
        "python" => Some("python"),
        "javascript" => Some("javascript"),
        "typescript" => Some("typescript"),
        "go" => Some("go"),
        _ => None,
    }
}

fn temp_suffix(language: &str) -> &'static str {
    match language {
        "javascript" => ".js",
        "typescript" => ".ts",
        "go" => ".go",
        _ => ".py",
    }
}

fn scan_cli(language: &str) -> Cli {
    Cli {
        root_dir: None,
        language: Some(language.to_string()),
        rules_path: None,
        rules_dir: None,
        use_embedded_rules: true,
        use_file_rules: false,
        output_format: "json".to_string(),
        verbose: false,
        summary_only: false,
        single_threaded: true,
        threads: None,
        taint_analysis: false,
        simple_analysis: false,
        skip_minified: None,
        include_test_fixtures: true,
        code_type: None,
        language_filter: None,
        version: false,
        fail_on_severity: None,
        error_on_findings: false,
    }
}

/// Run Sighthound (search + taint passes) on `target_path` in-process.
fn run_scan(target_path: &Path, language: &str) -> Option<Vec<Finding>> {
    let cli = scan_cli(language);
    let root = target_path.to_string_lossy();
    let mut findings = run_explicit_scan(&cli, &root, false).ok()?;
    if let Ok(taint) = run_taint_analysis_with_verbosity(&cli, &root, false, false) {
        findings.extend(taint);
    }
    Some(findings)
}

/// Run Sighthound on a real file, or an in-memory source via a temp copy.
fn run_sighthound_scan(
    source: &str,
    language: &str,
    file_path: Option<&Path>,
) -> Option<Vec<Finding>> {
    let sh_language = sighthound_language(language)?;
    if let Some(path) = file_path {
        if path.exists() {
            return run_scan(path, sh_language);
        }
    }
    let mut tmp = tempfile::Builder::new()
        .prefix("topos-sighthound-")
        .suffix(temp_suffix(language))
        .tempfile()
        .ok()?;
    tmp.write_all(source.as_bytes()).ok()?;
    tmp.flush().ok()?;
    run_scan(tmp.path(), sh_language)
}

fn finding_tags(finding: &Finding) -> Vec<String> {
    finding
        .tags
        .iter()
        .flatten()
        .map(|t| t.to_lowercase())
        .collect()
}

/// True when Sighthound produced this finding via taint analysis.
fn is_taint_finding(finding: &Finding) -> bool {
    let tags = finding_tags(finding);
    if !tags.is_empty() {
        return tags.iter().any(|t| TAINT_TAGS.contains(&t.as_str()));
    }
    let ftype = finding.finding_type.trim().to_lowercase();
    TAINT_FINDING_TYPES.contains(&ftype.as_str())
}

fn clean(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// Best-effort callee / sink function name for allowlisting and display.
///
/// For taint findings, `sink_info.sink_type` is checked first: Sighthound's
/// `function`/`sink_info.function_name` fields carry the *containing*
/// function for a taint flow, not the actionable sink operation, so
/// preferring them here would allowlist/report the wrong callee (mirrors
/// the fix in the pre-migration Python adapter, issue #168/#174).
fn finding_callee(finding: &Finding) -> Option<String> {
    if is_taint_finding(finding) {
        if let Some(sink_type) = finding
            .sink_info
            .as_ref()
            .and_then(|sink| clean(&sink.sink_type))
        {
            return Some(sink_type.to_string());
        }
    }
    if let Some(func) = clean(&finding.function) {
        return Some(func.to_string());
    }
    finding
        .sink_info
        .as_ref()
        .and_then(|sink| clean(&sink.function_name))
        .map(str::to_string)
}

/// Human-readable taint source text from `source_info` (not `snippet`).
fn finding_source_text(finding: &Finding) -> Option<String> {
    let info = finding.source_info.as_ref()?;
    let source_type = clean(&info.source_type);
    let location = clean(&info.location);
    let context = clean(&info.context);
    let head = source_type.or(location).or(context)?;
    let mut text = head.to_string();
    if let Some(location) = location {
        if location != head {
            text = format!("{text} @ {location}");
        }
    }
    if let Some(context) = context {
        if context != head {
            text = format!("{text} ({context})");
        }
    }
    Some(text)
}

/// Human-readable sink text from `sink_info` or the finding snippet.
///
/// For taint findings, `sink_type` (the matched sink operation) wins over
/// `function_name` (the containing function) — same rationale and issue
/// reference as [`finding_callee`].
fn finding_sink_text(finding: &Finding) -> Option<String> {
    if let Some(sink) = &finding.sink_info {
        let sink_type = clean(&sink.sink_type);
        if is_taint_finding(finding) {
            if let Some(sink_type) = sink_type {
                return Some(sink_type.to_string());
            }
        }
        if let Some(name) = clean(&sink.function_name) {
            return Some(name.to_string());
        }
        if let Some(sink_type) = sink_type {
            return Some(sink_type.to_string());
        }
    }
    clean(&finding.snippet).map(str::to_string)
}

/// Whether a Sighthound finding's actionable callee is acknowledged by
/// `allow`.
///
/// Resolves the callee fresh per finding via [`finding_callee`] (which
/// already prefers a taint finding's `sink_type` over its containing
/// function) rather than pre-filtering a registry substitution — the same
/// per-finding resolution the pre-migration Python adapter switched to in
/// issue #168/#174, since a stale pre-filtered registry can't reflect the
/// corrected callee.
fn finding_matches_allowlist(finding: &Finding, allow: Option<&HashSet<String>>) -> bool {
    let Some(allow) = allow else {
        return false;
    };
    if allow.is_empty() {
        return false;
    }
    let Some(callee) = finding_callee(finding) else {
        return false;
    };
    match_registry_key(&callee, allow.iter().map(String::as_str)).is_some()
}

/// Convert one Sighthound finding; `None` when allowlisted away.
fn map_finding(finding: &Finding, allow: Option<&HashSet<String>>) -> Option<SecurityFinding> {
    if finding_matches_allowlist(finding, allow) {
        return None;
    }
    let callee = finding_callee(finding);

    let taint = is_taint_finding(finding);
    Some(SecurityFinding {
        kind: if taint {
            "taint_flow"
        } else {
            "dangerous_call"
        }
        .to_string(),
        line: finding.line.max(1) as u32,
        snippet: finding.snippet.clone(),
        callee,
        source: taint.then(|| finding_source_text(finding)).flatten(),
        sink: taint.then(|| finding_sink_text(finding)).flatten(),
    })
}

/// Map Sighthound findings into wire models for one CPG's source.
///
/// Returns `None` when the embedded engine does not apply (unsupported
/// language, disabled via env, or a scan error) so the caller falls back to
/// the local CPG probes.
pub fn sighthound_security_findings(
    cpg: &CodePropertyGraph,
    max_findings: usize,
    allow: Option<&HashSet<String>>,
    file_path: Option<&Path>,
) -> Option<Vec<SecurityFinding>> {
    if std::env::var("TOPOS_DISABLE_SIGHTHOUND").is_ok_and(|v| !v.is_empty() && v != "0") {
        return None;
    }
    let raw_findings = run_sighthound_scan(&cpg.source, &cpg.language, file_path)?;

    let mut findings = Vec::new();
    for raw in &raw_findings {
        if let Some(mapped) = map_finding(raw, allow) {
            findings.push(mapped);
            if findings.len() >= max_findings {
                break;
            }
        }
    }
    Some(findings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use topos_engine::core::morphism::ProgramMorphism;

    #[test]
    fn unsupported_language_returns_none() {
        let mut morphism = ProgramMorphism::new("fn main() {}", "rust");
        let cpg = morphism.build_cpg().expect("CPG builds").clone();
        assert!(sighthound_security_findings(&cpg, 20, None, None).is_none());
    }

    #[test]
    fn embedded_engine_flags_dangerous_python() {
        let source = "import os\n\n\ndef f(cmd):\n    os.system(cmd)\n";
        let mut morphism = ProgramMorphism::new(source, "python");
        let cpg = morphism.build_cpg().expect("CPG builds").clone();
        let findings = sighthound_security_findings(&cpg, 20, None, None)
            .expect("python is supported by the embedded engine");
        // The embedded rules flag os.system command execution.
        assert!(
            findings.iter().any(
                |f| f.snippet.contains("os.system") || f.callee.as_deref() == Some("os.system")
            ),
            "expected an os.system finding, got: {findings:?}"
        );
    }

    fn taint_finding(sink_type: &str, function_name: &str, containing_function: &str) -> Finding {
        Finding {
            file: "f.py".to_string(),
            line: 1,
            column: 0,
            end_line: 1,
            end_column: 0,
            function: containing_function.to_string(),
            finding_type: "Taint Flow".to_string(),
            snippet: "sink(tainted)".to_string(),
            severity: "high".to_string(),
            confidence: "high".to_string(),
            description: None,
            cwe_id: None,
            source_info: None,
            sink_info: Some(sighthound::models::SinkInfo {
                sink_type: sink_type.to_string(),
                function_name: function_name.to_string(),
                location: "f.py:1".to_string(),
                variable: None,
            }),
            traces: None,
            tags: Some(vec!["taint_analysis".to_string()]),
        }
    }

    #[test]
    fn taint_finding_callee_prefers_sink_type_over_containing_function() {
        // sink_type is the actionable sink operation; `function` /
        // `sink_info.function_name` are the containing function, which the
        // pre-#174-fix code preferred by mistake.
        let finding = taint_finding("os.system", "system", "handle_request");
        assert_eq!(finding_callee(&finding), Some("os.system".to_string()));
        assert_eq!(finding_sink_text(&finding), Some("os.system".to_string()));
    }

    #[test]
    fn taint_finding_falls_back_when_sink_type_is_empty() {
        let finding = taint_finding("", "system", "handle_request");
        assert_eq!(finding_callee(&finding), Some("handle_request".to_string()));
        assert_eq!(finding_sink_text(&finding), Some("system".to_string()));
    }

    #[test]
    fn allowlist_matches_against_the_corrected_taint_callee() {
        let finding = taint_finding("os.system", "system", "handle_request");
        let allow: HashSet<String> = ["os.system".to_string()].into_iter().collect();
        assert!(finding_matches_allowlist(&finding, Some(&allow)));
        assert!(map_finding(&finding, Some(&allow)).is_none());

        // An allowlist entry that only matches the (wrong) containing
        // function must NOT suppress the finding — the actionable callee
        // is still `os.system`.
        let wrong_allow: HashSet<String> = ["handle_request".to_string()].into_iter().collect();
        assert!(!finding_matches_allowlist(&finding, Some(&wrong_allow)));
        assert!(map_finding(&finding, Some(&wrong_allow)).is_some());
    }

    #[test]
    fn no_allowlist_matches_nothing() {
        let finding = taint_finding("os.system", "system", "handle_request");
        assert!(!finding_matches_allowlist(&finding, None));
        let empty: HashSet<String> = HashSet::new();
        assert!(!finding_matches_allowlist(&finding, Some(&empty)));
    }
}
