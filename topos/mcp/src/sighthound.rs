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

use crate::schemas::{mode_for_kind, SecurityFinding};

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

/// Closed severity set, most severe first; the index is the sort rank.
pub(crate) const SEVERITIES: [&str; 4] = ["critical", "high", "medium", "low"];
/// Closed confidence set, most confident first.
pub(crate) const CONFIDENCES: [&str; 3] = ["high", "medium", "low"];

/// Fold a free-form scanner level into `allowed`; unknown values become
/// `"low"` (the last, least alarming entry of both sets).
fn normalize_level(raw: &str, allowed: &[&'static str]) -> &'static str {
    let raw = raw.trim().to_lowercase();
    allowed
        .iter()
        .copied()
        .find(|level| *level == raw)
        .unwrap_or("low")
}

/// `cwe-89` / `CWE-89` / `89` → `CWE-89`; `None` when there is no number.
fn normalize_cwe(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let digits = raw
        .get(..4)
        .filter(|p| p.eq_ignore_ascii_case("cwe-"))
        .map_or(raw, |_| &raw[4..]);
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| format!("CWE-{digits}"))
}

/// `Finding.cwe_id`, else the first `cwe-*` tag.
fn finding_cwe(finding: &Finding) -> Option<String> {
    finding
        .cwe_id
        .as_deref()
        .and_then(normalize_cwe)
        .or_else(|| finding_tags(finding).iter().find_map(|t| normalize_cwe(t)))
}

/// Advisory family: CWE first, then keywords from the rule category / tags
/// (Sighthound folds the category into `tags`) and the title.
pub(crate) fn family(cwe: Option<&str>, keywords: &[String]) -> &'static str {
    let by_cwe = cwe
        .and_then(|c| c.strip_prefix("CWE-"))
        .and_then(|n| n.parse::<u32>().ok())
        .and_then(|n| match n {
            77 | 78 | 89 | 90 | 91 | 94 | 95 | 917 | 943 => Some("injection"),
            79 | 80 => Some("xss"),
            502 => Some("deserialization"),
            22 | 23 | 36 | 73 | 601 => Some("path"),
            295 | 326 | 327 | 328 | 330 | 338 | 916 => Some("crypto"),
            259 | 287 | 306 | 321 | 798 | 862 | 863 => Some("auth"),
            _ => None,
        });
    if let Some(family) = by_cwe {
        return family;
    }
    // Order matters: injection before path so `xpath-injection` is injection.
    const KEYWORDS: [(&str, &[&str]); 6] = [
        ("xss", &["xss", "cross-site"]),
        ("deserialization", &["deserial", "pickle"]),
        ("injection", &["injection", "sqli", "command", "exec"]),
        ("path", &["path", "traversal", "redirect"]),
        ("crypto", &["crypto", "hash", "cipher", "random"]),
        ("auth", &["auth", "secret", "credential", "password"]),
    ];
    let keywords: Vec<String> = keywords.iter().map(|k| k.to_lowercase()).collect();
    KEYWORDS
        .iter()
        .find(|(_, needles)| {
            keywords
                .iter()
                .any(|k| needles.iter().any(|needle| k.contains(needle)))
        })
        .map_or("other", |(family, _)| family)
}

/// Convert one Sighthound finding; `None` when allowlisted away.
fn map_finding(finding: &Finding, allow: Option<&HashSet<String>>) -> Option<SecurityFinding> {
    if finding_matches_allowlist(finding, allow) {
        return None;
    }
    let callee = finding_callee(finding);

    let taint = is_taint_finding(finding);
    let kind = if taint {
        "taint_flow"
    } else {
        "dangerous_call"
    };
    let cwe = finding_cwe(finding);
    let title = clean(&finding.finding_type).map(str::to_string);
    let mut keywords = finding_tags(finding);
    keywords.extend(title.clone());
    Some(SecurityFinding {
        kind: kind.to_string(),
        line: finding.line.max(1) as u32,
        snippet: finding.snippet.clone(),
        callee,
        source: taint.then(|| finding_source_text(finding)).flatten(),
        sink: taint.then(|| finding_sink_text(finding)).flatten(),
        mode: mode_for_kind(kind),
        severity: Some(normalize_level(&finding.severity, &SEVERITIES).to_string()),
        confidence: Some(normalize_level(&finding.confidence, &CONFIDENCES).to_string()),
        family: Some(family(cwe.as_deref(), &keywords)),
        cwe,
        title,
    })
}

