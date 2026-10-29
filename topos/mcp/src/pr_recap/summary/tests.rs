use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use topos_engine::config::Severity;

use super::super::mcp::full_value;
use super::super::model::{Headline, Hotspot};
use super::super::progress::Timing;
use super::super::tests::{branchy, commit_all, recap, write_files, write_repo};
use super::super::verdict::direction_for;
use super::super::{PrRecap, SCHEMA};
use super::*;
use crate::schemas::{Detail, PrRecapInput};

pub(in crate::pr_recap) const TIMING: Timing = Timing {
    elapsed_ms: 7,
    scoring_ms: Some(5),
};

/// `n` new files over the complexity gate, the first `insecure` of them
/// also calling `eval`, plus one clean one.
pub(in crate::pr_recap) fn fixture(n: usize, insecure: usize) -> (tempfile::TempDir, PathBuf) {
    let (keep, repo) = write_repo(&[("README.md", "# hi\n")]);
    let mut files: Vec<(String, String)> = (0..n)
        .map(|i| {
            let mut body = branchy(&format!("pick{i}"));
            if i < insecure {
                body.push_str(&format!("\ndef run{i}(x):\n    return eval(x)\n"));
            }
            (format!("src/big{i}.py"), body)
        })
        .collect();
    files.push(("src/ok.py".into(), "def ok(x):\n    return x\n".into()));
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, body)| (path.as_str(), body.as_str()))
        .collect();
    write_files(&repo, &borrowed);
    commit_all(&repo, "add");
    (keep, repo)
}

pub(in crate::pr_recap) fn input(repo: &Path, detail: Detail, paths: &[&str]) -> PrRecapInput {
    let mut params: PrRecapInput = serde_json::from_str("{}").unwrap();
    params.directory = Some(repo.to_str().unwrap().to_string());
    params.base = Some("HEAD~1".to_string());
    params.head = Some("HEAD".to_string());
    params.no_coupling = true;
    params.detail = detail;
    params.paths = paths.iter().map(|p| p.to_string()).collect();
    params
}

pub(in crate::pr_recap) fn summarize(recap: &PrRecap, repo: &Path) -> Summary {
    build(recap, repo, &[], TIMING)
}

pub(in crate::pr_recap) fn json(summary: &Summary) -> Value {
    serde_json::to_value(summary).unwrap()
}

pub(in crate::pr_recap) fn counting(recap: &PrRecap, severity: Severity) -> usize {
    let counts = |f: &&crate::pr_recap::gates::Finding| f.counts() && f.severity == severity;
    recap.findings.iter().filter(counts).count()
}

/// One info finding on `path`, taken from a real one.
pub(in crate::pr_recap) fn info_on(recap: &mut PrRecap, path: &str) {
    let mut finding = recap.findings[0].clone();
    finding.severity = Severity::Info;
    finding.path = path.to_string();
    finding.waived = None;
    recap.findings = vec![finding];
}

pub(in crate::pr_recap) fn abs(repo: &Path, relative: &str) -> String {
    repo.join(relative).display().to_string()
}

#[test]
fn defaults_are_summary_all_files_no_paths() {
    let parsed: PrRecapInput = serde_json::from_str("{}").unwrap();
    assert_eq!(parsed.max_files, 0);
    assert_eq!(parsed.detail, Detail::Summary);
    assert!(parsed.paths.is_empty());
}

#[test]
fn summary_and_full_agree_on_the_verdict() {
    let (_keep, repo) = fixture(4, 2);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let value = json(&summarize(&recap, &repo));
    assert_eq!(value["schema"], SUMMARY_SCHEMA);
    let full = serde_json::to_value(&recap).unwrap();
    for key in ["check", "exit_code", "readiness", "reason", "base", "head"] {
        assert_eq!(value[key], full[key], "{key}");
    }
    assert_eq!(value["worst_file_direction"], full["direction"]);
    for severity in [Severity::Block, Severity::Warn] {
        let total = value["findings_total"][severity.as_str()].as_u64();
        assert_eq!(total.unwrap_or(0) as usize, counting(&recap, severity));
    }
    assert!(value["findings"][0].get("fix").is_none());
}

