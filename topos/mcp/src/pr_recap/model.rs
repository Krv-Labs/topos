//! `topos pr-recap` document model, schema `topos.pr_recap.v3`.
//!
//! One struct tree feeds `--json`, the terminal card, the compact CI card
//! and the GitHub comment. Renderers read this and never recompute a
//! verdict: every mark on a card is a field here, produced by the data
//! builder in `pr_recap.rs` from the lattice, the UAST ledger and the two
//! coupling graphs, and judged by the configured gates in `gates.rs`.

use std::collections::BTreeMap;

use serde::Serialize;
use topos_engine::config::Severity;
use topos_engine::functors::profunctors::uast::ledger::{Ledger, MatchKind};
use topos_engine::graphs::mdg::split::{NewSymbol, Reach, SymbolMove};

use super::gates::{optional_severity_name, Finding, Readiness};

pub const SCHEMA: &str = "topos.pr_recap.v3";

/// Below this normalized AST distance a file is "structurally unchanged".
pub const STRUCTURAL_CHANGE_THRESHOLD: f64 = 0.02;
/// A score move at least this large (0–1 scale) with no structural change is cosmetic.
pub const MEANINGFUL_SCORE_DELTA: f64 = 0.03;
/// Cluster decision growth above this fraction earns a `!` mark.
pub const CLUSTER_GROWTH_WARN: f64 = 0.10;

/// Which way the structure moved. Descriptive only: whether the change is
/// ready is [`Readiness`], decided by the configured gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Headline {
    SuspiciousNoStructuralChange,
    Regression,
    RegressionScore,
    Improvement,
    ImprovementScore,
    LateralMove,
}

