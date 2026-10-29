use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use topos_engine::config::Severity;
use topos_engine::evaluation::policies::base::Priority;

use super::super::gates::Waived;
use super::super::mcp::{full_value, respond_untimed};
use super::super::model::{Headline, Hotspot, SkippedFile};
use super::super::progress::Timing;
use super::super::tests::{
    branchy, commit_all, judging, no_coupling, recap, write_files, write_repo,
};
use super::super::{build_recap, PrRecap, SCHEMA};
use super::*;
use crate::schemas::{Detail, PrRecapInput};

/// Summary JSON for the 41-file fixture (3 blocks, 38 warnings) stays under
/// this; the same range's full v3 document is ~74K.
const SUMMARY_CEILING_CHARS: usize = 4_500;
const TEXT_CEILING_CHARS: usize = 3_000;

const TIMING: Timing = Timing {
    elapsed_ms: 7,
    scoring_ms: Some(5),
};

/// `n` new files over the complexity gate, the first `insecure` of them
/// also calling `eval`, plus one clean one.
fn fixture(n: usize, insecure: usize) -> (tempfile::TempDir, PathBuf) {
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

fn input(repo: &Path, detail: Detail, paths: &[&str]) -> PrRecapInput {
    let mut params: PrRecapInput = serde_json::from_str("{}").unwrap();
    params.directory = Some(repo.to_str().unwrap().to_string());
    params.base = Some("HEAD~1".to_string());
    params.head = Some("HEAD".to_string());
    params.no_coupling = true;
    params.detail = detail;
    params.paths = paths.iter().map(|p| p.to_string()).collect();
    params
}

fn summarize(recap: &PrRecap, repo: &Path) -> Summary {
    build(recap, repo, &[], TIMING)
}

fn json(summary: &Summary) -> Value {
    serde_json::to_value(summary).unwrap()
}

fn counting(recap: &PrRecap, severity: Severity) -> usize {
    let counts = |f: &&crate::pr_recap::gates::Finding| f.counts() && f.severity == severity;
    recap.findings.iter().filter(counts).count()
}

/// One info finding on `path`, taken from a real one.
fn info_on(recap: &mut PrRecap, path: &str) {
    let mut finding = recap.findings[0].clone();
    finding.severity = Severity::Info;
    finding.path = path.to_string();
    finding.waived = None;
    recap.findings = vec![finding];
}

fn abs(repo: &Path, relative: &str) -> String {
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
fn attention_names_the_reason_file_and_every_block_file() {
    let (_keep, repo) = fixture(4, 2);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let summary = summarize(&recap, &repo);
    let attention: Vec<&str> = summary.attention.iter().map(|a| a.path.as_str()).collect();
    let lead = recap.findings.iter().find(|f| f.counts()).unwrap();
    assert_eq!(attention[0], abs(&repo, &lead.path));
    for finding in recap.findings.iter().filter(|f| f.counts()) {
        if finding.severity == Severity::Block {
            assert!(attention.contains(&abs(&repo, &finding.path).as_str()));
        }
    }
    let blocks = summary
        .attention
        .iter()
        .take_while(|a| a.severity == Some("block"))
        .count();
    assert!(blocks > 0);
    assert!(summary.attention[blocks..]
        .iter()
        .all(|a| a.severity != Some("block")));
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
fn summary_and_text_stay_under_their_ceilings() {
    let (_keep, repo) = fixture(40, 3);
    let response = respond_untimed(input(&repo, Detail::Summary, &[]), None);
    let structured = response.structured_content.expect("structured");
    let size = serde_json::to_string(&structured).unwrap().len();
    let text = response.content[0].as_text().unwrap().text.clone();
    let full = respond_untimed(input(&repo, Detail::Full, &[]), None);
    let full_size = serde_json::to_string(&full.structured_content.unwrap())
        .unwrap()
        .len();
    eprintln!(
        "summary {size} chars, text {} chars, full {full_size}",
        text.len()
    );
    assert_eq!(structured["schema"], SUMMARY_SCHEMA);
    assert!(size < SUMMARY_CEILING_CHARS, "{size} chars");
    assert!(
        text.len() < TEXT_CEILING_CHARS,
        "{} chars:\n{text}",
        text.len()
    );
    assert!(size * 3 < full_size);
}

#[test]
fn incomplete_hint_names_the_mcp_parameter() {
    let (_keep, repo) = fixture(3, 1);
    let mut params = input(&repo, Detail::Summary, &[]);
    params.max_files = 1;
    let structured = respond_untimed(params, None).structured_content.unwrap();
    assert_eq!(structured["incomplete"], true);
    let reason = structured["reason"].as_str().unwrap();
    assert!(!reason.contains("--max-files"), "{reason}");
    assert!(reason.contains("max_files"), "{reason}");
}

fn mcp_recap(repo: &Path, max_files: usize) -> PrRecap {
    let mcp_judging = crate::pr_recap::Judging {
        cap_name: "max_files",
        ..judging(Priority::Secure)
    };
    build_recap(
        repo,
        "HEAD~1",
        "HEAD",
        max_files,
        None,
        no_coupling(),
        &mcp_judging,
        &mut (),
    )
    .unwrap()
}

#[test]
fn full_mode_is_the_cli_document_apart_from_the_parameter_name() {
    let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
    let files = [
        ("src/--max-files.py", branchy("a")),
        ("src/b.py", branchy("b")),
        ("src/c.py", branchy("c")),
    ];
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, b)| (*p, b.as_str())).collect();
    write_files(&repo, &borrowed);
    commit_all(&repo, "add");
    // Nothing capped: the two documents are the same value.
    let cli = serde_json::to_value(recap(&repo, "HEAD~1", "HEAD", 0)).unwrap();
    let mcp = serde_json::to_value(mcp_recap(&repo, 0)).unwrap();
    assert_eq!(cli, mcp);
    assert!(cli.to_string().contains("src/--max-files.py"));

    // Capped: only the hints name their own parameter, and a path that
    // contains the CLI flag is left alone.
    let cli = serde_json::to_string(&recap(&repo, "HEAD~1", "HEAD", 1)).unwrap();
    let mcp = serde_json::to_string(&mcp_recap(&repo, 1)).unwrap();
    assert!(cli.contains("raise --max-files") && mcp.contains("raise max_files"));
    let unify = |text: &str, name: &str| {
        text.replace(&format!("(raise {name})"), "(raise CAP)")
            .replace(&format!("over the {name} cap"), "over the CAP cap")
            .replace(&format!("Raise {name} to"), "Raise CAP to")
    };
    assert_eq!(unify(&cli, "--max-files"), unify(&mcp, "max_files"));
    assert!(mcp.contains("src/--max-files.py"));
}

fn resolve(recap: &PrRecap, root: &Path, asked: &[&str]) -> Resolved {
    let asked: Vec<String> = asked.iter().map(|p| p.to_string()).collect();
    resolve_paths(recap, root, &asked)
}

fn placed(resolved: &Resolved) -> (usize, usize) {
    (resolved.found.len(), resolved.missing.len())
}

#[test]
fn absolute_paths_resolve_through_symlinks() {
    let (_keep, repo) = fixture(3, 1);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let outside = tempfile::tempdir().unwrap();
    let link = outside.path().join("link");
    std::os::unix::fs::symlink(&repo, &link).unwrap();
    let real = repo.canonicalize().unwrap();
    for (root, base) in [
        (&repo, &link),
        (&link, &repo),
        (&repo, &real),
        (&real, &repo),
    ] {
        let path = base.join("src/big1.py").display().to_string();
        let resolved = resolve(&recap, root, &[path.as_str()]);
        assert_eq!(placed(&resolved), (1, 0), "{root:?} {base:?}");
    }
}

#[test]
fn relative_paths_are_cleaned_lexically_and_cannot_leave_the_repository() {
    let (_keep, repo) = fixture(3, 1);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    for raw in ["././src/big1.py", "src/../src/big1.py", "./src//big1.py"] {
        assert_eq!(placed(&resolve(&recap, &repo, &[raw])), (1, 0), "{raw}");
    }
    let escaped = resolve(&recap, &repo, &["../x.py", "src/../../x.py", "/etc/passwd"]);
    assert!(escaped.found.is_empty());
    let reasons: Vec<&str> = escaped.missing.iter().map(|m| m.reason.as_str()).collect();
    assert_eq!(reasons[..2], ["outside the repository"; 2]);
}

#[test]
fn a_backslash_is_part_of_a_file_name_on_unix() {
    let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
    write_files(
        &repo,
        &[
            ("a\\b.py", &branchy("odd")),
            ("a/b.py", "def ok(x):\n    return x\n"),
        ],
    );
    commit_all(&repo, "add");
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let resolved = resolve(&recap, &repo, &["a\\b.py"]);
    assert_eq!(placed(&resolved), (1, 0));
    assert_eq!(recap.files[resolved.found[0]].path, "a\\b.py");
    assert_eq!(placed(&resolve(&recap, &repo, &["src\\b.py"])), (0, 1));
}

#[test]
fn an_in_repo_path_with_a_missing_directory_is_resolved_lexically() {
    let (_keep, repo) = fixture(2, 0);
    let recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let gone = abs(&repo, "gone/dir/x.py");
    let resolved = resolve(&recap, &repo, &[gone.as_str()]);
    assert_eq!(resolved.missing[0].reason, "not changed in this range");
    let unresolvable = resolve(&recap, &repo, &["/no/such/dir/x.py"]);
    assert_eq!(unresolvable.missing[0].reason, "could not resolve the path");
}

#[test]
fn missing_paths_are_deduped_and_say_why() {
    let (_keep, repo) = fixture(2, 1);
    let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
    recap.skipped.push(SkippedFile {
        path: "docs/huge.py".to_string(),
        reason: "over the size limit".to_string(),
    });
    recap.deleted.push("old.py".to_string());
    let asked = [
        "nope.py",
        "./nope.py",
        "nope.py",
        "docs/huge.py",
        "old.py",
        "",
        "  ",
    ];
    let resolved = resolve(&recap, &repo, &asked);
    assert!(resolved.found.is_empty());
    let reasons: Vec<(&str, &str)> = resolved
        .missing
        .iter()
        .map(|m| (m.path.as_str(), m.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        vec![
            ("nope.py", "not changed in this range"),
            ("docs/huge.py", "skipped: over the size limit"),
            ("old.py", "deleted in this range"),
            ("", "invalid path: empty"),
            ("  ", "invalid path: empty"),
        ]
    );
}

#[test]
fn attention_leads_with_the_reason_file_whatever_made_it_the_reason() {
    let (_keep, repo) = fixture(8, 0);
    let base = recap(&repo, "HEAD~1", "HEAD", 0);
    let last = base.files.last().unwrap().path.clone();
    let last_abs = abs(&repo, &last);

    let mut info = base.clone();
    info_on(&mut info, &last);
    info.reason_path = Some(last.clone());
    let summary = summarize(&info, &repo);
    assert_eq!(summary.attention[0].path, last_abs);
    assert!(summary.attention[0].why.starts_with("info"));
    assert!(summary.findings.is_empty());
    assert_eq!(summary.findings_total["info"], 1);

    let mut waived = base.clone();
    for finding in &mut waived.findings {
        finding.waived = Some(Waived {
            reason: "r".into(),
            source: "s".into(),
            expires: None,
        });
    }
    waived.reason_path = Some(last.clone());
    let summary = summarize(&waived, &repo);
    assert_eq!(summary.attention[0].path, last_abs);
    assert!(summary.findings.is_empty());
    assert_eq!(summary.findings_total["waived"], waived.findings.len());

    let mut none = base.clone();
    none.findings.clear();
    none.reason_path = Some(last);
    let summary = summarize(&none, &repo);
    assert_eq!(summary.attention[0].path, last_abs);
    assert_eq!(summary.attention[0].why, "named in reason");

    let lead = base.findings.iter().find(|f| f.counts()).unwrap();
    assert_eq!(base.reason_path.as_deref(), Some(lead.path.as_str()));
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
    assert_eq!(summary.worst_file_direction, Some(recap.direction));
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
    let schema = serde_json::to_string(&schemars::schema_for!(Summary)).unwrap();
    for key in [
        "rollup_lost_pillar",
        "worst_file",
        "attention",
        "elapsed_ms",
        "omitted",
    ] {
        assert!(schema.contains(key), "{key}");
    }
}

#[test]
fn errors_keep_the_shape_of_the_mode_asked_for() {
    let params = |detail| {
        let mut params = input(Path::new("/no/such/dir"), detail, &[]);
        params.directory = Some("/no/such/dir".to_string());
        params
    };
    let summary = respond_untimed(params(Detail::Summary), None);
    let body = summary.structured_content.unwrap();
    assert_eq!(body["schema"], SUMMARY_SCHEMA);
    assert!(body["error"].as_str().is_some());
    assert_eq!(summary.is_error, Some(true));
    let full = respond_untimed(params(Detail::Full), None);
    let body = full.structured_content.unwrap();
    assert!(body.get("schema").is_none());
    assert!(body["error"].as_str().is_some());
}

#[test]
fn text_covers_info_only_incomplete_direction_and_paths() {
    let (_keep, repo) = fixture(3, 1);
    let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let first = recap.files[0].path.clone();
    info_on(&mut recap, &first);
    recap.reason_path = Some(first);
    let text = summarize(&recap, &repo).to_markdown();
    assert!(text.contains("Findings (info 1):"), "{text}");
    assert!(text.contains("Worst file direction:"), "{text}");
    assert!(text.contains("independent of readiness"), "{text}");
    assert!(text.contains("ms scoring"), "{text}");
    assert!(!text.contains("Incomplete:"));

    recap.incomplete = true;
    recap.scope.files_capped = 4;
    recap.reason = "x".repeat(900);
    let text = summarize(&recap, &repo).to_markdown();
    assert!(
        text.contains("Incomplete: 4 file(s) over the max_files cap"),
        "{text}"
    );
    assert!(text.contains('…'));

    let asked = ["src/big1.py".to_string(), "nope.py".to_string()];
    let text = build(&recap, &repo, &asked, TIMING).to_markdown();
    assert!(
        text.contains(&format!("File `{}`", abs(&repo, "src/big1.py"))),
        "{text}"
    );
    assert!(text.contains("medal"), "{text}");
    assert!(
        text.contains("hotspot ast.max_function_complexity"),
        "{text}"
    );
    assert!(
        text.contains("Not found `nope.py`: not changed in this range"),
        "{text}"
    );
}

#[test]
fn a_requested_record_counts_the_hotspots_it_does_not_print() {
    let (_keep, repo) = fixture(2, 0);
    let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
    let spot = recap.files[0].hotspots[0].clone();
    recap.files[0].hotspots = vec![spot; 5];
    let path = recap.files[0].path.clone();
    let text = build(&recap, &repo, &[path], TIMING).to_markdown();
    assert_eq!(
        text.matches("hotspot ast.max_function_complexity").count(),
        2
    );
    assert!(text.contains("+3 more hotspots"), "{text}");
}

#[test]
fn full_detail_text_lists_paths_not_found() {
    let (_keep, repo) = fixture(2, 0);
    let response = respond_untimed(input(&repo, Detail::Full, &["nope.py"]), None);
    let text = &response.content[0].as_text().unwrap().text;
    assert!(
        text.contains("Not found `nope.py`: not changed in this range"),
        "{text}"
    );
}

#[test]
fn the_summary_reports_what_its_times_measure() {
    let (_keep, repo) = fixture(2, 0);
    let response = respond_untimed(input(&repo, Detail::Summary, &[]), None);
    let scope = &response.structured_content.unwrap()["scope"];
    let (total, scoring) = (scope["elapsed_ms"].as_u64(), scope["scoring_ms"].as_u64());
    assert!(scoring.unwrap() <= total.unwrap());
}
