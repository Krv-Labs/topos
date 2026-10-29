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
use std::path::{Component, Path, PathBuf};

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
    /// `min` when `after` is below `limit`, else `max`; only with a finite
    /// limit and `after`. Decided on the unrounded values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound: Option<&'static str>,
    pub text: String,
}

/// A file's top hotspot span and what was measured there.
#[derive(Debug, Clone, Serialize)]
pub struct AttentionSpot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    pub line: usize,
    pub metric: String,
    /// The measured value against its gate, as in v3 (`pick complexity is
    /// 23, gate is 10`).
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttentionFile {
    pub path: String,
    /// Worst `block` or `warn` finding on the file; absent for a file that
    /// is only a hotspot or is named in `reason`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<&'static str>,
    /// Counting `block` and `warn` findings on the file.
    #[serde(skip_serializing_if = "is_zero")]
    pub findings: usize,
    /// Why the file is listed: its top finding's severity and gate,
    /// `hotspot`, or `named in reason`.
    pub why: String,
    /// Hotspots the file has; `hotspot` is the first.
    #[serde(skip_serializing_if = "is_zero")]
    pub hotspots: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotspot: Option<AttentionSpot>,
}

/// A requested path that matched no scored file, and why.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NotFound {
    pub path: String,
    pub reason: String,
}

/// Counts of what the summary leaves out; zero counts are not written.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Omitted {
    #[serde(skip_serializing_if = "is_zero")]
    pub findings: usize,
    /// Scored files with no entry in `attention` or `files`.
    #[serde(skip_serializing_if = "is_zero")]
    pub files: usize,
    /// Hotspots not shown, in `attention` or `files`.
    #[serde(skip_serializing_if = "is_zero")]
    pub hotspots: usize,
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
        self.findings + self.files + self.hotspots + self.clusters + self.skipped == 0
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
    /// The most severe single file's structural change (v3 `direction`),
    /// independent of readiness: it can be `SUSPICIOUS_NO_STRUCTURAL_CHANGE`
    /// on a passing PR.
    pub worst_file_direction: Headline,
    /// v3 `scope`, plus `elapsed_ms`: wall time to score the range.
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
    pub paths_not_found: Vec<NotFound>,
    pub non_claim: &'static str,
    /// Full records for the requested `paths`; written by [`Summary::to_value`].
    #[serde(skip)]
    pub files: Vec<FileRecap>,
}

/// Indexes into `recap.files` for the requested `paths`, and the paths that
/// matched no scored file, each with the reason. Only `recap.files` is ever
/// consulted; no file is read.
///
/// A path is cleaned lexically: `\` is a separator, `.` and empty segments
/// go, and `..` pops a segment but cannot leave the repository. An absolute
/// path is made relative to the repository root, resolving symlinks on both
/// sides first (`/var` is `/private/var` on macOS).
pub fn resolve_paths(
    recap: &PrRecap,
    root: &Path,
    paths: &[String],
) -> (Vec<usize>, Vec<NotFound>) {
    let mut found = BTreeSet::new();
    let mut missing: Vec<NotFound> = Vec::new();
    let mut seen_missing = BTreeSet::new();
    for raw in paths {
        let outcome = match normalize(raw, root) {
            Some(wanted) => match recap.files.iter().position(|file| file.path == wanted) {
                Some(index) => {
                    found.insert(index);
                    continue;
                }
                None => (wanted.clone(), why_missing(recap, &wanted)),
            },
            None => (raw.clone(), "outside the repository".to_string()),
        };
        if seen_missing.insert(outcome.0.clone()) {
            missing.push(NotFound {
                path: raw.clone(),
                reason: outcome.1,
            });
        }
    }
    (found.into_iter().collect(), missing)
}

fn why_missing(recap: &PrRecap, path: &str) -> String {
    if let Some(skipped) = recap.skipped.iter().find(|file| file.path == path) {
        return format!("skipped: {}", skipped.reason);
    }
    if recap.deleted.iter().any(|deleted| deleted == path) {
        return "deleted in this range".to_string();
    }
    "not changed in this range".to_string()
}

/// `raw` as a clean repo-relative `/` path, or `None` when it lies outside
/// the repository.
fn normalize(raw: &str, root: &Path) -> Option<String> {
    let unified = raw.replace('\\', "/");
    let path = Path::new(&unified);
    let relative: PathBuf = if path.is_absolute() {
        relative_to_root(path, root)?
    } else {
        path.to_path_buf()
    };
    let mut parts: Vec<String> = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.join("/"))
}

