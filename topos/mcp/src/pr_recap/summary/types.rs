//! The documents `topos.pr_recap.summary.v1` is made of.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;

use crate::shaping::is_zero;

use super::super::gates::Readiness;
use super::super::model::{AddedRollup, FileRecap, Headline, Medal, PillarRollup, Scope};
use super::paths::NotFound;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SummaryGate {
    pub preset: &'static str,
    pub fail_on: &'static str,
    pub changes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// The `.topos.toml` consulted, as an absolute path.
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SummaryFinding {
    pub gate: String,
    pub severity: &'static str,
    /// Absolute; empty for a range-wide finding.
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
    /// `min` when `after` is below `limit`, `max` when above; absent when
    /// they are equal or there is no limit. Decided on the unrounded values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound: Option<&'static str>,
    pub text: String,
}

/// A file's top hotspot span and what was measured there.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AttentionSpot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    pub line: usize,
    pub metric: String,
    /// The measured value against its gate, as in v3 (`pick complexity is
    /// 23, gate is 10`).
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AttentionFile {
    /// Absolute.
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

/// Counts of what the summary leaves out; zero counts are not written.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
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

impl Omitted {
    pub fn is_empty(&self) -> bool {
        self.counts().iter().all(|(_, count)| *count == 0)
    }

    /// Each count with its name, in the order the text lists them.
    pub fn counts(&self) -> [(&'static str, usize); 5] {
        [
            ("findings", self.findings),
            ("files", self.files),
            ("hotspots", self.hotspots),
            ("clusters", self.clusters),
            ("skipped", self.skipped),
        ]
    }
}

/// v3 `scope`, plus how long the run took.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SummaryScope {
    #[serde(flatten)]
    pub counts: Scope,
    /// Wall time of the run up to this response: preparing graphs, scoring
    /// files, the gates and clusters. Not the time to serialize this.
    pub elapsed_ms: u64,
    /// The part of `elapsed_ms` spent scoring files; absent when none was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoring_ms: Option<u64>,
}

/// v3 `project`: the rollup over the touched set. `rollup_lost_pillar` is
/// v3 `project.regression`, renamed because the worst file can regress
/// without the rollup losing a pillar.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SummaryProject {
    pub medal_before: Medal,
    pub medal_after: Medal,
    pub pillars: BTreeMap<String, PillarRollup>,
    /// The rollup over the touched set lost a pillar it passed at base.
    pub rollup_lost_pillar: bool,
    pub files_before: usize,
    pub files_after: usize,
    pub decisions_before: Option<usize>,
    pub decisions_after: Option<usize>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Summary {
    pub schema: &'static str,
    /// Absolute repository root. File paths in the structured fields below
    /// are absolute under it; range-wide findings carry no path, and prose
    /// (`reason`, finding `text`) keeps repo-relative paths.
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
    /// The structural change of the most severe single file, by that file's
    /// own status and independent of readiness: it can be
    /// `SUSPICIOUS_NO_STRUCTURAL_CHANGE` on a passing PR. v3 `direction` is
    /// this unless the files disagree on an improvement. Absent when no file
    /// was compared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worst_file_direction: Option<Headline>,
    /// The file `worst_file_direction` describes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worst_file: Option<String>,
    pub scope: SummaryScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<SummaryProject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<AddedRollup>,
    pub findings: Vec<SummaryFinding>,
    /// Counting findings by severity, and how many a waiver covers.
    pub findings_total: BTreeMap<&'static str, usize>,
    pub attention: Vec<AttentionFile>,
    /// What each metric shown measures.
    pub metrics: BTreeMap<String, &'static str>,
    #[serde(default, skip_serializing_if = "Omitted::is_empty")]
    pub omitted: Omitted,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths_not_found: Vec<NotFound>,
    pub non_claim: &'static str,
    /// The v3 records of the requested `paths`, with absolute paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileRecap>,
}
