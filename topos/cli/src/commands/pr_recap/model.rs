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

pub(crate) const SCHEMA: &str = "topos.pr_recap.v3";

/// Below this normalized AST distance a file is "structurally unchanged".
pub(crate) const STRUCTURAL_CHANGE_THRESHOLD: f64 = 0.02;
/// A score move at least this large (0–1 scale) with no structural change is cosmetic.
pub(crate) const MEANINGFUL_SCORE_DELTA: f64 = 0.03;
/// Cluster decision growth above this fraction earns a `!` mark.
pub(crate) const CLUSTER_GROWTH_WARN: f64 = 0.10;

/// Which way the structure moved. Descriptive only: whether the change is
/// ready is [`Readiness`], decided by the configured gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Headline {
    SuspiciousNoStructuralChange,
    Regression,
    RegressionScore,
    Improvement,
    ImprovementScore,
    LateralMove,
}

impl Headline {
    /// Worst-first rank used when one file decides the PR direction.
    pub(crate) fn rank(self) -> u8 {
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
pub(crate) enum FileChange {
    Added,
    Modified,
    Renamed,
}

/// A lattice verdict as the card shows it: `🥈 SILVER · SECURE_NAVIGABLE`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Medal {
    pub(crate) symbol: String,
    pub(crate) tier: String,
    pub(crate) verdict: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PillarDelta {
    pub(crate) measured: bool,
    pub(crate) before_passed: Option<bool>,
    pub(crate) after_passed: Option<bool>,
    /// Displayed 0–100 scale, one decimal.
    pub(crate) before_score: Option<f64>,
    pub(crate) after_score: Option<f64>,
    /// The gate that failed, when a previously passing pillar no longer does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lost_gate: Option<String>,
    /// The gate this pillar fails at head, when it fails one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) gate: Option<GateCrossing>,
}

/// A gate a pillar fails at head: which bound, and whether this change
/// pushed the metric further past it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct GateCrossing {
    pub(crate) metric: String,
    /// `None` when the metric was not measured at base (an added file).
    pub(crate) before: Option<f64>,
    pub(crate) after: f64,
    /// The bound on the violated side: the upper bound, or the lower one
    /// for a metric that fell below its band.
    pub(crate) limit: f64,
    /// The change moved the metric further past `limit`, or the base did
    /// not measure it at all.
    pub(crate) worse: bool,
}

impl PillarDelta {
    pub(crate) fn lost(&self) -> bool {
        self.before_passed == Some(true) && self.after_passed == Some(false)
    }
    pub(crate) fn cleared(&self) -> bool {
        self.before_passed == Some(false) && self.after_passed == Some(true)
    }
    /// Displayed-scale delta when both sides were scored.
    pub(crate) fn shift(&self) -> Option<f64> {
        self.before_score
            .zip(self.after_score)
            .map(|(before, after)| after - before)
    }
}

/// A named callable with its complexity, for "worst function" cells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FunctionRef {
    pub(crate) name: String,
    pub(crate) line: usize,
    pub(crate) complexity: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Hotspot {
    pub(crate) path: String,
    pub(crate) line: usize,
    /// The function the line sits in, when the metric is per function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) function: Option<String>,
    pub(crate) metric: String,
    pub(crate) detail: String,
    pub(crate) advice: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ClusterRole {
    Parent,
    Child,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ClusterMembership {
    /// Path of the cluster parent (equals `FileRecap::path` for the parent).
    pub(crate) parent: String,
    pub(crate) role: ClusterRole,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FileRecap {
    pub(crate) path: String,
    pub(crate) change: FileChange,
    /// An added file outside a split is `REGRESSION` when it arrives
    /// failing SECURE or SLOP, `IMPROVEMENT` otherwise; a split child is
    /// judged through its cluster's mark. Descriptive, like the direction.
    pub(crate) status: Headline,
    /// The worst severity among this file's findings; `None` without any.
    #[serde(serialize_with = "optional_severity_name")]
    pub(crate) severity: Option<Severity>,
    pub(crate) lines_before: usize,
    pub(crate) lines_after: usize,
    pub(crate) lines_added: usize,
    pub(crate) lines_removed: usize,
    /// `None` for an added file.
    pub(crate) medal_before: Option<Medal>,
    /// `None` only when the file could not be parsed at head.
    pub(crate) medal_after: Option<Medal>,
    pub(crate) pillars: BTreeMap<String, PillarDelta>,
    pub(crate) structural_distance: Option<f64>,
    /// Scores moved while the syntax tree barely did (per-file SUSPICIOUS).
    pub(crate) cosmetic: bool,
    pub(crate) complexity_relocated_within_file: bool,
    pub(crate) worst_function_before: Option<FunctionRef>,
    pub(crate) worst_function_after: Option<FunctionRef>,
    /// `cfg.cyclomatic` (file-level decision count).
    pub(crate) decisions_before: Option<usize>,
    pub(crate) decisions_after: Option<usize>,
    /// From the coupling graphs; `None` when COMPOSABLE was not measured.
    pub(crate) fan_in_before: Option<usize>,
    pub(crate) fan_in_after: Option<usize>,
    pub(crate) fan_out_before: Option<usize>,
    pub(crate) fan_out_after: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cluster: Option<ClusterMembership>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) hotspots: Vec<Hotspot>,
}

impl FileRecap {
    pub(crate) fn is_new(&self) -> bool {
        self.change == FileChange::Added
    }

    pub(crate) fn is_split_child(&self) -> bool {
        self.cluster
            .as_ref()
            .is_some_and(|member| member.role == ClusterRole::Child)
    }

    /// Landed as SLOP, or did not parse at head.
    pub(crate) fn landed_slop(&self) -> bool {
        self.medal_after
            .as_ref()
            .is_none_or(|medal| medal.tier == "SLOP")
    }
}

/// `✓ SPLIT`, `! SPLIT`, `X SPLIT`. Descriptive: the split gates in
/// `gates.rs` decide what a split costs the verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ClusterMark {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ClusterChild {
    pub(crate) path: String,
    /// `None` when COMPOSABLE was not measured.
    pub(crate) reach: Option<Reach>,
    /// Distinct files importing or calling this child at head, excluding itself.
    pub(crate) importers: Vec<String>,
    /// Symbols that were defined in the parent at base and live here at head.
    pub(crate) moved_in: usize,
}

/// One parent file that was split into new files in this change.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Cluster {
    pub(crate) parent: String,
    pub(crate) children: Vec<ClusterChild>,
    pub(crate) mark: ClusterMark,
    /// Short, factual reasons for the mark ("decisions rose 33→46 (+39%)").
    pub(crate) reasons: Vec<String>,
    pub(crate) lines_before: usize,
    pub(crate) lines_after: usize,
    /// Every metric that gates SECURE, summed over the parent (before) and
    /// over the parent plus its children (after).
    pub(crate) secure_findings_before: usize,
    pub(crate) secure_findings_after: usize,
    /// Σ `cfg.cyclomatic` over parent (before) and parent + children (after).
    pub(crate) decisions_before: usize,
    pub(crate) decisions_after: usize,
    pub(crate) worst_function_before: Option<FunctionRef>,
    pub(crate) worst_function_after: Option<FunctionRef>,
    pub(crate) parent_fan_out_before: Option<usize>,
    pub(crate) parent_fan_out_after: Option<usize>,
    /// Head fan-out ignoring callees that live in the cluster's own children.
    pub(crate) parent_fan_out_after_excluding_children: Option<usize>,
    /// From the coupling graphs (empty when not measured).
    pub(crate) symbols_moved: Vec<SymbolMove>,
    pub(crate) symbols_new: Vec<NewSymbol>,
    pub(crate) symbols_lost: Vec<String>,
    /// From the UAST function ledger; `None` when a side failed to parse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ledger: Option<Ledger>,
}

impl Cluster {
    /// The parent's fan-out rose only because it now imports the files it
    /// was carved into, so COMPOSABLE is set aside for it.
    pub(crate) fn routes_fan_out(&self) -> bool {
        self.parent_fan_out_after_excluding_children
            .zip(self.parent_fan_out_before)
            .is_some_and(|(after, before)| after <= before)
    }