fn relative_to_root(path: &Path, root: &Path) -> Option<PathBuf> {
    if let Ok(relative) = path.strip_prefix(root) {
        return Some(relative.to_path_buf());
    }
    let root = root.canonicalize().ok()?;
    // The file may be gone at head, so resolve its directory when it is.
    let resolved = path.canonicalize().ok().or_else(|| {
        let parent = path.parent()?.canonicalize().ok()?;
        Some(parent.join(path.file_name()?))
    })?;
    resolved.strip_prefix(&root).ok().map(Path::to_path_buf)
}

pub fn build(recap: &PrRecap, root: &Path, paths: &[String], elapsed_ms: u64) -> Summary {
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
    let omitted = omitted(recap, &shown, &attention, &included);
    let more = (!omitted.is_empty()).then(|| {
        "detail: \"full\" returns the whole topos.pr_recap.v3 document; paths: [..] adds the full \
         records for the named files."
            .to_string()
    });

    let mut scope = serde_json::to_value(&recap.scope).unwrap_or(Value::Null);
    if let Some(object) = scope.as_object_mut() {
        object.insert("elapsed_ms".to_string(), Value::from(elapsed_ms));
    }
    round_floats(&mut scope);
    let rolled = |value: Option<Value>| {
        value.map(|mut value| {
            round_floats(&mut value);
            value
        })
    };

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
        scope,
        project: rolled(recap.project.as_ref().and_then(|project| {
            let mut value = serde_json::to_value(project).ok()?;
            let object = value.as_object_mut()?;
            let lost = object.remove("regression")?;
            object.insert("rollup_lost_pillar".to_string(), lost);
            Some(value)
        })),
        added: rolled(
            recap
                .added
                .as_ref()
                .and_then(|added| serde_json::to_value(added).ok()),
        ),
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

/// What the summary leaves out. A scored file counts as left out only when
/// it has no entry in `attention` or in the requested `files`; a hotspot
/// only when it is not the one shown for an attention file or inside a
/// requested file record.
fn omitted(
    recap: &PrRecap,
    shown: &[&Finding],
    attention: &[AttentionFile],
    included: &[usize],
) -> Omitted {
    let listed: BTreeSet<&str> = attention
        .iter()
        .map(|file| file.path.as_str())
        .chain(
            included
                .iter()
                .map(|&index| recap.files[index].path.as_str()),
        )
        .collect();
    let files = recap
        .files
        .iter()
        .filter(|file| !listed.contains(file.path.as_str()))
        .count();
    let hotspots = recap
        .files
        .iter()
        .enumerate()
        .filter(|(index, _)| !included.contains(index))
        .map(|(_, file)| {
            let shown_one = attention
                .iter()
                .any(|entry| entry.path == file.path && entry.hotspot.is_some());
            file.hotspots.len() - usize::from(shown_one)
        })
        .sum();
    Omitted {
        findings: recap.findings.len() - shown.len(),
        files,
        hotspots,
        clusters: recap.clusters.len(),
        skipped: recap.skipped.len(),
    }
}

fn slim(finding: &Finding) -> SummaryFinding {
    let bound = match (finding.after, finding.limit) {
        (Some(after), Some(limit)) if after.is_finite() && limit.is_finite() => {
            Some(if after < limit { "min" } else { "max" })
        }
        _ => None,
    };
    let [before, after, limit] = round_group([finding.before, finding.after, finding.limit]);
    SummaryFinding {
        gate: finding.gate.key().to_string(),
        severity: finding.severity.as_str(),
        path: finding.path.clone(),
        line: finding.line,
        function: finding.function.clone(),
        pillar: finding.pillar.clone(),
        metric: finding.metric.clone(),
        before,
        after,
        limit,
        bound,
        text: finding.text.clone(),
    }
}

/// One ranking of the files that need attention: files with a block
/// finding, then a warning, then files that are only hotspots; within a
/// tier by finding count, then by the order the gates ranked their first
/// finding. The file `reason` names always leads, whatever its severity.
/// At least [`MAX_ATTENTION`] files, and every file with a block finding.
fn attention(recap: &PrRecap, counting: &[(usize, &Finding)]) -> Vec<AttentionFile> {
    struct Entry<'a> {
        path: &'a str,
        tier: u8,
        count: usize,
        first: usize,
        why: String,
    }
    fn why(finding: &Finding) -> String {
        format!("{} {}", finding.severity.as_str(), finding.gate.key())
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
                why: why(finding),
            }),
        }
    }
    let lead = recap.reason_path.as_deref();
    if let Some(path) = lead {
        if entries.iter().all(|entry| entry.path != path) {
            let top = counting.iter().find(|(_, f)| f.path == path);
            entries.push(Entry {
                path,
                tier: 0,
                count: 0,
                first: top.map_or(usize::MAX, |(index, _)| *index),
                why: top.map_or_else(|| "named in reason".to_string(), |(_, f)| why(f)),
            });
        }
    }
    for file in recap.files.iter().filter(|file| !file.hotspots.is_empty()) {
        if entries.iter().all(|entry| entry.path != file.path) {
            entries.push(Entry {
                path: &file.path,
                tier: 0,
                count: file.hotspots.len(),
                first: usize::MAX,
                why: "hotspot".to_string(),
            });
        }
    }
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
            let file = recap.files.iter().find(|file| file.path == entry.path);
            let hotspot = file
                .and_then(|file| file.hotspots.first())
                .map(|spot| AttentionSpot {
                    function: spot.function.clone(),
                    line: spot.line,
                    metric: spot.metric.clone(),
                    detail: spot.detail.clone(),
                });
            AttentionFile {
                path: entry.path.to_string(),
                severity: match entry.tier {
                    2 => Some("block"),
                    1 => Some("warn"),
                    _ => None,
                },
                findings: if entry.tier == 0 { 0 } else { entry.count },
                why: entry.why,
                hotspots: file.map_or(0, |file| file.hotspots.len()),
                hotspot,
            }
        })
        .collect()
}