#[test]
fn every_path_in_the_summary_is_absolute() {
    let (_keep, repo) = fixture(4, 2);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let asked = ["src/big1.py".to_string()];
    let value = json(&build(&recap, &repo, &asked, TIMING));
    let root = value["root"].as_str().unwrap().to_string();
    let under = |path: &Value| path.as_str().unwrap().starts_with(&root);
    for finding in value["findings"].as_array().unwrap() {
        assert!(under(&finding["path"]), "{finding}");
    }
    for file in value["attention"].as_array().unwrap() {
        assert!(under(&file["path"]), "{file}");
    }
    assert!(under(&value["worst_file"]));
    assert_eq!(value["files"][0]["path"], abs(&repo, "src/big1.py"));
    for spot in value["files"][0]["hotspots"].as_array().unwrap() {
        assert!(under(&spot["path"]), "{spot}");
    }
    // The full document keeps the CLI's repo-relative paths.
    let (full, _) = full_value(&recap, &repo, &[]);
    assert!(full["files"][0]["path"]
        .as_str()
        .unwrap()
        .starts_with("src/"));
}

#[test]
fn blocks_are_never_dropped_by_the_finding_cap() {
    let (_keep, repo) = fixture(14, 14);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let blocks = counting(&recap, Severity::Block);
    assert!(blocks > MAX_FINDINGS, "fixture has {blocks} blocks");
    let summary = summarize(&recap, &repo);
    assert_eq!(summary.findings.len(), blocks);
    assert!(summary.findings.iter().all(|f| f.severity == "block"));
    assert!(summary.attention.len() >= blocks.min(14));
}

#[test]
fn warnings_fill_the_cap_after_the_blocks() {
    let (_keep, repo) = fixture(2, 1);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let summary = summarize(&recap, &repo);
    assert!(summary.findings.len() <= MAX_FINDINGS);
    let severities: Vec<&str> = summary.findings.iter().map(|f| f.severity).collect();
    let first_warn = severities.iter().position(|s| *s == "warn");
    let first_warn = first_warn.unwrap_or(severities.len());
    assert!(severities[first_warn..].iter().all(|s| *s != "block"));
}

#[test]
fn paths_return_requested_records_and_report_unknown() {
    let (_keep, repo) = fixture(3, 1);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let paths = ["src/big1.py".to_string(), "nope.py".to_string()];
    let value = json(&build(&recap, &repo, &paths, TIMING));
    assert_eq!(value["files"].as_array().unwrap().len(), 1);
    assert!(value["files"][0].get("medal_after").is_some());
    assert_eq!(value["paths_not_found"][0]["path"], "nope.py");
    assert_eq!(
        value["paths_not_found"][0]["reason"],
        "not changed in this range"
    );

    let (full, missing) = full_value(&recap, &repo, &paths);
    assert_eq!(full["schema"], SCHEMA);
    assert_eq!(full["files"].as_array().unwrap().len(), 1);
    assert_eq!(full["paths_not_found"][0]["path"], "nope.py");
    assert_eq!(missing.len(), 1);
    let (whole, _) = full_value(&recap, &repo, &[]);
    assert_eq!(whole, serde_json::to_value(&recap).unwrap());
}

#[test]
fn metrics_describe_every_metric_the_findings_show() {
    let (_keep, repo) = fixture(2, 1);
    let summary = summarize(&recap(&repo, "HEAD~1", "HEAD", 0), &repo);
    let shown: Vec<&str> = summary
        .findings
        .iter()
        .filter_map(|finding| finding.metric.as_deref())
        .collect();
    assert!(!shown.is_empty());
    for metric in shown {
        assert!(summary.metrics.contains_key(metric), "{metric}");
    }
}