fn rank(value: Option<&str>, order: &[&str]) -> usize {
    value
        .and_then(|v| order.iter().position(|o| *o == v))
        .unwrap_or(order.len())
}

/// Most urgent first: severity, then confidence, taint before pattern, line.
pub(crate) fn sort_findings(findings: &mut [SecurityFinding]) {
    findings.sort_by_key(|f| {
        (
            rank(f.severity.as_deref(), &SEVERITIES),
            rank(f.confidence.as_deref(), &CONFIDENCES),
            f.mode.as_deref() != Some("taint"),
            f.line,
        )
    });
}

/// Map raw findings, drop allowlisted ones, and sort (uncapped).
fn map_and_sort(raw_findings: &[Finding], allow: Option<&HashSet<String>>) -> Vec<SecurityFinding> {
    let mut findings: Vec<SecurityFinding> = raw_findings
        .iter()
        .filter_map(|raw| map_finding(raw, allow))
        .collect();
    sort_findings(&mut findings);
    findings
}

/// Every non-allowlisted Sighthound finding for `source`, sorted most
/// urgent first and uncapped (callers take the display cap and compute the
/// advisory over the full list).
///
/// Returns `None` when the embedded engine does not apply (unsupported
/// language, disabled via env, or a scan error) so the caller falls back to
/// the local CPG probes.
pub fn sighthound_all_findings(
    source: &str,
    language: &str,
    allow: Option<&HashSet<String>>,
    file_path: Option<&Path>,
) -> Option<Vec<SecurityFinding>> {
    if std::env::var("TOPOS_DISABLE_SIGHTHOUND").is_ok_and(|v| !v.is_empty() && v != "0") {
        return None;
    }
    let raw_findings = run_sighthound_scan(source, language, file_path)?;
    Some(map_and_sort(&raw_findings, allow))
}