impl Headline {
    /// Worst-first rank used when one file decides the PR direction.
    pub fn rank(self) -> u8 {
        match self {
            Headline::SuspiciousNoStructuralChange => 0,
            Headline::Regression => 1,
            Headline::RegressionScore => 2,
            Headline::LateralMove => 3,
            Headline::ImprovementScore => 4,
            Headline::Improvement => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileChange {
    Added,
    Modified,
    Renamed,
}

/// A lattice verdict as the card shows it: `🥈 SILVER · SECURE_NAVIGABLE`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Medal {
    pub symbol: String,
    pub tier: String,
    pub verdict: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PillarDelta {
    pub measured: bool,
    pub before_passed: Option<bool>,
    pub after_passed: Option<bool>,
    /// Displayed 0–100 scale, one decimal.
    pub before_score: Option<f64>,
    pub after_score: Option<f64>,
    /// The gate that failed, when a previously passing pillar no longer does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lost_gate: Option<String>,
    /// The gate this pillar fails at head, when it fails one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateCrossing>,
}

/// A gate a pillar fails at head: which bound, and whether this change
/// pushed the metric further past it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GateCrossing {
    pub metric: String,
    /// `None` when the metric was not measured at base (an added file).
    pub before: Option<f64>,
    pub after: f64,
    /// The bound on the violated side: the upper bound, or the lower one
    /// for a metric that fell below its band.
    pub limit: f64,
    /// The change moved the metric further past `limit`, or the base did
    /// not measure it at all.
    pub worse: bool,
}

impl PillarDelta {
    pub fn lost(&self) -> bool {
        self.before_passed == Some(true) && self.after_passed == Some(false)
    }
    pub fn cleared(&self) -> bool {
        self.before_passed == Some(false) && self.after_passed == Some(true)
    }
    /// Displayed-scale delta when both sides were scored.
    pub fn shift(&self) -> Option<f64> {
        self.before_score
            .zip(self.after_score)
            .map(|(before, after)| after - before)
    }
}

/// A named callable with its complexity, for "worst function" cells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FunctionRef {
    pub name: String,
    pub line: usize,
    pub complexity: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hotspot {
    pub path: String,
    pub line: usize,
    /// The function the line sits in, when the metric is per function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    pub metric: String,
    pub detail: String,
    pub advice: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClusterRole {
    Parent,
    Child,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClusterMembership {
    /// Path of the cluster parent (equals `FileRecap::path` for the parent).
    pub parent: String,
    pub role: ClusterRole,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileRecap {
    pub path: String,
    pub change: FileChange,
    /// An added file outside a split is `REGRESSION` when it arrives
    /// failing SECURE or SLOP, `IMPROVEMENT` otherwise; a split child is
    /// judged through its cluster's mark. Descriptive, like the direction.
    pub status: Headline,
    /// The worst severity among this file's findings; `None` without any.
    #[serde(serialize_with = "optional_severity_name")]
    pub severity: Option<Severity>,
    pub lines_before: usize,
    pub lines_after: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
    /// `None` for an added file.
    pub medal_before: Option<Medal>,
    /// `None` only when the file could not be parsed at head.
    pub medal_after: Option<Medal>,
    pub pillars: BTreeMap<String, PillarDelta>,
    pub structural_distance: Option<f64>,
    /// Scores moved while the syntax tree barely did (per-file SUSPICIOUS).
    pub cosmetic: bool,
    pub complexity_relocated_within_file: bool,
    pub worst_function_before: Option<FunctionRef>,
    pub worst_function_after: Option<FunctionRef>,
    /// `cfg.cyclomatic` (file-level decision count).
    pub decisions_before: Option<usize>,
    pub decisions_after: Option<usize>,
    /// From the coupling graphs; `None` when COMPOSABLE was not measured.
    pub fan_in_before: Option<usize>,
    pub fan_in_after: Option<usize>,
    pub fan_out_before: Option<usize>,
    pub fan_out_after: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<ClusterMembership>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<Hotspot>,
}

impl FileRecap {
    pub fn is_new(&self) -> bool {
        self.change == FileChange::Added
    }

    pub fn is_split_child(&self) -> bool {
        self.cluster
            .as_ref()
            .is_some_and(|member| member.role == ClusterRole::Child)
    }

    /// Landed as SLOP, or did not parse at head.
    pub fn landed_slop(&self) -> bool {
        self.medal_after
            .as_ref()
            .is_none_or(|medal| medal.tier == "SLOP")
    }
}

/// `✓ SPLIT`, `! SPLIT`, `X SPLIT`. Descriptive: the split gates in
/// `gates.rs` decide what a split costs the verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClusterMark {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClusterChild {
    pub path: String,
    /// `None` when COMPOSABLE was not measured.
    pub reach: Option<Reach>,
    /// Distinct files importing or calling this child at head, excluding itself.
    pub importers: Vec<String>,
    /// Symbols that were defined in the parent at base and live here at head.
    pub moved_in: usize,
}

/// One parent file that was split into new files in this change.
#[derive(Debug, Clone, Serialize)]
pub struct Cluster {
    pub parent: String,
    pub children: Vec<ClusterChild>,
    pub mark: ClusterMark,
    /// Short, factual reasons for the mark ("decisions rose 33→46 (+39%)").
    pub reasons: Vec<String>,
    pub lines_before: usize,
    pub lines_after: usize,
    /// Every metric that gates SECURE, summed over the parent (before) and
    /// over the parent plus its children (after).
    pub secure_findings_before: usize,
    pub secure_findings_after: usize,
    /// Σ `cfg.cyclomatic` over parent (before) and parent + children (after).
    pub decisions_before: usize,
    pub decisions_after: usize,
    pub worst_function_before: Option<FunctionRef>,
    pub worst_function_after: Option<FunctionRef>,
    pub parent_fan_out_before: Option<usize>,
    pub parent_fan_out_after: Option<usize>,
    /// Head fan-out ignoring callees that live in the cluster's own children.
    pub parent_fan_out_after_excluding_children: Option<usize>,
    /// From the coupling graphs (empty when not measured).
    pub symbols_moved: Vec<SymbolMove>,
    pub symbols_new: Vec<NewSymbol>,
    pub symbols_lost: Vec<String>,
    /// From the UAST function ledger; `None` when a side failed to parse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger: Option<Ledger>,
}

impl Cluster {
    /// The parent's fan-out rose only because it now imports the files it
    /// was carved into, so COMPOSABLE is set aside for it.
    pub fn routes_fan_out(&self) -> bool {
        self.parent_fan_out_after_excluding_children
            .zip(self.parent_fan_out_before)
            .is_some_and(|(after, before)| after <= before)
    }

    /// SECURE findings the split added across parent and children. Code
    /// moved out of the parent brings its findings along, so only a rise
    /// in the cluster total is new risk.
    pub fn secure_rise(&self) -> Option<usize> {
        self.secure_findings_after
            .checked_sub(self.secure_findings_before)
            .filter(|rise| *rise > 0)
    }

    /// Complexity the moved functions gained on the way, when the worst
    /// function did not fall to pay for it.
    pub fn moved_growth(&self) -> Option<i64> {
        let gained: i64 = self
            .ledger
            .as_ref()?
            .matches
            .iter()
            .filter(|entry| matches!(entry.kind, MatchKind::MovedModified | MatchKind::Renamed))
            .map(|entry| entry.complexity_delta)
            .filter(|delta| *delta > 0)
            .sum();
        let worst_fell = self
            .worst_function_before
            .as_ref()
            .zip(self.worst_function_after.as_ref())
            .is_some_and(|(before, after)| after.complexity < before.complexity);
        (gained > 0 && !worst_fell).then_some(gained)
    }

    /// Total decisions grew past [`CLUSTER_GROWTH_WARN`].
    pub fn bloated(&self) -> bool {
        self.decisions_after as f64 > self.decisions_before as f64 * (1.0 + CLUSTER_GROWTH_WARN)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PillarRollup {
    pub before_passed: bool,
    pub after_passed: bool,
    /// Mean displayed score over the files measuring this pillar.
    pub before_score: f64,
    pub after_score: f64,
    /// Files that measured the pillar on each side, and how many failed it.
    pub files_before: usize,
    pub files_after: usize,
    pub failing_before: usize,
    pub failing_after: usize,
}

/// Rollup over the existing (modified or renamed) files at base and at
/// head. Both sides cover the same files, so a before → after move is a
/// change in those files and not a change in who was counted. Added files
/// are reported apart, in [`AddedRollup`]: folding them into the head side
/// only would let clean new files lift the average.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectRollup {
    pub medal_before: Medal,
    pub medal_after: Medal,
    /// A pillar is present only when both sides scored it.
    pub pillars: BTreeMap<String, PillarRollup>,
    /// The rollup over the touched set, not any one file, lost a pillar:
    /// passed at base, fails at head. Can be `false` while `direction` is
    /// `REGRESSION`, since one file's regression may not move the rollup.
    pub regression: bool,
    /// Existing files compared; equal, since both sides are the same files.
    pub files_before: usize,
    pub files_after: usize,
    /// Total control-flow decision points (`Σ cfg.cyclomatic`) across existing files at base.
    /// `None` when any existing file lacks a measurement on this side.
    pub decisions_before: Option<usize>,
    /// Total at head; `None` when any existing file lacks a measurement at head.
    pub decisions_after: Option<usize>,
}

/// One pillar over the added files, at head.
#[derive(Debug, Clone, Serialize)]
pub struct AddedPillar {
    /// Every added file measuring this pillar passes it.
    pub passed: bool,
    /// Mean displayed score over the added files measuring this pillar.
    pub score: f64,
    pub files: usize,
    pub failing: usize,
}

/// The quality of the files this change added. They have no before side,
/// so they are never mixed into [`ProjectRollup`]'s comparison.
#[derive(Debug, Clone, Serialize)]
pub struct AddedRollup {
    pub files: usize,
    /// Lattice verdict over the pillars every added file passes.
    pub medal: Medal,
    pub pillars: BTreeMap<String, AddedPillar>,
    /// Total control-flow decision points (`Σ cfg.cyclomatic`) across added files at head.
    /// `None` when any added file lacks a measurement.
    pub decisions: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CouplingStatus {
    pub measured: bool,
    /// Why not, or where from ("built from .git/topos-pr-5", "gitnexus not installed").
    pub note: String,
    pub reason: CouplingReason,
    /// The quoted wait for building the graphs, when they were left
    /// unbuilt and there was a past build to go on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate_ms: Option<u64>,
}

/// Why the coupling graphs were or were not there for this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CouplingReason {
    /// Built on this run.
    Built,
    /// Already built at these commits and reused.
    Cached,
    /// `--no-coupling`.
    Flag,
    /// The graphs needed building and the person at the terminal said no
    /// (or skipped the question).
    Declined,
    /// The graphs needed building, nobody was asked, and the default was
    /// not to build.
    NotAsked,
    /// No pull request number, so no stores to build.
    NoPr,
    /// `gitnexus` is not on `PATH` and nothing was built already.
    GitnexusMissing,
    /// Resolving the commits or building the graphs failed; `note` says how.
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scope {
    pub files_scored: usize,
    pub files_new: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub files_skipped: usize,
    pub files_deleted: usize,
    /// Scoreable files left out by `--max-files`, lowest churn first out.
    pub files_capped: usize,
    pub coupling: CouplingStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullRequest {
    pub number: u64,
    pub head_ref: String,
    pub base_ref: String,
}

/// Which rules produced the verdict.
#[derive(Debug, Clone, Serialize)]
pub struct GateSummary {
    pub preset: &'static str,
    pub fail_on: &'static str,
    /// Settings that differ from the preset's.
    pub changes: usize,
    /// The `.topos.toml` consulted, or `None` when `--preset` set the
    /// policy or no file was found.
    pub source: Option<String>,
    /// How many findings a card lists before folding the rest into
    /// `N more` (`max_hotspots`). Rendering only; not in the document.
    #[serde(skip)]
    pub max_hotspots: usize,
}

/// What one `[[pr_recap.waive]]` entry did on this run.
#[derive(Debug, Clone, Serialize)]
pub struct WaiverSummary {
    pub gate: String,
    /// The path glob.
    pub path: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
    /// Findings it waived; for an expired waiver, the findings it would
    /// have waived.
    pub matched: usize,
    /// `used`, `unused` (matched nothing) or `expired`.
    pub status: &'static str,
}

impl WaiverSummary {
    pub fn is_used(&self) -> bool {
        self.status == "used"
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PrRecap {
    pub schema: &'static str,
    /// The commit compared against: the merge-base of the requested base
    /// and the head, so commits that landed on the base branch after the
    /// fork are not charged to this change.
    pub base: String,
    pub head: String,
    /// The pillar every file was classified with (`secure` unless the
    /// project configures one, or `--priority` overrides it).
    pub priority: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<PullRequest>,
    pub gate: GateSummary,
    /// The worst finding's severity: blocked, needs attention, or ready.
    pub readiness: Readiness,
    /// 1 when the readiness fails the check under `gate.fail_on`, else 0.
    pub exit_code: i32,
    /// `"fail"` iff `exit_code` is 1.
    pub check: &'static str,
    /// The first counting finding's text, or what happened when there is
    /// none.
    pub reason: String,
    /// The file `reason` names, when it names one: the lead finding's path,
    /// or the worst file the direction text is about. Not in the document.
    #[serde(skip)]
    pub reason_path: Option<String>,
    /// Every finding the configured gates kept, most important first, the
    /// waived ones last.
    pub findings: Vec<Finding>,
    /// Every `[[pr_recap.waive]]` entry and what it did.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub waivers: Vec<WaiverSummary>,
    /// The worst single file's headline (`REGRESSION` when any one file
    /// regressed), not a verdict on the touched set as a whole; compare
    /// `project.regression`. Descriptive only.
    pub direction: Headline,
    /// `--max-files` left `scope.files_capped` files unscored. The verdict
    /// covers only the highest-churn files that were scored.
    pub incomplete: bool,
    pub scope: Scope,
    /// `None` when no existing file was scored (every file is new).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectRollup>,
    /// `None` when no file was added.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<AddedRollup>,
    pub clusters: Vec<Cluster>,
    pub files: Vec<FileRecap>,
    pub skipped: Vec<SkippedFile>,
    pub deleted: Vec<String>,
    /// The first `[pr_recap] max_hotspots` hotspots, in rank order.
    pub hotspots: Vec<Hotspot>,
    /// Every hotspot the range has, shown or not.
    pub hotspots_total: usize,
    /// Structural direction is not proof that behavior is unchanged.
    pub non_claim: &'static str,
}

impl PrRecap {
    pub fn new_files(&self) -> Vec<&FileRecap> {
        self.files.iter().filter(|f| f.is_new()).collect()
    }
}

/// Whole-percent change from `before` to `after`, rounded; 100 when growing
/// from nothing. Reasons and renderers share it so the same change never
/// reads as two different percentages.
pub fn percent_change(before: usize, after: usize) -> i64 {
    if before == 0 {
        return if after == 0 { 0 } else { 100 };
    }
    #[expect(clippy::cast_precision_loss, reason = "decision counts are small")]
    let fraction = (after as f64 - before as f64) / before as f64;
    #[expect(clippy::cast_possible_truncation, reason = "rounded percentage")]
    let percent = (fraction * 100.0).round() as i64;
    percent
}