impl Summary {
    /// The JSON document, with the requested file records written as they
    /// are in v3. Floats were rounded to three significant figures (more
    /// where three would make two different values in one finding equal)
    /// when the summary was built.
    pub fn to_value(&self) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
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
            "**PR Recap ({})**: `{}` — {}\n`{}...{}` · {} file(s) scored in {} ms · gate {}\n",
            self.check,
            self.readiness.word(),
            clip(&self.reason, 400),
            short(&self.base),
            short(&self.head),
            self.scope["files_scored"],
            self.scope["elapsed_ms"],
            self.gate.preset,
        );
        let direction = serde_json::to_value(self.worst_file_direction)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        out.push_str(&format!(
            "Worst file direction: {direction} (the most severe single file's structural \
             change, independent of readiness)\n"
        ));
        if self.incomplete {
            out.push_str(&format!(
                "Incomplete: {} file(s) over the max_files cap went unscored.\n",
                self.scope["files_capped"]
            ));
        }
        if !self.findings_total.is_empty() {
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
                let mut line = format!("- `{}` ({})", file.path, file.why);
                if file.findings > 1 {
                    line.push_str(&format!(" ×{}", file.findings));
                }
                if let Some(spot) = &file.hotspot {
                    line.push_str(&format!(
                        " · {}:{} {}",
                        spot.function.as_deref().unwrap_or("-"),
                        spot.line,
                        spot.detail
                    ));
                }
                out.push_str(&line);
                out.push('\n');
            }
        }
        for file in &self.files {
            out.push_str(&format!("File {}\n", file_line(file)));
        }
        for missing in &self.paths_not_found {
            out.push_str(&format!(
                "Not found `{}`: {}\n",
                missing.path, missing.reason
            ));
        }
        let omitted = [
            ("findings", self.omitted.findings),
            ("files", self.omitted.files),
            ("hotspots", self.omitted.hotspots),
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

/// One requested file record in a line: pillars, medals, worst function,
/// top hotspots.
fn file_line(file: &FileRecap) -> String {
    let mut parts = vec![format!("`{}`", file.path)];
    let pillars: Vec<String> = file
        .pillars
        .iter()
        .filter_map(|(name, delta)| {
            let (before, after) = (delta.before_score, delta.after_score);
            match (before, after) {
                (Some(before), Some(after)) => {
                    Some(format!("{name} {}→{}", sig3(before), sig3(after)))
                }
                (None, Some(after)) => Some(format!("{name} {}", sig3(after))),
                _ => None,
            }
        })
        .collect();
    if !pillars.is_empty() {
        parts.push(pillars.join(", "));
    }
    let medal = |medal: &Option<super::model::Medal>| {
        medal
            .as_ref()
            .map_or_else(|| "-".to_string(), |m| format!("{} {}", m.symbol, m.tier))
    };
    parts.push(format!(
        "medal {}→{}",
        medal(&file.medal_before),
        medal(&file.medal_after)
    ));
    if let Some(worst) = &file.worst_function_after {
        parts.push(format!(
            "worst {}:{} complexity {}",
            worst.name, worst.line, worst.complexity
        ));
    }
    for spot in file.hotspots.iter().take(2) {
        parts.push(format!(
            "hotspot {}:{} {}",
            spot.metric, spot.line, spot.detail
        ));
    }
    parts.join(" · ")
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
        (Some(before), Some(after)) => format!(" {before}→{after}"),
        (None, Some(after)) => format!(" {after}"),
        _ => String::new(),
    };
    let limit = match (finding.bound, finding.limit) {
        (Some(bound), Some(limit)) => format!(" ({bound} {limit})"),
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

/// `x` to `digits` significant figures.
fn sig(x: f64, digits: usize) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    format!("{:.*e}", digits - 1, x).parse().unwrap_or(x)
}

/// `x` to three significant figures.
pub fn sig3(x: f64) -> f64 {
    sig(x, 3)
}

/// The values of one finding to three significant figures, or to as many
/// as four to six when three would make two different values equal (a
/// `9.999` after against a `10` limit), else as they are.
fn round_group(values: [Option<f64>; 3]) -> [Option<f64>; 3] {
    for digits in 3..=6 {
        let rounded = values.map(|value| value.map(|x| sig(x, digits)));
        let collides = (0..3).any(|i| {
            (i + 1..3).any(|j| match (values[i], values[j], rounded[i], rounded[j]) {
                (Some(a), Some(b), Some(ra), Some(rb)) => a != b && ra == rb,
                _ => false,
            })
        });
        if !collides {
            return rounded;
        }
    }
    values
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
    const TEXT_CEILING_CHARS: usize = 3_000;

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
        let summary = build(&recap, &repo, &[], 7);
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
        let summary = build(&recap, &repo, &[], 7);
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
        let summary = build(&recap, &repo, &[], 7);
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
        let summary = build(&recap, &repo, &[], 7);
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
        let value = build(&recap, &repo, &paths, 7).to_value();
        assert_eq!(value["files"].as_array().unwrap().len(), 1);
        assert_eq!(value["files"][0]["path"], "src/big1.py");
        assert!(value["files"][0].get("medal_after").is_some());
        assert_eq!(value["paths_not_found"][0]["path"], "nope.py");
        assert_eq!(
            value["paths_not_found"][0]["reason"],
            "not changed in this range"
        );

        let abs = repo.join("src/big2.py").display().to_string();
        let (found, missing) = resolve_paths(&recap, &repo, &[abs, "./src/ok.py".to_string()]);
        assert_eq!(found.len(), 2);
        assert!(missing.is_empty());

        let full = full_value(&recap, &repo, &paths);
        assert_eq!(full["schema"], SCHEMA);
        assert_eq!(full["files"].as_array().unwrap().len(), 1);
        assert_eq!(full["paths_not_found"][0]["path"], "nope.py");
        assert_eq!(
            full_value(&recap, &repo, &[]),
            serde_json::to_value(&recap).unwrap()
        );
    }

    #[test]
    fn metrics_describe_the_metrics_shown_and_floats_are_rounded() {
        let (_keep, repo) = fixture(2, 1);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let summary = build(&recap, &repo, &[], 7);
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

    use super::super::gates::Waived;
    use super::super::model::SkippedFile;

    fn paths_of(attention: &[AttentionFile]) -> Vec<&str> {
        attention.iter().map(|a| a.path.as_str()).collect()
    }

    /// One info finding on `path`, taken from a real one.
    fn info_on(recap: &mut PrRecap, path: &str) {
        let mut finding = recap.findings[0].clone();
        finding.severity = Severity::Info;
        finding.path = path.to_string();
        finding.waived = None;
        recap.findings = vec![finding];
    }

    fn mcp_recap(repo: &Path, max_files: usize) -> PrRecap {
        build_recap_named(
            repo,
            "HEAD~1",
            "HEAD",
            max_files,
            "max_files",
            None,
            super::super::tests::no_coupling(),
            &super::super::tests::judging(Priority::Secure),
            &mut (),
        )
        .unwrap()
    }

    #[test]
    fn full_mode_is_the_cli_document_apart_from_the_parameter_name() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        write_files(
            &repo,
            &[
                ("src/--max-files.py", &branchy("a")),
                ("src/b.py", &branchy("b")),
                ("src/c.py", &branchy("c")),
            ],
        );
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

    #[test]
    fn absolute_paths_resolve_through_symlinks_and_normalize() {
        let (_keep, repo) = fixture(3, 1);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let outside = tempfile::tempdir().unwrap();
        let link = outside.path().join("link");
        std::os::unix::fs::symlink(&repo, &link).unwrap();
        let real = repo.canonicalize().unwrap();
        let ask = |root: &Path, path: String| resolve_paths(&recap, root, &[path]);

        for (root, path) in [
            (&repo, link.join("src/big1.py")),
            (&link, repo.join("src/big1.py")),
            (&repo, real.join("src/big1.py")),
            (&real, repo.join("src/big1.py")),
        ] {
            let (found, missing) = ask(root, path.display().to_string());
            assert_eq!((found.len(), missing.len()), (1, 0), "{root:?} {path:?}");
        }
        for raw in [
            "././src/big1.py",
            "src/../src/big1.py",
            "src\\big1.py",
            "./src//big1.py",
        ] {
            let (found, missing) = resolve_paths(&recap, &repo, &[raw.to_string()]);
            assert_eq!((found.len(), missing.len()), (1, 0), "{raw}");
        }
        // Escaping the repository is named, never resolved.
        let (found, missing) = resolve_paths(
            &recap,
            &repo,
            &["../x.py".to_string(), "src/../../x.py".to_string()],
        );
        assert!(found.is_empty());
        assert!(missing.iter().all(|m| m.reason == "outside the repository"));
        let (_, missing) = ask(&repo, "/etc/passwd".to_string());
        assert_eq!(missing[0].reason, "outside the repository");
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
        let asked: Vec<String> = ["nope.py", "./nope.py", "nope.py", "docs/huge.py", "old.py"]
            .map(String::from)
            .to_vec();
        let (found, missing) = resolve_paths(&recap, &repo, &asked);
        assert!(found.is_empty());
        let reasons: Vec<(&str, &str)> = missing
            .iter()
            .map(|m| (m.path.as_str(), m.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            vec![
                ("nope.py", "not changed in this range"),
                ("docs/huge.py", "skipped: over the size limit"),
                ("old.py", "deleted in this range"),
            ]
        );
    }

    #[test]
    fn attention_leads_with_the_reason_file_whatever_made_it_the_reason() {
        let (_keep, repo) = fixture(8, 0);
        let base = recap(&repo, "HEAD~1", "HEAD", 0);
        let last = base.files.last().unwrap().path.clone();

        // Info finding (a sub-materiality score drop) is the reason.
        let mut info = base.clone();
        info_on(&mut info, &last);
        info.reason_path = Some(last.clone());
        let summary = build(&info, &repo, &[], 0);
        assert_eq!(summary.attention[0].path, last);
        assert!(summary.attention[0].why.starts_with("info"));
        assert!(summary.findings.is_empty());
        assert_eq!(summary.findings_total["info"], 1);

        // Every finding waived: the reason came from the direction text.
        let mut waived = base.clone();
        for finding in &mut waived.findings {
            finding.waived = Some(Waived {
                reason: "r".into(),
                source: "s".into(),
                expires: None,
            });
        }
        waived.reason_path = Some(last.clone());
        let summary = build(&waived, &repo, &[], 0);
        assert_eq!(summary.attention[0].path, last);
        assert!(summary.findings.is_empty());
        assert_eq!(summary.findings_total["waived"], waived.findings.len());

        // No findings at all.
        let mut none = base.clone();
        none.findings.clear();
        none.reason_path = Some(last.clone());
        let summary = build(&none, &repo, &[], 0);
        assert_eq!(summary.attention[0].path, last);
        assert_eq!(summary.attention[0].why, "named in reason");

        // The recap itself carries it: the lead finding's file.
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
            assert_eq!(build(&recap, &repo, &[], 0).attention[0].path, "a.py");
        }
    }

    #[test]
    fn hotspots_carry_their_measure_and_are_counted() {
        let (_keep, repo) = fixture(8, 0);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let summary = build(&recap, &repo, &[], 0);
        let spot = summary
            .attention
            .iter()
            .find_map(|a| a.hotspot.as_ref())
            .expect("a hotspot");
        assert!(spot.detail.contains("gate is"), "{}", spot.detail);
        assert!(summary
            .attention
            .iter()
            .all(|a| a.hotspots >= a.hotspot.is_some() as usize));
        let total: usize = recap.files.iter().map(|f| f.hotspots.len()).sum();
        let shown = summary
            .attention
            .iter()
            .filter(|a| a.hotspot.is_some())
            .count();
        assert!(total > shown, "fixture should leave hotspots out");
        assert_eq!(summary.omitted.hotspots, total - shown);
        assert_eq!(summary.to_value()["omitted"]["hotspots"], total - shown);
    }

    #[test]
    fn omitted_files_excludes_files_shown_in_attention_or_files() {
        let (_keep, repo) = fixture(12, 2);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let asked = ["src/big11.py".to_string()];
        let summary = build(&recap, &repo, &asked, 0);
        let mut listed: BTreeSet<&str> = paths_of(&summary.attention).into_iter().collect();
        listed.insert("src/big11.py");
        assert_eq!(summary.omitted.files, recap.files.len() - listed.len());
        assert_eq!(summary.files.len(), 1);
    }

    #[test]
    fn rounding_never_makes_different_values_equal() {
        let [before, after, limit] = round_group([None, Some(9.9996), Some(10.0)]);
        assert_eq!((before, after, limit), (None, Some(9.9996), Some(10.0)));
        let [_, after, limit] = round_group([None, Some(9.99), Some(10.0)]);
        assert_eq!((after, limit), (Some(9.99), Some(10.0)));
        let [a, b, _] = round_group([Some(12.3456), Some(7.0), None]);
        assert_eq!((a, b), (Some(12.3), Some(7.0)));
        let [a, ..] = round_group([Some(1.0000001), Some(1.0), None]);
        assert_eq!(a, Some(1.0000001));

        let (_keep, repo) = fixture(1, 1);
        let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let finding = &mut recap.findings[0];
        finding.before = Some(11.0);
        finding.after = Some(9.9996);
        finding.limit = Some(10.0);
        let slim = slim(finding);
        assert_eq!((slim.after, slim.limit), (Some(9.9996), Some(10.0)));
        assert_eq!(slim.bound, Some("min"));
    }

    #[test]
    fn nan_values_are_null_with_no_bound() {
        let (_keep, repo) = fixture(1, 1);
        let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
        recap.findings[0].after = Some(f64::NAN);
        recap.findings[0].limit = Some(10.0);
        let summary = build(&recap, &repo, &[], 0);
        let value = summary.to_value();
        assert_eq!(summary.findings[0].bound, None);
        assert!(value["findings"][0]["after"].is_null());
        assert!(value["findings"][0].get("bound").is_none());
        assert!(!summary.to_markdown().is_empty());
    }

    #[test]
    fn text_covers_info_only_incomplete_direction_and_paths() {
        let (_keep, repo) = fixture(3, 1);
        let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let last = recap.files[0].path.clone();
        info_on(&mut recap, &last);
        recap.reason_path = Some(last);
        let text = build(&recap, &repo, &[], 0).to_markdown();
        assert!(text.contains("Findings (info 1):"), "{text}");
        assert!(text.contains("Worst file direction:"), "{text}");
        assert!(text.contains("independent of readiness"), "{text}");
        assert!(!text.contains("Incomplete:"));

        recap.incomplete = true;
        recap.scope.files_capped = 4;
        recap.reason = "x".repeat(900);
        let text = build(&recap, &repo, &[], 0).to_markdown();
        assert!(
            text.contains("Incomplete: 4 file(s) over the max_files cap"),
            "{text}"
        );
        assert!(text.contains('…'));

        let asked = ["src/big1.py".to_string(), "nope.py".to_string()];
        let text = build(&recap, &repo, &asked, 0).to_markdown();
        assert!(text.contains("File `src/big1.py`"), "{text}");
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
    fn summary_reports_scoring_time_and_a_timeout_names_the_work() {
        let (_keep, repo) = fixture(1, 1);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        assert_eq!(
            build(&recap, &repo, &[], 1234).to_value()["scope"]["elapsed_ms"],
            1234
        );

        let timed_out = McpError::internal_error("Operation timed out after 300s", None);
        let error = timeout_error(timed_out.clone(), 812);
        assert!(error.message.contains("while scoring 812 file(s)"));
        assert!(error.message.contains("`max_files`") && error.message.contains("`paths`"));
        assert!(timeout_error(timed_out, 0)
            .message
            .contains("before scoring began"));
        let other = McpError::internal_error("Request cancelled", None);
        assert_eq!(timeout_error(other, 5).message, "Request cancelled");
    }
}
