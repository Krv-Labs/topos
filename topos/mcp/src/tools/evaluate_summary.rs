//! `topos.evaluate_project.summary.v1`: the MCP default shape of a project
//! evaluation.
//!
//! The full result carries one row per file, ~1.1K characters each. This is
//! a distinct struct built from the computed [`ProjectEvaluationResult`]:
//! the rollup, the five worst files with the metric that put each there, and
//! counts in place of the `hard_fails` and `maintainability_giants` lists.
//! What it leaves out is counted in `omitted`, with the parameters that
//! widen it in `more`. Same conventions as `topos.pr_recap.summary.v1`:
//! facts and handles, floats to three significant figures, absolute paths.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::pr_recap::gates::metric_description;
use crate::shaping::{round_floats, sig3};
use crate::schemas::{
    AgentContract, LatticeElement, PrioritySource, ProjectEvaluationResult, ProjectFileEntry,
    ProjectLanguageRollup, WorstFileEntry,
};

pub const SUMMARY_SCHEMA: &str = "topos.evaluate_project.summary.v1";

/// Files in `worst`, and in `worst_files` of the full result.
pub const WORST_LEN: usize = 5;
/// Above this many files, `warnings` lists where the files are.
pub const LARGE_TREE_FILES: usize = 1_000;
/// Directories named in the large-tree warning.
const LARGE_TREE_DIRS: usize = 5;

/// One file of the ranking, with the gate that put it there.
#[derive(Debug, Clone, Serialize)]
pub struct SummaryWorst {
    /// Absolute: pass it straight to `topos_evaluate_file`.
    pub path: String,
    pub verdict: LatticeElement,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pillar: Option<String>,
    /// The pillar's score in percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// The failing gate's metric; absent when the file fails no gate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    /// `min` when `value` is below `limit`, else `max`; only with a limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound: Option<&'static str>,
    /// Gates the file fails (all pillars).
    #[serde(skip_serializing_if = "is_zero")]
    pub gates_failed: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub parse_failure: bool,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Counts of what the summary leaves out.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Omitted {
    /// Scored files not named in `worst`.
    pub files: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub schema: &'static str,
    /// Absolute scanned directory.
    pub root: String,
    pub file_count: usize,
    pub parse_failures: usize,
    pub scan_ms: u64,
    pub priority: String,
    pub priority_source: PrioritySource,
    pub coupling_available: bool,
    /// Per dimension: the generator's value when every file satisfies it,
    /// else `SLOP`.
    pub rolled_up_dimensions: BTreeMap<String, LatticeElement>,
    /// Per dimension: the lowest file score in percent.
    pub rolled_up_scores: BTreeMap<String, f64>,
    /// The lattice element of the dimensions every file satisfies.
    pub aggregate_floor_verdict: LatticeElement,
    pub language_rollups: Vec<ProjectLanguageRollup>,
    /// Files that fail a gate or do not parse (`hard_fails` in full).
    pub failing_files: usize,
    /// Files with advisory cyclomatic above the SIMPLE band and every gate
    /// passing (`maintainability_giants` in full).
    pub giants: usize,
    /// The worst files, worst first.
    pub worst: Vec<SummaryWorst>,
    /// What each metric shown in `worst` measures.
    pub metrics: BTreeMap<String, &'static str>,
    #[serde(skip_serializing_if = "Omitted::is_empty")]
    pub omitted: Omitted,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
    /// Estimated tokens (serialized characters / 4) of the `files` page
    /// that `detail: "full"` returns for the requested `limit`/`offset`.
    pub full_page_tokens_estimate: usize,
    pub warnings: Vec<String>,
    pub agent_contract: Option<AgentContract>,
}

impl Omitted {
    fn is_empty(&self) -> bool {
        self.files == 0
    }
}

pub fn absolute(root: &str, relative: &str) -> String {
    Path::new(root).join(relative).to_string_lossy().to_string()
}

fn worst_row(root: &str, worst: &WorstFileEntry) -> SummaryWorst {
    SummaryWorst {
        path: absolute(root, &worst.filepath),
        verdict: worst.lattice_element,
        pillar: worst.pillar.clone(),
        score: worst.score,
        metric: worst.metric.clone(),
        value: worst.value,
        limit: worst.limit,
        bound: worst.bound,
        gates_failed: worst.gates_failed,
        parse_failure: worst.parse_failure,
    }
}

pub fn build(result: &ProjectEvaluationResult, limit: usize) -> Summary {
    let root = result.root.as_str();
    let worst: Vec<SummaryWorst> = result
        .worst_files
        .iter()
        .map(|entry| worst_row(root, entry))
        .collect();
    let mut metrics = BTreeMap::new();
    for metric in worst.iter().filter_map(|w| w.metric.as_deref()) {
        if let Some(description) = metric_description(metric) {
            metrics.insert(metric.to_string(), description);
        }
    }
    let omitted = Omitted {
        files: result.total.saturating_sub(worst.len()),
    };
    let more = (!omitted.is_empty()).then(|| {
        format!(
            "detail: \"full\" returns per-file rows, paged by limit/offset (~{} tokens at \
             limit {limit}); a narrower path scores fewer files; topos_evaluate_file scores \
             one file.",
            result.page_tokens_estimate
        )
    });
    Summary {
        schema: SUMMARY_SCHEMA,
        root: result.root.clone(),
        file_count: result.file_count,
        parse_failures: result.parse_failures,
        scan_ms: result.scan_ms,
        priority: result.priority.clone(),
        priority_source: result.priority_source,
        coupling_available: result.coupling_available,
        rolled_up_dimensions: result.rolled_up_dimensions.clone(),
        rolled_up_scores: result.rolled_up_scores.clone(),
        aggregate_floor_verdict: result.aggregate_floor_verdict,
        language_rollups: result
            .language_rollups
            .iter()
            .map(|rollup| ProjectLanguageRollup {
                worst_file_path: rollup.worst_file_path.as_deref().map(|p| absolute(root, p)),
                ..rollup.clone()
            })
            .collect(),
        failing_files: result.hard_fails.len(),
        giants: result.maintainability_giants.len(),
        worst,
        metrics,
        omitted,
        more,
        full_page_tokens_estimate: result.page_tokens_estimate,
        warnings: result.warnings.clone(),
        agent_contract: result.agent_contract.clone(),
    }
}

impl Summary {
    /// The JSON document, floats to three significant figures.
    pub fn to_value(&self) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        round_floats(&mut value);
        value
    }

