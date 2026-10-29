//! `topos.pr_recap.summary.v1`: the MCP default shape of a recap.
//!
//! The full `topos.pr_recap.v3` document runs to tens of thousands of
//! characters, mostly per-file records. This is a distinct struct built from
//! the computed [`PrRecap`]: the verdict, the blocking evidence with handles
//! (path, line, function, metric, before, after, limit) and one ranking of
//! the files that need attention. Everything left out is counted in
//! `omitted`, with the parameter that widens it in `more`. It states facts;
//! per-finding `fix` text stays in the full document, and `metrics` says
//! what each metric shown measures.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use topos_engine::config::Severity;

use super::gates::{metric_description, Finding, Readiness};
use super::model::{FileRecap, Headline, PrRecap};

pub const SUMMARY_SCHEMA: &str = "topos.pr_recap.summary.v1";

/// Findings listed in the summary: every block, then warnings up to this
/// total. Blocks are never dropped past it.
pub const MAX_FINDINGS: usize = 10;
/// Files in `attention`, unless more files carry a block finding.
pub const MAX_ATTENTION: usize = 5;

#[derive(Debug, Clone, Serialize)]
pub struct SummaryGate {
    pub preset: &'static str,
    pub fail_on: &'static str,
    pub changes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryFinding {
    pub gate: String,
    pub severity: &'static str,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pillar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    /// `min` when `after` is below `limit`, else `max`; only with a limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound: Option<&'static str>,
    pub text: String,
}

/// A file's top hotspot span.
#[derive(Debug, Clone, Serialize)]
pub struct AttentionSpot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    pub line: usize,
    pub metric: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttentionFile {
    pub path: String,
    /// Worst `block` or `warn` finding on the file; absent for a file that
    /// is only a hotspot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<&'static str>,
    /// Counting `block` and `warn` findings on the file.
    #[serde(skip_serializing_if = "is_zero")]
    pub findings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotspot: Option<AttentionSpot>,
}

/// Counts of what the summary leaves out; zero counts are not written.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Omitted {
    #[serde(skip_serializing_if = "is_zero")]
    pub findings: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub files: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub clusters: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Omitted {
    fn is_empty(&self) -> bool {
        self.findings + self.files + self.clusters + self.skipped == 0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub schema: &'static str,
    /// Absolute repository root, to join with repo-relative paths.
    pub root: String,
    pub base: String,
    pub head: String,
    pub priority: &'static str,
    pub readiness: Readiness,
    pub check: &'static str,
    pub exit_code: i32,
    pub reason: String,
    pub incomplete: bool,
    pub gate: SummaryGate,
    /// The worst single file's headline (v3 `direction`), not a verdict on
    /// the touched set.
    pub worst_file_direction: Headline,
    pub scope: Value,
    /// The touched-set rollup; `rollup_lost_pillar` is v3 `project.regression`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<Value>,
    pub findings: Vec<SummaryFinding>,
    /// Counting findings by severity, and how many a waiver covers.
    pub findings_total: BTreeMap<&'static str, usize>,
    pub attention: Vec<AttentionFile>,
    pub metrics: BTreeMap<String, &'static str>,
    #[serde(skip_serializing_if = "Omitted::is_empty")]
    pub omitted: Omitted,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths_not_found: Vec<String>,
    pub non_claim: &'static str,
    /// Full records for the requested `paths`; written by [`Summary::to_value`].
    #[serde(skip)]
    pub files: Vec<FileRecap>,
}

/// Indexes into `recap.files` for the requested `paths`, and the paths that
/// matched no scored file. `./` and the absolute repo root are stripped.
pub fn resolve_paths(recap: &PrRecap, root: &Path, paths: &[String]) -> (Vec<usize>, Vec<String>) {
    let mut found = BTreeSet::new();
    let mut missing = Vec::new();
    for raw in paths {
        let wanted = normalize(raw, root);
        match recap.files.iter().position(|file| file.path == wanted) {
            Some(index) => {
                found.insert(index);
            }
            None => missing.push(raw.clone()),
        }
    }
    (found.into_iter().collect(), missing)
}

fn normalize(raw: &str, root: &Path) -> String {
    let path = Path::new(raw);
    let relative = path.strip_prefix(root).unwrap_or(path);
    let text = relative.to_string_lossy();
    text.strip_prefix("./").unwrap_or(&text).to_string()
}

pub fn build(recap: &PrRecap, root: &Path, paths: &[String]) -> Summary {
    let counting: Vec<(usize, &Finding)> = recap
        .findings
        .iter()
        .enumerate()
        .filter(|(_, finding)| finding.counts())
        .collect();
    let blocks = counting
        .iter()
        .filter(|(_, finding)| finding.severity == Severity::Block)
        .count();
    let room = MAX_FINDINGS.saturating_sub(blocks);
    let mut warns = 0;
    let shown: Vec<&Finding> = counting
        .iter()
        .map(|(_, finding)| *finding)
        .filter(|finding| match finding.severity {
            Severity::Block => true,
            Severity::Warn => {
                warns += 1;
                warns <= room
            }
            _ => false,
        })
        .collect();

    let mut findings_total: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (_, finding) in &counting {
        *findings_total.entry(finding.severity.as_str()).or_default() += 1;
    }
    let waived = recap.findings.len() - counting.len();
    if waived > 0 {
        findings_total.insert("waived", waived);
    }

    let attention = attention(recap, &counting);
    let mut metrics = BTreeMap::new();
    let spot_metrics = attention
        .iter()
        .filter_map(|file| file.hotspot.as_ref().map(|spot| spot.metric.as_str()));
    let finding_metrics = shown.iter().filter_map(|f| f.metric.as_deref());
    for metric in finding_metrics.chain(spot_metrics) {
        if let Some(description) = metric_description(metric) {
            metrics.insert(metric.to_string(), description);
        }
    }

    let (included, paths_not_found) = resolve_paths(recap, root, paths);
    let omitted = Omitted {
        findings: recap.findings.len() - shown.len(),
        files: recap.files.len() - included.len(),
        clusters: recap.clusters.len(),
        skipped: recap.skipped.len(),
    };
    let more = (!omitted.is_empty()).then(|| {
        "detail: \"full\" returns the whole topos.pr_recap.v3 document; paths: [..] adds the full \
         records for the named files."
            .to_string()
    });

    Summary {
        schema: SUMMARY_SCHEMA,
        root: root.display().to_string(),
        base: recap.base.clone(),
        head: recap.head.clone(),
        priority: recap.priority,
        readiness: recap.readiness,
        check: recap.check,
        exit_code: recap.exit_code,
        reason: recap.reason.clone(),
        incomplete: recap.incomplete,
        gate: SummaryGate {
            preset: recap.gate.preset,
            fail_on: recap.gate.fail_on,
            changes: recap.gate.changes,
            source: recap.gate.source.clone(),
        },
        worst_file_direction: recap.direction,
        scope: serde_json::to_value(&recap.scope).unwrap_or(Value::Null),
        project: recap.project.as_ref().and_then(|project| {
            let mut value = serde_json::to_value(project).ok()?;
            let object = value.as_object_mut()?;
            let lost = object.remove("regression")?;
            object.insert("rollup_lost_pillar".to_string(), lost);
            Some(value)
        }),
        added: recap
            .added
            .as_ref()
            .and_then(|added| serde_json::to_value(added).ok()),
        findings: shown.into_iter().map(slim).collect(),
        findings_total,
        attention,
        metrics,
        omitted,
        more,
        paths_not_found,
        non_claim: recap.non_claim,
        files: included
            .into_iter()
            .map(|index| recap.files[index].clone())
            .collect(),
    }
}

fn slim(finding: &Finding) -> SummaryFinding {
    let bound = finding.limit.map(|limit| match finding.after {
        Some(after) if after < limit => "min",
        _ => "max",
    });
    SummaryFinding {
        gate: finding.gate.key().to_string(),
        severity: finding.severity.as_str(),
        path: finding.path.clone(),
        line: finding.line,
        function: finding.function.clone(),
        pillar: finding.pillar.clone(),
        metric: finding.metric.clone(),
        before: finding.before,
        after: finding.after,
        limit: finding.limit,
        bound,
        text: finding.text.clone(),
    }
}

/// One ranking of the files that need attention: files with a block
/// finding, then a warning, then files that are only hotspots; within a
/// tier by finding count, then by the order the gates ranked their first
/// finding. The file `reason` names leads. At least [`MAX_ATTENTION`]
/// files, and every file with a block finding.
fn attention(recap: &PrRecap, counting: &[(usize, &Finding)]) -> Vec<AttentionFile> {
    struct Entry<'a> {
        path: &'a str,
        tier: u8,
        count: usize,
        first: usize,
    }
    let mut entries: Vec<Entry> = Vec::new();
    for (index, finding) in counting {
        let tier = match finding.severity {
            Severity::Block => 2,
            Severity::Warn => 1,
            _ => continue,
        };
        if finding.path.is_empty() {
            continue;
        }
        match entries.iter_mut().find(|entry| entry.path == finding.path) {
            Some(entry) => {
                entry.tier = entry.tier.max(tier);
                entry.count += 1;
            }
            None => entries.push(Entry {
                path: &finding.path,
                tier,
                count: 1,
                first: *index,
            }),
        }
    }
    for file in recap.files.iter().filter(|file| !file.hotspots.is_empty()) {
        if entries.iter().all(|entry| entry.path != file.path) {
            entries.push(Entry {
                path: &file.path,
                tier: 0,
                count: file.hotspots.len(),
                first: usize::MAX,
            });
        }
    }
    let lead = counting
        .first()
        .map(|(_, finding)| finding.path.as_str())
        .filter(|path| !path.is_empty());
    entries.sort_by(|a, b| {
        (Some(b.path) == lead)
            .cmp(&(Some(a.path) == lead))
            .then(b.tier.cmp(&a.tier))
            .then(b.count.cmp(&a.count))
            .then(a.first.cmp(&b.first))
            .then(a.path.cmp(b.path))
    });
    let block_files = entries.iter().filter(|entry| entry.tier == 2).count();
    entries.truncate(MAX_ATTENTION.max(block_files));
    entries
        .into_iter()
        .map(|entry| {
            let hotspot = recap
                .files
                .iter()
                .find(|file| file.path == entry.path)
                .and_then(|file| file.hotspots.first())
                .map(|spot| AttentionSpot {
                    function: spot.function.clone(),
                    line: spot.line,
                    metric: spot.metric.clone(),
                });
            AttentionFile {
                path: entry.path.to_string(),
                severity: match entry.tier {
                    2 => Some("block"),
                    1 => Some("warn"),
                    _ => None,
                },
                findings: if entry.tier == 0 { 0 } else { entry.count },
                hotspot,
            }
        })
        .collect()
}

impl Summary {
    /// The JSON document: floats rounded to three significant figures, and
    /// the requested file records written as they are in v3.
    pub fn to_value(&self) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        round_floats(&mut value);
        if !self.files.is_empty() {
            if let (Some(object), Ok(files)) =
                (value.as_object_mut(), serde_json::to_value(&self.files))
            {
                object.insert("files".to_string(), files);
            }
        }
        value
    }