#[test]
fn metrics_include_those_only_a_requested_record_shows() {
    let (_keep, repo) = fixture(2, 0);
    let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let index = recap.files.len() - 1;
    recap.files[index].hotspots = vec![Hotspot {
        path: recap.files[index].path.clone(),
        line: 1,
        function: None,
        metric: "cpg.taint_flows".to_string(),
        detail: "a flow".to_string(),
        advice: String::new(),
    }];
    let path = recap.files[index].path.clone();
    let without = summarize(&recap, &repo);
    let with = build(&recap, &repo, &[path], TIMING);
    assert!(!with.files.is_empty());
    assert!(with.metrics.contains_key("cpg.taint_flows"));
    assert!(with.metrics.len() >= without.metrics.len());
}

#[test]
fn direction_text_names_its_file_in_a_real_recap() {
    let (_keep, repo) = write_repo(&[("a.py", &branchy("solve"))]);
    write_files(&repo, &[("a.py", "def solve(x):\n    return x\n")]);
    commit_all(&repo, "simplify");
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    assert!(recap.findings.iter().all(|f| !f.counts()));
    if recap.direction != Headline::LateralMove {
        assert_eq!(recap.reason_path.as_deref(), Some("a.py"));
        let summary = summarize(&recap, &repo);
        assert_eq!(summary.attention[0].path, abs(&repo, "a.py"));
    }
    let summary = summarize(&recap, &repo);
    assert_eq!(summary.worst_file, Some(abs(&repo, "a.py")));
    assert_eq!(summary.worst_file_direction, Some(recap.files[0].status));
}

#[test]
fn an_empty_range_has_no_direction() {
    let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
    write_files(&repo, &[("README.md", "# changed\n")]);
    commit_all(&repo, "docs only");
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    assert!(recap.files.is_empty());
    let summary = summarize(&recap, &repo);
    assert_eq!(summary.worst_file_direction, None);
    assert_eq!(summary.worst_file, None);
    let value = json(&summary);
    assert!(value.get("worst_file_direction").is_none());
    assert!(!summary.to_markdown().contains("Worst file direction"));
}

#[test]
fn hotspots_carry_their_measure_and_are_counted() {
    let (_keep, repo) = fixture(8, 0);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let summary = summarize(&recap, &repo);
    let spot = summary.attention.iter().find_map(|a| a.hotspot.as_ref());
    assert!(spot.expect("a hotspot").detail.contains("gate is"));
    let total: usize = recap.files.iter().map(|f| f.hotspots.len()).sum();
    let shown = summary
        .attention
        .iter()
        .filter(|a| a.hotspot.is_some())
        .count();
    assert!(total > shown, "fixture should leave hotspots out");
    assert_eq!(summary.omitted.hotspots, total - shown);
    assert_eq!(json(&summary)["omitted"]["hotspots"], total - shown);
}

#[test]
fn omitted_files_excludes_files_shown_in_attention_or_files() {
    let (_keep, repo) = fixture(12, 2);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let asked = ["src/big11.py".to_string()];
    let summary = build(&recap, &repo, &asked, TIMING);
    let mut listed: BTreeSet<String> = summary.attention.iter().map(|a| a.path.clone()).collect();
    listed.insert(abs(&repo, "src/big11.py"));
    assert_eq!(summary.omitted.files, recap.files.len() - listed.len());
    assert_eq!(summary.files.len(), 1);
}

#[test]
fn bound_survives_rounding() {
    let (_keep, repo) = fixture(1, 1);
    let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let finding = &mut recap.findings[0];
    finding.before = Some(11.0);
    finding.after = Some(9.9996);
    finding.limit = Some(10.0);
    let slim = slim(finding, &Absolute::new(&repo));
    assert_eq!((slim.after, slim.limit), (Some(9.9996), Some(10.0)));
    assert_eq!(slim.bound, Some("min"));
    finding.after = Some(10.0);
    assert_eq!(slim_bound(finding, &repo), None);
}