    /// A compact markdown rendering for clients that show only the text
    /// block.
    pub fn to_markdown(&self) -> String {
        let mut out = format!(
            "**Project evaluation** `{}`: `{}` (priority {})\n{} file(s) scored in {} · parse \
             failures {} · failing {} · giants {}\n",
            self.root,
            self.aggregate_floor_verdict.as_str(),
            self.priority,
            self.file_count,
            duration(self.scan_ms),
            self.parse_failures,
            self.failing_files,
            self.giants,
        );
        let rollup: Vec<String> = self
            .rolled_up_dimensions
            .iter()
            .map(|(dim, verdict)| {
                let score = self
                    .rolled_up_scores
                    .get(dim)
                    .map(|s| format!(" {}%", sig3(*s)))
                    .unwrap_or_default();
                let held = if *verdict == LatticeElement::SLOP {
                    "not held"
                } else {
                    "held"
                };
                format!("{dim} {held}{score}")
            })
            .collect();
        if !rollup.is_empty() {
            out.push_str(&format!(
                "Rollup (every file / lowest score): {}\n",
                rollup.join(", ")
            ));
        }
        if !self.worst.is_empty() {
            out.push_str("Worst:\n");
            for file in &self.worst {
                out.push_str(&format!("- {}\n", worst_line(file)));
            }
        }
        for warning in self.warnings.iter().take(2) {
            out.push_str(&format!("Warning: {}\n", clip(warning, 300)));
        }
        if let Some(more) = &self.more {
            out.push_str(&format!(
                "Omitted: {} file(s). {more}\n",
                self.omitted.files
            ));
        }
        out
    }
}

fn worst_line(file: &SummaryWorst) -> String {
    let mut line = format!("`{}` {}", file.path, file.verdict.as_str());
    if file.parse_failure {
        line.push_str(" · did not parse");
    }
    if let (Some(pillar), Some(score)) = (&file.pillar, file.score) {
        line.push_str(&format!(" · {pillar} {}%", sig3(score)));
    }
    if let (Some(metric), Some(value)) = (&file.metric, file.value) {
        line.push_str(&format!(" · {metric} {}", sig3(value)));
        if let (Some(bound), Some(limit)) = (file.bound, file.limit) {
            line.push_str(&format!(" ({bound} {})", sig3(limit)));
        }
    }
    if file.gates_failed > 1 {
        line.push_str(&format!(" · {} gates failed", file.gates_failed));
    }
    line
}

fn duration(ms: u64) -> String {
    if ms >= 1_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        format!("{ms}ms")
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{}…", kept.trim_end())
}

/// Estimated tokens of `rows` as JSON: serialized characters / 4, the
/// heuristic of the context-budget ratchet.
pub fn estimate_tokens(rows: &[ProjectFileEntry]) -> usize {
    serde_json::to_string(rows).map_or(0, |json| json.len() / 4)
}

/// For a very large tree, a fact for `warnings`: the file count and where
/// the files are, so a narrower `path` can be chosen.
pub fn large_tree_warning(file_count: usize, entries: &[ProjectFileEntry]) -> Option<String> {
    if file_count <= LARGE_TREE_FILES {
        return None;
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in entries {
        let mut parts = Path::new(&entry.filepath).components();
        let top = match (parts.next(), parts.next()) {
            (Some(dir), Some(_)) => dir.as_os_str().to_string_lossy().to_string(),
            _ => ".".to_string(),
        };
        *counts.entry(top).or_default() += 1;
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let listed: Vec<String> = ranked
        .iter()
        .take(LARGE_TREE_DIRS)
        .map(|(dir, count)| format!("{dir} {count}"))
        .collect();
    Some(format!(
        "{file_count} files scanned; top-level directories by file count: {}. `path` can name \
         one of them.",
        listed.join(", ")
    ))
}
