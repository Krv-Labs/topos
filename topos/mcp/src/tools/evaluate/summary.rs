//! `topos.evaluate_project.summary.v1`: the MCP default shape of a project
//! evaluation.
//!
//! The full result carries one row per file, ~1.1K characters each. This is
//! a distinct struct built from the computed [`ProjectEvaluationResult`]:
//! the rollup, the five worst files with what binds each, and counts in
//! place of the `hard_fails` and `maintainability_giants` lists. What it
//! leaves out is counted in `omitted`, with the parameters that widen it in
//! `more`. Same conventions as `topos.pr_recap.summary.v1`: facts and
//! handles, absolute paths, and measured values rounded far enough apart
//! that a value and its limit never read as equal.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;

use crate::schemas::{
    AgentContract, BindingConstraint, LatticeElement, PrioritySource, ProjectEvaluationResult,
    ProjectLanguageRollup, WorstFileEntry,
};
use crate::shaping;

pub(crate) const SUMMARY_SCHEMA: &str = "topos.evaluate_project.summary.v1";

/// Files in `worst`, and in `worst_files` of the full result.
pub(crate) const WORST_LEN: usize = 5;

/// The order of `worst` (and of `files` in the full result).
const RANKED_BY: &str = "failures (failed gates or a parse failure, most first), then lowest \
                         score, then fewest pillars held";

/// Characters of one warning in the text block.
const TEXT_WARNING_CHARS: usize = 300;

/// What binds a file: `topos_evaluate_file`'s `binding_constraint` for it,
/// with the value rounded apart from its limit and the side it is on.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(crate) struct SummaryBinding {
    pub pillar: String,
    /// Raw-metric key, or the dangerous callee for a SECURE finding.
    pub metric: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// The bound the value must satisfy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    /// `min` when `value` is below `limit`, else `max`; only with both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound: Option<&'static str>,
    /// Offending symbol, or `<module>` for a whole-file metric.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<usize>,
}

/// One file of the ranking, with why it ranks there and what binds it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(crate) struct SummaryWorst {
    /// Absolute: pass it straight to `topos_evaluate_file`.
    pub path: String,
    pub verdict: LatticeElement,
    /// The file's lowest-scoring pillar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pillar: Option<String>,
    /// The pillar's score in percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Gates the file fails (all pillars).
    #[serde(skip_serializing_if = "shaping::is_zero")]
    pub gates_failed: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub parse_failure: bool,
    /// The same constraint `topos_evaluate_file` reports for this file;
    /// `null` when it would report none.
    pub binding_constraint: Option<SummaryBinding>,
}

/// Counts of what the summary leaves out.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub(crate) struct Omitted {
    /// Scored files not named in `worst`.
    pub files: usize,
}