    /// A compact markdown rendering for clients that show only the text
    /// block.
    pub fn to_markdown(&self) -> String {
        const TEXT_FINDINGS: usize = 8;
        let mut out = format!(
            "**PR Recap ({})**: `{}` — {}\n`{}...{}` · {} file(s) scored · gate {}\n",
            self.check,
            self.readiness.word(),
            clip(&self.reason, 400),
            short(&self.base),
            short(&self.head),
            self.scope["files_scored"],
            self.gate.preset,
        );
        if !self.findings.is_empty() {
            let totals: Vec<String> = self
                .findings_total
                .iter()
                .map(|(severity, count)| format!("{severity} {count}"))
                .collect();
            out.push_str(&format!("Findings ({}):\n", totals.join(", ")));
            for finding in self.findings.iter().take(TEXT_FINDINGS) {
                out.push_str(&format!("- {}\n", bullet(finding)));
            }
            if self.findings.len() > TEXT_FINDINGS {
                out.push_str(&format!(
                    "- {} more in structuredContent\n",
                    self.findings.len() - TEXT_FINDINGS
                ));
            }
        }
        if !self.attention.is_empty() {
            out.push_str("Attention:\n");
            for file in &self.attention {
                let mut line = format!("- `{}`", file.path);
                if let Some(severity) = file.severity {
                    line.push_str(&format!(" {severity}"));
                }
                if file.findings > 0 {
                    line.push_str(&format!(" ×{}", file.findings));
                }
                if let Some(spot) = &file.hotspot {
                    line.push_str(&format!(
                        " · {}:{} {}",
                        spot.function.as_deref().unwrap_or("-"),
                        spot.line,
                        spot.metric
                    ));
                }
                out.push_str(&line);
                out.push('\n');
            }
        }
        let omitted = [
            ("findings", self.omitted.findings),
            ("files", self.omitted.files),
            ("clusters", self.omitted.clusters),
            ("skipped", self.omitted.skipped),
        ]
        .iter()
        .filter(|(_, count)| *count > 0)
        .map(|(name, count)| format!("{name} {count}"))
        .collect::<Vec<_>>();
        if let Some(more) = &self.more {
            out.push_str(&format!("Omitted: {}. {more}\n", omitted.join(", ")));
        }
        out
    }
}

