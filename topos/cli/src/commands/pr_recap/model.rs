//! `topos pr-recap` document model, schema `topos.pr_recap.v2`.
//!
//! One struct tree feeds `--json`, the terminal card, the compact CI card
//! and the GitHub comment. Renderers read this and never recompute a
//! verdict: every mark on a card is a field here, produced by the data
//! builder in `pr_recap.rs` from the lattice, the UAST ledger and the two
//! coupling graphs.

use std::collections::BTreeMap;

use serde::Serialize;
use topos_engine::functors::profunctors::uast::ledger::Ledger;
use topos_engine::graphs::mdg::split::{NewSymbol, Reach, SymbolMove};

pub(crate) const SCHEMA: &str = "topos.pr_recap.v2";

/// Below this normalized AST distance a file is "structurally unchanged".
pub(crate) const STRUCTURAL_CHANGE_THRESHOLD: f64 = 0.02;
/// A score move at least this large (0–1 scale) with no structural change is cosmetic.
pub(crate) const MEANINGFUL_SCORE_DELTA: f64 = 0.03;
/// Cluster decision growth above this fraction earns a `!` mark.
pub(crate) const CLUSTER_GROWTH_WARN: f64 = 0.10;

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
    /// The word a card prints. `as_str` is the JSON name.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Headline::SuspiciousNoStructuralChange => "SUSPICIOUS",
            Headline::Regression => "REGRESSION",
            Headline::RegressionScore => "SCORE DOWN",
            Headline::Improvement => "IMPROVEMENT",
            Headline::ImprovementScore => "SCORE UP",
            Headline::LateralMove => "LATERAL",
        }
    }

    pub(crate) fn fails_check(self) -> bool {
        matches!(
            self,
            Headline::SuspiciousNoStructuralChange
                | Headline::Regression
                | Headline::RegressionScore
        )
    }

    /// Worst-first rank used when one file decides the PR headline.
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
    pub(crate) status: Headline,
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
}

/// `✓ SPLIT`, `! SPLIT`, `X SPLIT`.
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

/// Rollup over every scored file at base and at head.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProjectRollup {
    pub(crate) medal_before: Medal,
    pub(crate) medal_after: Medal,
    pub(crate) pillars: BTreeMap<String, PillarRollup>,
    /// A pillar the touched set passed at base is failed at head.
    pub(crate) regression: bool,
    pub(crate) files_before: usize,
    pub(crate) files_after: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CouplingStatus {
    pub(crate) measured: bool,
    /// Why not, or where from ("built from .git/topos-pr-5", "gitnexus not installed").
    pub(crate) note: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Scope {
    pub(crate) files_scored: usize,
    pub(crate) files_new: usize,
    pub(crate) lines_added: usize,
    pub(crate) lines_removed: usize,
    pub(crate) files_skipped: usize,
    pub(crate) files_deleted: usize,
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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrRecap {
    pub(crate) schema: &'static str,
    pub(crate) base: String,
    pub(crate) head: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) review: Option<PullRequest>,
    pub(crate) headline: Headline,
    pub(crate) check: &'static str,
    pub(crate) reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    pub(crate) scope: Scope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) project: Option<ProjectRollup>,
    pub(crate) clusters: Vec<Cluster>,
    pub(crate) files: Vec<FileRecap>,
    pub(crate) skipped: Vec<SkippedFile>,
    pub(crate) deleted: Vec<String>,
    pub(crate) hotspots: Vec<Hotspot>,
    /// Structural direction is not proof that behavior is unchanged.
    pub(crate) non_claim: &'static str,
}

impl PrRecap {
    /// Files not belonging to any cluster, in document order.
    pub(crate) fn unclustered_files(&self) -> Vec<&FileRecap> {
        self.files.iter().filter(|f| f.cluster.is_none()).collect()
    }

    pub(crate) fn new_files(&self) -> Vec<&FileRecap> {
        self.files.iter().filter(|f| f.is_new()).collect()
    }
}