    /// SECURE findings the split added across parent and children. Code
    /// moved out of the parent brings its findings along, so only a rise
    /// in the cluster total is new risk.
    pub(crate) fn secure_rise(&self) -> Option<usize> {
        self.secure_findings_after
            .checked_sub(self.secure_findings_before)
            .filter(|rise| *rise > 0)
    }

    /// Complexity the moved functions gained on the way, when the worst
    /// function did not fall to pay for it.
    pub(crate) fn moved_growth(&self) -> Option<i64> {
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
    pub(crate) fn bloated(&self) -> bool {
        self.decisions_after as f64 > self.decisions_before as f64 * (1.0 + CLUSTER_GROWTH_WARN)
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PillarRollup {
    pub(crate) before_passed: bool,
    pub(crate) after_passed: bool,
    /// Mean displayed score over the files measuring this pillar.
    pub(crate) before_score: f64,
    pub(crate) after_score: f64,
    /// Files that measured the pillar on each side, and how many failed it.
    pub(crate) files_before: usize,
    pub(crate) files_after: usize,
    pub(crate) failing_before: usize,
    pub(crate) failing_after: usize,
}

/// Rollup over the existing (modified or renamed) files at base and at
/// head. Both sides cover the same files, so a before → after move is a
/// change in those files and not a change in who was counted. Added files
/// are reported apart, in [`AddedRollup`]: folding them into the head side
/// only would let clean new files lift the average.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProjectRollup {
    pub(crate) medal_before: Medal,
    pub(crate) medal_after: Medal,
    /// A pillar is present only when both sides scored it.
    pub(crate) pillars: BTreeMap<String, PillarRollup>,
    /// A pillar the touched set passed at base is failed at head.
    pub(crate) regression: bool,
    /// Existing files compared; equal, since both sides are the same files.
    pub(crate) files_before: usize,
    pub(crate) files_after: usize,
}

/// One pillar over the added files, at head.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AddedPillar {
    /// Every added file measuring this pillar passes it.
    pub(crate) passed: bool,
    /// Mean displayed score over the added files measuring this pillar.
    pub(crate) score: f64,
    pub(crate) files: usize,
    pub(crate) failing: usize,
}

/// The quality of the files this change added. They have no before side,
/// so they are never mixed into [`ProjectRollup`]'s comparison.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AddedRollup {
    pub(crate) files: usize,
    /// Lattice verdict over the pillars every added file passes.
    pub(crate) medal: Medal,
    pub(crate) pillars: BTreeMap<String, AddedPillar>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CouplingStatus {
    pub(crate) measured: bool,
    /// Why not, or where from ("built from .git/topos-pr-5", "gitnexus not installed").
    pub(crate) note: String,
    pub(crate) reason: CouplingReason,
    /// The quoted wait for building the graphs, when they were left
    /// unbuilt and there was a past build to go on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) estimate_ms: Option<u64>,
}

/// Why the coupling graphs were or were not there for this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CouplingReason {
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
pub(crate) struct Scope {
    pub(crate) files_scored: usize,
    pub(crate) files_new: usize,
    pub(crate) lines_added: usize,
    pub(crate) lines_removed: usize,
    pub(crate) files_skipped: usize,
    pub(crate) files_deleted: usize,
    /// Scoreable files left out by `--max-files`, lowest churn first out.
    pub(crate) files_capped: usize,
    pub(crate) coupling: CouplingStatus,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkippedFile {
    pub(crate) path: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PullRequest {
    pub(crate) number: u64,
    pub(crate) head_ref: String,
    pub(crate) base_ref: String,
}

/// Which rules produced the verdict.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GateSummary {
    pub(crate) preset: &'static str,
    pub(crate) fail_on: &'static str,
    /// Settings that differ from the preset's.
    pub(crate) changes: usize,
    /// The `.topos.toml` consulted, or `None` when `--preset` set the
    /// policy or no file was found.
    pub(crate) source: Option<String>,
    /// How many findings a card lists before folding the rest into
    /// `N more` (`max_hotspots`). Rendering only; not in the document.
    #[serde(skip)]
    pub(crate) max_hotspots: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrRecap {
    pub(crate) schema: &'static str,
    /// The commit compared against: the merge-base of the requested base
    /// and the head, so commits that landed on the base branch after the
    /// fork are not charged to this change.
    pub(crate) base: String,
    pub(crate) head: String,
    /// The pillar every file was classified with (`secure` unless the
    /// project configures one, or `--priority` overrides it).
    pub(crate) priority: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) review: Option<PullRequest>,
    pub(crate) gate: GateSummary,
    /// The worst finding's severity: blocked, needs attention, or ready.
    pub(crate) readiness: Readiness,
    /// 1 when the readiness fails the check under `gate.fail_on`, else 0.
    pub(crate) exit_code: i32,
    /// `"fail"` iff `exit_code` is 1.
    pub(crate) check: &'static str,
    /// The first finding's text, or what happened when there is none.
    pub(crate) reason: String,
    /// Every finding the configured gates kept, most important first.
    pub(crate) findings: Vec<Finding>,
    /// Which way the structure moved. Descriptive only.
    pub(crate) direction: Headline,
    /// `--max-files` left `scope.files_capped` files unscored. The verdict
    /// covers only the highest-churn files that were scored.
    pub(crate) incomplete: bool,
    pub(crate) scope: Scope,
    /// `None` when no existing file was scored (every file is new).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) project: Option<ProjectRollup>,
    /// `None` when no file was added.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) added: Option<AddedRollup>,
    pub(crate) clusters: Vec<Cluster>,
    pub(crate) files: Vec<FileRecap>,
    pub(crate) skipped: Vec<SkippedFile>,
    pub(crate) deleted: Vec<String>,
    /// The first `[pr_recap] max_hotspots` hotspots, in rank order.
    pub(crate) hotspots: Vec<Hotspot>,
    /// Every hotspot the range has, shown or not.
    pub(crate) hotspots_total: usize,
    /// Structural direction is not proof that behavior is unchanged.
    pub(crate) non_claim: &'static str,
}

impl PrRecap {
    pub(crate) fn new_files(&self) -> Vec<&FileRecap> {
        self.files.iter().filter(|f| f.is_new()).collect()
    }
}

/// Whole-percent change from `before` to `after`, rounded; 100 when growing
/// from nothing. Reasons and renderers share it so the same change never
/// reads as two different percentages.
pub(crate) fn percent_change(before: usize, after: usize) -> i64 {
    if before == 0 {
        return if after == 0 { 0 } else { 100 };
    }
    #[expect(clippy::cast_precision_loss, reason = "decision counts are small")]
    let fraction = (after as f64 - before as f64) / before as f64;
    #[expect(clippy::cast_possible_truncation, reason = "rounded percentage")]
    let percent = (fraction * 100.0).round() as i64;
    percent
}