impl Omitted {
    fn is_empty(&self) -> bool {
        self.files == 0
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(crate) struct Summary {
    pub schema: &'static str,
    /// Absolute scanned directory. Absent on an error for a path that is a
    /// file or outside the file root.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub root: String,
    /// Source files found.
    pub file_count: usize,
    pub parse_failures: usize,
    /// Found but not evaluated (unreadable); counted in `failing_files`, and
    /// in no rollup.
    #[serde(skip_serializing_if = "shaping::is_zero")]
    pub unread_files: usize,
    /// Milliseconds spent on the scan: walk, dependency graph, scoring.
    /// Absent on an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_ms: Option<u64>,
    pub priority: String,
    pub priority_source: PrioritySource,
    pub coupling_available: bool,
    /// The lattice element of the pillars every scored file holds. Absent on
    /// an error, when nothing was scanned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate_floor_verdict: Option<LatticeElement>,
    /// The pillars every scored file holds, which are the ones in
    /// `aggregate_floor_verdict`; empty when no pillar holds for every file.
    /// Absent on an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_by_every_file: Option<Vec<String>>,
    /// Per dimension: the generator's value when every scored file
    /// satisfies it, else `SLOP`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub rolled_up_dimensions: BTreeMap<String, LatticeElement>,
    /// Per dimension: the lowest file score in percent.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub rolled_up_scores: BTreeMap<String, f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub language_rollups: Vec<ProjectLanguageRollup>,
    /// Files that fail a gate, do not parse, or could not be read
    /// (`hard_fails` in full, plus the unread files).
    pub failing_files: usize,
    /// Giants: files whose advisory cyclomatic complexity is above the
    /// SIMPLE band while every gate passes (`maintainability_giants` in
    /// full). A watch list, not failures.
    pub giants: usize,
    /// How `worst` is ordered.
    pub ranked_by: &'static str,
    /// The worst files, worst first.
    pub worst: Vec<SummaryWorst>,
    /// What each metric shown in `worst` measures.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub metrics: BTreeMap<String, &'static str>,
    #[serde(skip_serializing_if = "Omitted::is_empty")]
    pub omitted: Omitted,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
    /// Estimated tokens (characters / 4) of the whole `detail: "full"`
    /// response, structured content and text block, for the requested
    /// `limit`/`offset`.
    #[serde(skip_serializing_if = "shaping::is_zero")]
    pub full_response_tokens_estimate: usize,
    pub warnings: Vec<String>,
    pub agent_contract: Option<AgentContract>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn summary_binding(binding: BindingConstraint) -> SummaryBinding {
    let BindingConstraint {
        pillar,
        metric,
        value,
        threshold,
        symbol,
        line_start,
        line_end,
    } = binding;
    let (bound, value, limit) = match (value, threshold) {
        (Some(value), Some(limit)) => {
            let side = shaping::bound(value, limit);
            match shaping::round_apart(&[value, limit])[..] {
                [value, limit] => (side, Some(value), Some(limit)),
                _ => (side, Some(value), Some(limit)),
            }
        }
        (value, limit) => (None, value.map(shaping::sig3), limit.map(shaping::sig3)),
    };
    SummaryBinding {
        pillar,
        metric,
        value,
        limit,
        bound,
        symbol,
        line_start,
        line_end,
    }
}

fn worst_row(worst: &WorstFileEntry) -> SummaryWorst {
    let WorstFileEntry {
        filepath,
        lattice_element,
        pillar,
        score,
        gates_failed,
        parse_failure,
        binding_constraint,
    } = worst.clone();
    SummaryWorst {
        path: filepath,
        verdict: lattice_element,
        pillar,
        score: score.map(shaping::sig3),
        gates_failed,
        parse_failure,
        binding_constraint: binding_constraint.flatten().map(summary_binding),
    }
}

/// Says when the requested `offset` is past the last scored file: the fact
/// both the summary and the full text state.
pub(super) fn offset_past_end(result: &ProjectEvaluationResult) -> Option<String> {
    if result.total == 0 || result.offset < result.total {
        return None;
    }
    let scored = if result.unread_files > 0 {
        "scored "
    } else {
        ""
    };
    Some(format!(
        "offset {} is past the last of {} {scored}files",
        result.offset, result.total
    ))
}

/// What `more` says: the whole-response estimate at the requested `limit`,
/// and when `offset` is past the last file.
fn more_text(result: &ProjectEvaluationResult, limit: usize) -> String {
    let past_end = offset_past_end(result)
        .map(|note| format!(", but {note}"))
        .unwrap_or_default();
    format!(
        "detail: \"full\" returns per-file rows, paged by limit/offset (~{} tokens for the whole \
         response at limit {limit}{past_end}); a narrower path scores fewer files; \
         topos_evaluate_file scores one file.",
        result.response_tokens_estimate
    )
}

/// The pillars every scored file holds, or `None` when nothing was scanned.
fn held_by_every_file(result: &ProjectEvaluationResult) -> Option<Vec<String>> {
    result.aggregate_floor_verdict?;
    Some(
        result
            .rolled_up_dimensions
            .iter()
            .filter(|(_, verdict)| **verdict != LatticeElement::SLOP)
            .map(|(dim, _)| dim.clone())
            .collect(),
    )
}

/// The summary of `result`, with the estimate for the page of `limit` rows
/// that was requested.
pub(crate) fn build(result: &ProjectEvaluationResult, limit: usize) -> Summary {
    let worst: Vec<SummaryWorst> = result.worst_files.iter().map(worst_row).collect();
    let metrics = shaping::describe_metrics(
        worst
            .iter()
            .filter_map(|w| w.binding_constraint.as_ref())
            .map(|binding| binding.metric.as_str()),
    );
    let omitted = Omitted {
        files: result.total.saturating_sub(worst.len()),
    };
    let more = (!omitted.is_empty() || offset_past_end(result).is_some())
        .then(|| more_text(result, limit));
    Summary {
        schema: SUMMARY_SCHEMA,
        root: result.root.clone(),
        file_count: result.file_count,
        parse_failures: result.parse_failures,
        unread_files: result.unread_files,
        scan_ms: result.scan_ms,
        priority: result.priority.clone(),
        priority_source: result.priority_source,
        coupling_available: result.coupling_available,
        aggregate_floor_verdict: result.aggregate_floor_verdict,
        held_by_every_file: held_by_every_file(result),
        rolled_up_dimensions: result.rolled_up_dimensions.clone(),
        rolled_up_scores: result.rolled_up_scores.clone(),
        language_rollups: result.language_rollups.clone(),
        failing_files: result.hard_fails.len() + result.unread_files,
        giants: result.maintainability_giants.len(),
        ranked_by: RANKED_BY,
        worst,
        metrics,
        omitted,
        more,
        full_response_tokens_estimate: result.response_tokens_estimate,
        warnings: result.warnings.clone(),
        agent_contract: result.agent_contract.clone(),
        error: result.error.clone(),
    }
}

impl Summary {
    /// A compact markdown rendering for clients that show only the text
    /// block.
    pub(crate) fn to_markdown(&self) -> String {
        if let Some(error) = &self.error {
            let path = if self.root.is_empty() {
                String::new()
            } else {
                format!("Path: `{}`\n", self.root)
            };
            return format!("**Error:** {error}\n{path}");
        }
        let mut out = format!("{}{}", self.headline(), self.counts_lines());
        out.push_str(&self.rollup_line());
        out.push_str(&self.worst_lines());
        let counts = [("files", self.omitted.files)];
        if let Some(line) = shaping::omitted_line(&counts, self.more.as_deref()) {
            out.push_str(&format!("{line}\n"));
        }
        out.push_str(&self.warning_lines());
        out
    }

    /// Every warning, one line each (continuation lines are joined) and
    /// each clipped to [`TEXT_WARNING_CHARS`]; the full text is in
    /// `structuredContent.warnings`.
    fn warning_lines(&self) -> String {
        self.warnings
            .iter()
            .map(|warning| {
                let one_line = warning.split_whitespace().collect::<Vec<_>>().join(" ");
                format!(
                    "Warning: {}\n",
                    shaping::clip(&one_line, TEXT_WARNING_CHARS)
                )
            })
            .collect()
    }

    fn headline(&self) -> String {
        let floor = self
            .aggregate_floor_verdict
            .map_or("none", LatticeElement::as_str);
        let held = match self.held_by_every_file.as_deref() {
            Some([]) | None => "no pillar".to_string(),
            Some(held) => held.join(", "),
        };
        let unread = if self.unread_files > 0 {
            format!("; {} unreadable not counted", self.unread_files)
        } else {
            String::new()
        };
        format!(
            "**Project evaluation** `{}`: floor `{floor}` (the pillars every scored file holds: \
             {held}{unread}), priority {}\n",
            self.root, self.priority
        )
    }

    fn counts_lines(&self) -> String {
        let scored = self.omitted.files + self.worst.len();
        let unread = if self.unread_files > 0 {
            format!(", {} unreadable", self.unread_files)
        } else {
            String::new()
        };
        let scan = self
            .scan_ms
            .map(|ms| format!("; scan {}", duration(ms)))
            .unwrap_or_default();
        format!(
            "{} file(s) found, {scored} scored{unread}{scan} · parse failures {}\n\
             failing {} (a failed gate, a parse failure or unreadable) · giants {} (advisory \
             cyclomatic above the SIMPLE band, every gate passing)\n",
            self.file_count, self.parse_failures, self.failing_files, self.giants
        )
    }

    fn rollup_line(&self) -> String {
        if self.rolled_up_dimensions.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = self
            .rolled_up_dimensions
            .iter()
            .map(|(dim, verdict)| {
                let held = if *verdict == LatticeElement::SLOP {
                    "not held"
                } else {
                    "held"
                };
                let score = self
                    .rolled_up_scores
                    .get(dim)
                    .map(|s| format!(" {}%", shaping::sig3(*s)))
                    .unwrap_or_default();
                format!("{dim} {held}{score}")
            })
            .collect();
        format!(
            "Rollup (every scored file / lowest score): {}\n",
            parts.join(", ")
        )
    }

    fn worst_lines(&self) -> String {
        if self.worst.is_empty() {
            return String::new();
        }
        let mut out = format!("Worst, ranked by {}:\n", self.ranked_by);
        for file in &self.worst {
            out.push_str(&format!("- {}\n", worst_line(file)));
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
        line.push_str(&format!(" · {pillar} {score}%"));
    }
    if file.gates_failed > 1 {
        line.push_str(&format!(" · {} gates failed", file.gates_failed));
    }
    if let Some(binding) = &file.binding_constraint {
        line.push_str(&format!(" · binding: {}", binding_text(binding)));
    }
    line
}

fn binding_text(binding: &SummaryBinding) -> String {
    let mut text = binding.metric.clone();
    if let Some(value) = binding.value {
        text.push_str(&format!(" {value}"));
        match (binding.bound, binding.limit) {
            (Some(bound), Some(limit)) => text.push_str(&format!(" ({bound} {limit})")),
            (None, Some(limit)) => text.push_str(&format!(" (limit {limit})")),
            _ => {}
        }
    }
    if let Some(symbol) = &binding.symbol {
        let line = binding
            .line_start
            .map(|line| format!(":{line}"))
            .unwrap_or_default();
        text.push_str(&format!(" in {symbol}{line}"));
    }
    text
}

fn duration(ms: u64) -> String {
    if ms >= 1_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        format!("{ms}ms")
    }
}