/// [`sighthound_all_findings`] for one CPG's source, capped at
/// `max_findings` after sorting.
pub fn sighthound_security_findings(
    cpg: &CodePropertyGraph,
    max_findings: usize,
    allow: Option<&HashSet<String>>,
    file_path: Option<&Path>,
) -> Option<Vec<SecurityFinding>> {
    let mut findings = sighthound_all_findings(&cpg.source, &cpg.language, allow, file_path)?;
    findings.truncate(max_findings);
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

    fn pattern_finding(line: usize, severity: &str, confidence: &str) -> Finding {
        Finding {
            line,
            finding_type: "Command Execution".to_string(),
            snippet: format!("os.system(x{line})"),
            function: "os.system".to_string(),
            severity: severity.to_string(),
            confidence: confidence.to_string(),
            sink_info: None,
            tags: None,
            ..taint_finding("", "", "")
        }
    }

    #[test]
    fn severity_and_confidence_fold_into_closed_sets() {
        assert_eq!(normalize_level("Critical", &SEVERITIES), "critical");
        assert_eq!(normalize_level(" HIGH ", &SEVERITIES), "high");
        assert_eq!(normalize_level("Medium", &CONFIDENCES), "medium");
        assert_eq!(normalize_level("info", &SEVERITIES), "low");
        assert_eq!(normalize_level("", &SEVERITIES), "low");
        assert_eq!(normalize_level("critical", &CONFIDENCES), "low");

        let mapped = map_finding(&pattern_finding(3, "Bogus", "Certain"), None).unwrap();
        assert_eq!(mapped.severity.as_deref(), Some("low"));
        assert_eq!(mapped.confidence.as_deref(), Some("low"));
        assert_eq!(mapped.mode.as_deref(), Some("pattern"));
        assert_eq!(mapped.title.as_deref(), Some("Command Execution"));
    }

    #[test]
    fn cwe_normalizes_from_id_then_tag() {
        assert_eq!(normalize_cwe("cwe-89").as_deref(), Some("CWE-89"));
        assert_eq!(normalize_cwe("CWE-78").as_deref(), Some("CWE-78"));
        assert_eq!(normalize_cwe("502").as_deref(), Some("CWE-502"));
        assert_eq!(normalize_cwe("cwe-"), None);
        assert_eq!(normalize_cwe("injection"), None);

        let mut from_id = taint_finding("os.system", "system", "f");
        from_id.cwe_id = Some("cwe-78".to_string());
        assert_eq!(finding_cwe(&from_id).as_deref(), Some("CWE-78"));

        let mut from_tag = taint_finding("os.system", "system", "f");
        from_tag.tags = Some(vec!["taint_analysis".into(), "CWE-22".into()]);
        assert_eq!(finding_cwe(&from_tag).as_deref(), Some("CWE-22"));
        let mapped = map_finding(&from_tag, None).unwrap();
        assert_eq!(mapped.cwe.as_deref(), Some("CWE-22"));
        assert_eq!(mapped.family, Some("path"));
        assert_eq!(mapped.mode.as_deref(), Some("taint"));
    }

    #[test]
    fn family_maps_cwe_first_then_keywords() {
        assert_eq!(family(Some("CWE-78"), &[]), "injection");
        assert_eq!(family(Some("CWE-79"), &[]), "xss");
        assert_eq!(family(Some("CWE-502"), &[]), "deserialization");
        assert_eq!(family(Some("CWE-22"), &[]), "path");
        assert_eq!(family(Some("CWE-601"), &[]), "path");
        assert_eq!(family(Some("CWE-798"), &[]), "auth");
        assert_eq!(family(Some("CWE-327"), &[]), "crypto");
        assert_eq!(family(Some("CWE-918"), &[]), "other");
        assert_eq!(family(None, &[]), "other");
        // CWE wins over a contradicting keyword.
        assert_eq!(family(Some("CWE-79"), &["sqli".into()]), "xss");
        assert_eq!(family(None, &["path-traversal".into()]), "path");
        assert_eq!(family(None, &["xpath-injection".into()]), "injection");
        assert_eq!(family(None, &["Hardcoded Secret".into()]), "auth");
        assert_eq!(family(None, &["data_flow".into(), "ssrf".into()]), "other");
    }

    #[test]
    fn findings_sort_by_severity_confidence_mode_then_line() {
        let mut taint_medium = taint_finding("os.system", "system", "f");
        taint_medium.line = 9;
        taint_medium.severity = "medium".into();
        taint_medium.confidence = "high".into();
        let raw = vec![
            pattern_finding(1, "low", "high"),
            pattern_finding(8, "medium", "high"),
            taint_medium,
            pattern_finding(5, "high", "low"),
            pattern_finding(4, "high", "high"),
            pattern_finding(2, "critical", "low"),
        ];
        let lines: Vec<u32> = map_and_sort(&raw, None).iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![2, 4, 5, 9, 8, 1]);
    }

    #[test]
    fn advisory_counts_the_full_list_when_the_cap_truncates() {
        let mut raw: Vec<Finding> = (1..=24)
            .map(|line| pattern_finding(line, "medium", "medium"))
            .collect();
        let mut critical = taint_finding("os.system", "system", "f");
        critical.line = 99;
        critical.severity = "Critical".into();
        critical.cwe_id = Some("cwe-78".into());
        raw.push(critical); // last in scan order
        let mut weak_high = pattern_finding(50, "high", "low");
        weak_high.cwe_id = Some("cwe-79".into());
        raw.push(weak_high);

        let report =
            crate::security_findings::SecurityReport::from_full(map_and_sort(&raw, None), true, 20);
        assert_eq!(report.findings.len(), 20);
        assert_eq!(report.findings[0].line, 99, "critical is shown first");
        assert_eq!(report.findings[0].severity.as_deref(), Some("critical"));
        let advisory = report.advisory.expect("scanner ran");
        assert_eq!(advisory.max_severity, "critical");
        assert_eq!(advisory.omitted, 26 - 20);
        assert_eq!(
            advisory.actionable, 1,
            "high+low-confidence is not actionable"
        );
        assert_eq!(advisory.taint, 1);
        // Pattern findings titled "Command Execution" fall to keywords.
        assert_eq!(advisory.by_family.get("injection"), Some(&25));
        assert_eq!(advisory.by_family.get("xss"), Some(&1));
        assert!(!advisory.by_family.contains_key("other"));
    }

    #[test]
    fn advisory_absent_unless_the_scanner_ran() {
        let report = crate::security_findings::SecurityReport::from_full(Vec::new(), false, 20);
        assert!(report.advisory.is_none());
        let zero = crate::security_findings::SecurityReport::from_full(Vec::new(), true, 20)
            .advisory
            .expect("ran with no findings");
        assert_eq!(zero.max_severity, "none");
        assert_eq!((zero.actionable, zero.taint, zero.omitted), (0, 0, 0));
    }
}