fn bullet(finding: &SummaryFinding) -> String {
    let location = match finding.line {
        Some(line) => format!("`{}:{line}`", finding.path),
        None => format!("`{}`", finding.path),
    };
    let function = finding
        .function
        .as_deref()
        .map(|name| format!(" {name}"))
        .unwrap_or_default();
    let Some(metric) = &finding.metric else {
        return format!(
            "{} {location}{function} {}",
            finding.severity,
            clip(&finding.text, 160)
        );
    };
    let values = match (finding.before, finding.after) {
        (Some(before), Some(after)) => format!(" {}→{}", sig3(before), sig3(after)),
        (None, Some(after)) => format!(" {}", sig3(after)),
        _ => String::new(),
    };
    let limit = match (finding.bound, finding.limit) {
        (Some(bound), Some(limit)) => format!(" ({bound} {})", sig3(limit)),
        _ => String::new(),
    };
    format!(
        "{} {location}{function} {metric}{values}{limit}",
        finding.severity
    )
}

fn short(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{}…", kept.trim_end())
}

/// `x` to three significant figures.
pub fn sig3(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    format!("{x:.2e}").parse().unwrap_or(x)
}

fn round_floats(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            if let Some(rounded) = number
                .as_f64()
                .map(sig3)
                .and_then(serde_json::Number::from_f64)
            {
                *number = rounded;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(round_floats),
        Value::Object(map) => map.values_mut().for_each(round_floats),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{branchy, commit_all, recap, write_files, write_repo};
    use super::super::*;
    use super::*;
    use crate::schemas::PrRecapInput;

    /// Summary JSON for the 41-file fixture (3 blocks, 38 warnings) stays
    /// under this; the same range's full v3 document is ~74K.
    const SUMMARY_CEILING_CHARS: usize = 4_500;
    const TEXT_CEILING_CHARS: usize = 2_500;

    /// `n` new files over the complexity gate, the first `insecure` of them
    /// also calling `eval`, plus one clean one.
    fn fixture(n: usize, insecure: usize) -> (tempfile::TempDir, PathBuf) {
        let (keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        let mut files: Vec<(String, String)> = (0..n)
            .map(|i| {
                let mut body = branchy(&format!("pick{i}"));
                if i < insecure {
                    body.push_str(&format!(
                        "
def run{i}(x):
    return eval(x)
"
                    ));
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

    fn input(repo: &Path, detail: PrRecapDetail, paths: &[&str]) -> PrRecapInput {
        PrRecapInput {
            directory: Some(repo.to_str().unwrap().to_string()),
            pr: None,
            base: Some("HEAD~1".to_string()),
            head: Some("HEAD".to_string()),
            max_files: serde_json::from_str::<PrRecapInput>("{}")
                .unwrap()
                .max_files,
            detail,
            paths: paths.iter().map(|p| p.to_string()).collect(),
            no_coupling: true,
            priority: None,
            strict: false,
            preset: None,
        }
    }

    fn counting(recap: &PrRecap, severity: Severity) -> usize {
        recap
            .findings
            .iter()
            .filter(|f| f.counts() && f.severity == severity)
            .count()
    }

    #[test]
    fn defaults_are_summary_all_files_no_paths() {
        let parsed: PrRecapInput = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed.max_files, 0);
        assert_eq!(parsed.detail, PrRecapDetail::Summary);
        assert!(parsed.paths.is_empty());
    }

    #[test]
    fn summary_and_full_agree_on_the_verdict() {
        let (_keep, repo) = fixture(4, 2);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let summary = build(&recap, &repo, &[]);
        let value = summary.to_value();
        assert_eq!(value["schema"], SUMMARY_SCHEMA);
        let full = serde_json::to_value(&recap).unwrap();
        for key in ["check", "exit_code", "readiness", "reason", "base", "head"] {
            assert_eq!(value[key], full[key], "{key}");
        }
        assert_eq!(value["worst_file_direction"], full["direction"]);
        for severity in [Severity::Block, Severity::Warn] {
            let in_total = value["findings_total"][severity.as_str()]
                .as_u64()
                .unwrap_or(0);
            assert_eq!(in_total as usize, counting(&recap, severity));
        }
        assert!(value["findings"][0].get("fix").is_none());
    }

    #[test]
    fn attention_names_the_reason_file_and_every_block_file() {
        let (_keep, repo) = fixture(4, 2);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let summary = build(&recap, &repo, &[]);
        let attention: Vec<&str> = summary.attention.iter().map(|a| a.path.as_str()).collect();
        let lead = recap
            .findings
            .iter()
            .find(|f| f.counts())
            .expect("a finding");
        assert!(!lead.path.is_empty());
        assert_eq!(attention[0], lead.path);
        for finding in recap
            .findings
            .iter()
            .filter(|f| f.counts() && f.severity == Severity::Block)
        {
            assert!(attention.contains(&finding.path.as_str()), "{attention:?}");
        }
        let blocks: Vec<_> = summary
            .attention
            .iter()
            .take_while(|a| a.severity == Some("block"))
            .collect();
        assert!(!blocks.is_empty());
        assert!(summary
            .attention
            .iter()
            .skip(blocks.len())
            .all(|a| a.severity != Some("block")));
    }

    #[test]
    fn blocks_are_never_dropped_by_the_finding_cap() {
        let (_keep, repo) = fixture(14, 14);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let blocks = counting(&recap, Severity::Block);
        assert!(blocks > MAX_FINDINGS, "fixture has {blocks} blocks");
        let summary = build(&recap, &repo, &[]);
        let shown = summary
            .findings
            .iter()
            .filter(|f| f.severity == "block")
            .count();
        assert_eq!(shown, blocks);
        assert!(summary.findings.iter().all(|f| f.severity == "block"));
        assert!(summary.attention.len() >= blocks.min(14));
    }

    #[test]
    fn warnings_fill_the_cap_after_the_blocks() {
        let (_keep, repo) = fixture(2, 1);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let summary = build(&recap, &repo, &[]);
        assert!(summary.findings.len() <= MAX_FINDINGS);
        let sevs: Vec<&str> = summary.findings.iter().map(|f| f.severity).collect();
        let first_warn = sevs.iter().position(|s| *s == "warn").unwrap_or(sevs.len());
        assert!(sevs[first_warn..].iter().all(|s| *s != "block"));
    }

    #[test]
    fn paths_return_requested_records_and_report_unknown() {
        let (_keep, repo) = fixture(3, 1);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let paths = ["src/big1.py".to_string(), "nope.py".to_string()];
        let value = build(&recap, &repo, &paths).to_value();
        assert_eq!(value["files"].as_array().unwrap().len(), 1);
        assert_eq!(value["files"][0]["path"], "src/big1.py");
        assert!(value["files"][0].get("medal_after").is_some());
        assert_eq!(value["paths_not_found"][0], "nope.py");
        assert_eq!(value["omitted"]["files"], recap.files.len() - 1);

        let abs = repo.join("src/big2.py").display().to_string();
        let (found, missing) = resolve_paths(&recap, &repo, &[abs, "./src/ok.py".to_string()]);
        assert_eq!(found.len(), 2);
        assert!(missing.is_empty());

        let full = full_value(&recap, &repo, &paths);
        assert_eq!(full["schema"], SCHEMA);
        assert_eq!(full["files"].as_array().unwrap().len(), 1);
        assert_eq!(full["paths_not_found"][0], "nope.py");
        assert_eq!(
            full_value(&recap, &repo, &[]),
            serde_json::to_value(&recap).unwrap()
        );
    }

    #[test]
    fn metrics_describe_the_metrics_shown_and_floats_are_rounded() {
        let (_keep, repo) = fixture(2, 1);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let summary = build(&recap, &repo, &[]);
        for finding in &summary.findings {
            if let Some(metric) = finding
                .metric
                .as_deref()
                .filter(|m| metric_description(m).is_some())
            {
                assert!(summary.metrics.contains_key(metric));
            }
        }
        assert_eq!(sig3(12.34567), 12.3);
        assert_eq!(sig3(0.0123456), 0.0123);
        assert_eq!(sig3(1234.5), 1230.0);
        assert_eq!(sig3(-0.5556), -0.556);
        let mut value = serde_json::json!({ "a": [1.23456, 7], "b": { "c": 0.98765 } });
        round_floats(&mut value);
        assert_eq!(
            value,
            serde_json::json!({ "a": [1.23, 7], "b": { "c": 0.988 } })
        );
    }

    #[test]
    fn summary_and_text_stay_under_their_ceilings() {
        let (_keep, repo) = fixture(40, 3);
        let response = pr_recap_sync_with_cancel(input(&repo, PrRecapDetail::Summary, &[]), None);
        let structured = response.structured_content.expect("structured");
        let json = serde_json::to_string(&structured).unwrap();
        let text = response.content[0].as_text().unwrap().text.clone();
        let full = pr_recap_sync_with_cancel(input(&repo, PrRecapDetail::Full, &[]), None);
        let full_json = serde_json::to_string(&full.structured_content.unwrap()).unwrap();
        eprintln!(
            "summary {} chars, text {} chars, full {} chars",
            json.len(),
            text.len(),
            full_json.len()
        );
        assert_eq!(structured["schema"], SUMMARY_SCHEMA);
        assert!(json.len() < SUMMARY_CEILING_CHARS, "{} chars", json.len());
        assert!(
            text.len() < TEXT_CEILING_CHARS,
            "{} chars:\n{text}",
            text.len()
        );
        assert!(json.len() * 3 < full_json.len());
    }

    #[test]
    fn incomplete_hint_names_the_mcp_parameter() {
        let (_keep, repo) = fixture(3, 1);
        let mut params = input(&repo, PrRecapDetail::Summary, &[]);
        params.max_files = 1;
        let response = pr_recap_sync_with_cancel(params, None);
        let structured = response.structured_content.unwrap();
        assert_eq!(structured["incomplete"], true);
        let reason = structured["reason"].as_str().unwrap();
        assert!(!reason.contains("--max-files"), "{reason}");
        assert!(reason.contains("max_files"), "{reason}");
    }
}