fn slim_bound(finding: &crate::pr_recap::gates::Finding, repo: &Path) -> Option<&'static str> {
    slim(finding, &Absolute::new(repo)).bound
}

#[test]
fn nan_values_are_null_with_no_bound() {
    let (_keep, repo) = fixture(1, 1);
    let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
    recap.findings[0].after = Some(f64::NAN);
    recap.findings[0].limit = Some(10.0);
    let summary = summarize(&recap, &repo);
    let value = json(&summary);
    assert_eq!(summary.findings[0].bound, None);
    assert!(value["findings"][0]["after"].is_null());
    assert!(value["findings"][0].get("bound").is_none());
    assert!(!summary.to_markdown().is_empty());
}

#[test]
fn the_rollups_and_scope_are_typed_fields() {
    let (_keep, repo) = write_repo(&[("a.py", &branchy("solve"))]);
    write_files(&repo, &[("a.py", "def solve(x):\n    return x\n")]);
    commit_all(&repo, "simplify");
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let value = json(&summarize(&recap, &repo));
    assert_eq!(value["project"]["rollup_lost_pillar"], false);
    assert!(value["project"].get("regression").is_none());
    assert_eq!(value["scope"]["elapsed_ms"], 7);
    assert_eq!(value["scope"]["scoring_ms"], 5);
    assert_eq!(value["scope"]["files_scored"], 1);
    assert!(value["scope"]["coupling"].is_object());
}

#[test]
fn the_summary_has_a_json_schema() {
    let schema = serde_json::to_value(schemars::schema_for!(Summary)).unwrap();
    let required = |node: &Value| -> Vec<String> {
        let names = node["required"].as_array().cloned().unwrap_or_default();
        names
            .iter()
            .map(|n| n.as_str().unwrap().to_string())
            .collect()
    };
    let top = required(&schema);
    for field in [
        "schema",
        "root",
        "readiness",
        "check",
        "exit_code",
        "scope",
        "findings",
        "attention",
    ] {
        assert!(
            top.iter().any(|f| f == field),
            "{field} not required in {top:?}"
        );
    }
    // Optional parts are not required.
    for field in [
        "worst_file",
        "worst_file_direction",
        "project",
        "files",
        "omitted",
    ] {
        assert!(!top.iter().any(|f| f == field), "{field}");
    }
    assert!(schema["properties"]["worst_file"].is_object());
    // `scope` is v3's scope plus the timings, flattened into one object.
    let scope = &schema["$defs"]["SummaryScope"];
    assert!(scope["properties"]["files_scored"].is_object());
    assert!(scope["properties"]["elapsed_ms"].is_object());
    let scope_required = required(scope);
    assert!(scope_required.iter().any(|f| f == "elapsed_ms"));
    assert!(!scope_required.iter().any(|f| f == "scoring_ms"));
    let project = &schema["$defs"]["SummaryProject"]["properties"];
    assert!(project["rollup_lost_pillar"].is_object());
    assert!(project.get("regression").is_none());
}

#[test]
fn the_worst_file_and_its_direction_are_one_files_status() {
    let (_keep, repo) = fixture(3, 0);
    let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
    recap.files[0].status = Headline::Improvement;
    recap.files[1].status = Headline::ImprovementScore;
    recap.files[2].status = Headline::Improvement;
    let (direction, _, named) = direction_for(&recap.files, &recap.base, &recap.head);
    // The files disagree on an improvement, so the aggregate is lateral and
    // names no file; the summary still pairs the worst file with its own status.
    assert_eq!((direction, named), (Headline::LateralMove, None));
    recap.direction = direction;
    let summary = summarize(&recap, &repo);
    assert_eq!(
        summary.worst_file_direction,
        Some(Headline::ImprovementScore)
    );
    assert_eq!(summary.worst_file, Some(abs(&repo, &recap.files[1].path)));
}
