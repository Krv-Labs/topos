//! The facts every `topos pr-recap` renderer shows, derived once.
//!
//! The terminal card and the GitHub comment each print the same document
//! in their own grammar. Anything a renderer would otherwise *derive*
//! from [`PrRecap`] — the tallies, the worst-function span, a cluster's
//! decision growth, which findings block and which need attention — is
//! computed here, in [`RecapView::new`], so the two can never drift apart
//! on a number.
//!
//! Like the renderers, nothing here decides a verdict: every mark still
//! comes from a field the data builder set.

use std::collections::BTreeMap;

use topos_engine::config::{GateId, Severity};

use super::gates::Finding;
use super::model::{
    percent_change, Cluster, ClusterChild, ClusterMark, CouplingReason, FileRecap, FunctionRef,
    PillarDelta, PrRecap, CLUSTER_GROWTH_WARN,
};
use crate::commands::render::truncate_right;

/// Pillar keys in `Generator::ALL` order.
pub(super) const PILLARS: [&str; 4] = ["simple", "composable", "secure", "navigable"];

/// Lattice tiers, worst first, so "a medal went up" is subtraction.
const TIERS: [&str; 5] = ["SLOP", "BRONZE", "SILVER", "GOLD", "PLATINUM"];

pub(super) struct RecapView<'a> {
    pub(super) recap: &'a PrRecap,
    /// `#5` for a pull request, `2e352d7…7b18166` for two revisions.
    pub(super) subject: String,
    /// `priority secure`, `COMPOSABLE measured` — what the verdict was
    /// computed with. Files over `--max-files` are
    /// [`RecapView::incomplete_note`]'s to report.
    pub(super) context: Vec<String>,
    pub(super) tally: Tally,
    pub(super) clusters: Vec<ClusterView<'a>>,
    /// Σ decisions over every cluster, before → after.
    pub(super) decisions: (usize, usize),
    /// Smallest and largest worst-function drop across the clusters, in
    /// whole percent; `None` when no cluster's worst function fell.
    pub(super) worst_drop: Option<(i64, i64)>,
    /// Split parents that still fail pillars, grouped by the exact set
    /// they fail; the largest group, as `(names, pillars)`.
    pub(super) still_failing: Option<(Vec<String>, Vec<String>)>,
    /// The block findings, most important first.
    pub(super) blocking: Vec<&'a Finding>,
    /// The warn findings, most important first.
    pub(super) attention: Vec<&'a Finding>,
    /// The info findings: counted on the card, collapsed in the GitHub
    /// comment.
    pub(super) notes: Vec<&'a Finding>,
}

/// The counts the summary sentence is made of.
pub(super) struct Tally {
    /// Medal moves over files scored on both sides.
    pub(super) up: usize,
    pub(super) down: usize,
    pub(super) new: usize,
    /// `11 PLATINUM, 5 GOLD, 1 SILVER`, biggest group first; empty when
    /// no file was added.
    pub(super) new_medals: String,
}

pub(super) struct ClusterView<'a> {
    pub(super) cluster: &'a Cluster,
    pub(super) mark: char,
    pub(super) parent: Option<&'a FileRecap>,
    /// Every child, with its scored file when it has one.
    pub(super) children: Vec<(&'a ClusterChild, Option<&'a FileRecap>)>,
    /// Whole-percent decision growth, only when it earns the `!` mark.
    pub(super) growth: Option<i64>,
}

impl ClusterView<'_> {
    /// `117→85` (with `arrow` between), `85` for a function that did not
    /// exist at base, `·` when head has none.
    pub(super) fn worst(&self, arrow: &str) -> String {
        worst_span(
            self.cluster.worst_function_before.as_ref(),
            self.cluster.worst_function_after.as_ref(),
            arrow,
        )
    }
}

/// Findings at one place — the same file, line and function — read as
/// one item: a function that lost SIMPLE and NAVIGABLE is one thing to
/// fix, not two.
pub(super) struct Item<'a> {
    /// In the recap's order, so the first is the most important.
    pub(super) findings: Vec<&'a Finding>,
}

impl<'a> Item<'a> {
    pub(super) fn lead(&self) -> &'a Finding {
        self.findings[0]
    }

    pub(super) fn severity(&self) -> Severity {
        self.lead().severity
    }
}

impl<'a> RecapView<'a> {
    pub(super) fn new(recap: &'a PrRecap) -> Self {
        let clusters: Vec<ClusterView<'a>> = recap
            .clusters
            .iter()
            .map(|cluster| cluster_view(recap, cluster))
            .collect();
        let decisions = recap
            .clusters
            .iter()
            .fold((0, 0), |(before, after), cluster| {
                (
                    before + cluster.decisions_before,
                    after + cluster.decisions_after,
                )
            });
        Self {
            recap,
            subject: subject(recap),
            context: context(recap),
            tally: tally_of(recap),
            decisions,
            worst_drop: worst_drop(recap),
            still_failing: still_failing(&clusters),
            blocking: at_severity(recap, Severity::Block),
            attention: at_severity(recap, Severity::Warn),
            notes: at_severity(recap, Severity::Info),
            clusters,
        }
    }

    /// The block and warn findings merged by place, most important first.
    pub(super) fn items(&self) -> Vec<Item<'a>> {
        merged(self.blocking.iter().chain(&self.attention).copied())
    }

    /// The info findings merged by place, in the recap's order, less the
    /// dips too small to show.
    pub(super) fn note_items(&self) -> Vec<Item<'a>> {
        merged(
            self.notes
                .iter()
                .copied()
                .filter(|finding| !hidden_dip(finding)),
        )
    }

    /// ` · incomplete, 3 unscored` when `--max-files` left files out; a
    /// CI log must not read as complete.
    pub(super) fn incomplete_note(&self) -> String {
        if self.recap.incomplete {
            format!(" · incomplete, {} unscored", self.recap.scope.files_capped)
        } else {
            String::new()
        }
    }
}

/// Findings at the same `(path, line, function)` as one item, placed
/// where its first finding was.
fn merged<'a>(findings: impl Iterator<Item = &'a Finding>) -> Vec<Item<'a>> {
    let mut items: Vec<Item<'a>> = Vec::new();
    for finding in findings {
        let same_place = |other: &Finding| {
            other.path == finding.path
                && other.line == finding.line
                && other.function == finding.function
        };
        match items.iter_mut().find(|item| same_place(item.lead())) {
            Some(item) => item.findings.push(finding),
            None => items.push(Item {
                findings: vec![finding],
            }),
        }
    }
    items
}

/// Score moves under this, on the displayed 0–100 scale, are hidden by
/// both renderers: no arrow, no CHANGE segment, no note.
const MIN_VISIBLE_SHIFT: f64 = 1.0;

/// A score move of at least a point, either way.
pub(super) fn visible_shift(shift: f64) -> bool {
    shift.abs() >= MIN_VISIBLE_SHIFT
}

/// How far a finding's score fell, `before − after`.
pub(super) fn drop_of(finding: &Finding) -> f64 {
    finding.before.unwrap_or_default() - finding.after.unwrap_or_default()
}

/// A score drop of at least a point: shown, or counted as a smaller dip.
pub(super) fn visible_dip(finding: &Finding) -> bool {
    finding.gate == GateId::ScoreDrop && drop_of(finding) >= MIN_VISIBLE_SHIFT
}

/// A score drop under a point: neither renderer shows or counts it.
fn hidden_dip(finding: &Finding) -> bool {
    finding.gate == GateId::ScoreDrop && !visible_dip(finding)
}

fn at_severity(recap: &PrRecap, severity: Severity) -> Vec<&Finding> {
    recap
        .findings
        .iter()
        .filter(|finding| finding.severity == severity)
        .collect()
}

fn subject(recap: &PrRecap) -> String {
    recap.review.as_ref().map_or_else(
        || format!("{}…{}", short_rev(&recap.base), short_rev(&recap.head)),
        |review| format!("#{}", review.number),
    )
}

fn context(recap: &PrRecap) -> Vec<String> {
    // Saying which priority classified the files is what makes the gates
    // on a card comparable to `evaluate`'s.
    let mut parts = vec![format!("priority {}", recap.priority)];
    let coupling = &recap.scope.coupling;
    if coupling.measured {
        parts.push("COMPOSABLE measured".to_string());
    } else if coupling.reason == CouplingReason::Error {
        // The error itself can run to lines of gitnexus output; the tip
        // points at it.
        parts.push("COMPOSABLE not measured (graph build failed)".to_string());
    } else if coupling.note.is_empty() {
        parts.push("COMPOSABLE not measured".to_string());
    } else {
        parts.push(format!("COMPOSABLE not measured ({})", coupling.note));
    }
    parts
}

/// What to do about unmeasured COMPOSABLE, by why it went unmeasured.
/// Nothing when it was measured, turned off, or there was no pull request.
pub(super) fn coupling_tip(recap: &PrRecap) -> Option<String> {
    let coupling = &recap.scope.coupling;
    match coupling.reason {
        CouplingReason::Declined | CouplingReason::NotAsked => {
            let wait = coupling.estimate_ms.map_or_else(
                || "usually 10–60 s".to_string(),
                |ms| format!("~{}", seconds(ms)),
            );
            Some(format!(
                "Tip: re-run with --yes to build the coupling graphs ({wait} once)."
            ))
        }
        CouplingReason::GitnexusMissing => Some(
            "Tip: install GitNexus (npm install -g gitnexus) to measure COMPOSABLE.".to_string(),
        ),
        CouplingReason::Error => {
            let cause = coupling.note.lines().next().unwrap_or("").trim();
            Some(format!(
                "Tip: building the coupling graphs failed ({}); --json has the full error.",
                truncate_right(cause, 80)
            ))
        }
        CouplingReason::Built
        | CouplingReason::Cached
        | CouplingReason::Flag
        | CouplingReason::NoPr => None,
    }
}

/// `25000` → `25 s`.
pub(super) fn seconds(ms: u64) -> String {
    format!("{} s", (ms + 500) / 1_000)
}

fn tally_of(recap: &PrRecap) -> Tally {
    let mut up = 0;
    let mut down = 0;
    for file in &recap.files {
        let (Some(before), Some(after)) = (&file.medal_before, &file.medal_after) else {
            continue;
        };
        match tier_rank(&after.tier).cmp(&tier_rank(&before.tier)) {
            std::cmp::Ordering::Greater => up += 1,
            std::cmp::Ordering::Less => down += 1,
            std::cmp::Ordering::Equal => {}
        }
    }
    let new_files = recap.new_files();
    Tally {
        up,
        down,
        new: new_files.len(),
        new_medals: tally(
            new_files
                .into_iter()
                .filter_map(|file| file.medal_after.as_ref())
                .map(|medal| (medal.tier.clone(), medal.tier.clone())),
            false,
        ),
    }
}

/// Whether any pillar the base passed now fails — a loss even when
/// another pillar was gained and `status` reads as a lateral move.
pub(super) fn lost_a_pillar(file: &FileRecap) -> bool {
    file.pillars.values().any(PillarDelta::lost)
}

fn cluster_view<'a>(recap: &'a PrRecap, cluster: &'a Cluster) -> ClusterView<'a> {
    let file_at = |path: &str| recap.files.iter().find(|file| file.path == path);
    let parent = recap
        .files
        .iter()
        .find(|file| file.path == cluster.parent && file.cluster.is_some())
        .or_else(|| file_at(&cluster.parent));
    let (before, after) = (cluster.decisions_before, cluster.decisions_after);
    #[expect(clippy::cast_precision_loss, reason = "decision counts are small")]
    let warn = after > before && before > 0 && {
        let fraction = (after - before) as f64 / before as f64;
        fraction > CLUSTER_GROWTH_WARN
    };
    ClusterView {
        cluster,
        mark: cluster_mark(cluster.mark),
        parent,
        children: cluster
            .children
            .iter()
            .map(|child| (child, file_at(&child.path)))
            .collect(),
        growth: warn.then(|| percent_change(before, after)),
    }
}

fn worst_drop(recap: &PrRecap) -> Option<(i64, i64)> {
    let drops: Vec<i64> = recap
        .clusters
        .iter()
        .filter_map(|cluster| {
            let before = cluster.worst_function_before.as_ref()?.complexity;
            let after = cluster.worst_function_after.as_ref()?.complexity;
            (before > after).then(|| -percent_change(before, after))
        })
        .collect();
    Some((*drops.iter().min()?, *drops.iter().max()?))
}

fn still_failing(clusters: &[ClusterView<'_>]) -> Option<(Vec<String>, Vec<String>)> {
    let mut groups: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    for view in clusters {
        let Some(file) = view.parent else {
            continue;
        };
        let failing = pillar_names(file, |delta| delta.after_passed == Some(false));
        if failing.is_empty() {
            continue;
        }
        groups
            .entry(failing)
            .or_default()
            .push(basename(&file.path).to_string());
    }
    groups
        .into_iter()
        .max_by_key(|(_, names)| names.len())
        .map(|(pillars, names)| (names, pillars))
}

/// Upper-case names of the pillars `keep` selects, in `PILLARS` order.
pub(super) fn pillar_names(file: &FileRecap, keep: impl Fn(&PillarDelta) -> bool) -> Vec<String> {
    PILLARS
        .iter()
        .filter(|key| file.pillars.get(**key).is_some_and(&keep))
        .map(|key| key.to_ascii_uppercase())
        .collect()
}

// ---------------------------------------------------------------- shared

pub(super) fn cluster_mark(mark: ClusterMark) -> char {
    match mark {
        ClusterMark::Ok => '✓',
        ClusterMark::Warn => '!',
        ClusterMark::Fail => 'X',
    }
}

/// Worst-function complexity, before `arrow` after; just the head value
/// for a function that did not exist at base; `·` when head has none.
pub(super) fn worst_span(
    before: Option<&FunctionRef>,
    after: Option<&FunctionRef>,
    arrow: &str,
) -> String {
    match (before, after) {
        (Some(before), Some(after)) => {
            format!("{}{arrow}{}", before.complexity, after.complexity)
        }
        (None, Some(after)) => after.complexity.to_string(),
        (_, None) => "·".to_string(),
    }
}

/// `PLATINUM ×2, GOLD` (multiply) or `11 PLATINUM, 5 GOLD` (count).
/// Entries are `(tier, label)`: the tier orders the groups, the label is
/// what the reader sees.
pub(super) fn tally<I: Iterator<Item = (String, String)>>(entries: I, multiply: bool) -> String {
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for entry in entries {
        *counts.entry(entry).or_default() += 1;
    }
    let mut ordered: Vec<((String, String), usize)> = counts.into_iter().collect();
    ordered.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(tier_rank(&b.0 .0).cmp(&tier_rank(&a.0 .0)))
            .then(a.0.cmp(&b.0))
    });
    ordered
        .into_iter()
        .map(|((_, label), count)| {
            if multiply && count == 1 {
                label
            } else if multiply {
                format!("{label} ×{count}")
            } else {
                format!("{count} {label}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn tier_rank(tier: &str) -> usize {
    TIERS.iter().position(|known| *known == tier).unwrap_or(0)
}

pub(super) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The directory every path sits under, with its trailing `/`, when at
/// least two different paths share one. Paths are then shown relative to
/// it, and it is printed once.
pub(super) fn common_parent<'p>(paths: impl IntoIterator<Item = &'p str>) -> Option<String> {
    let mut paths: Vec<&str> = paths.into_iter().collect();
    paths.sort_unstable();
    paths.dedup();
    let (first, rest) = paths.split_first()?;
    if rest.is_empty() {
        return None;
    }
    let mut parent = dirname(first);
    for path in rest {
        while !path.starts_with(parent) {
            parent = dirname(parent.trim_end_matches('/'));
        }
    }
    (!parent.is_empty()).then(|| parent.to_string())
}

/// Everything up to and including the last `/`; empty for a bare name.
fn dirname(path: &str) -> &str {
    &path[..path.rfind('/').map_or(0, |at| at + 1)]
}

/// `path` without the `parent` that [`common_parent`] found.
pub(super) fn relative<'p>(path: &'p str, parent: Option<&str>) -> &'p str {
    parent
        .and_then(|parent| path.strip_prefix(parent))
        .unwrap_or(path)
}

/// The shortest trailing part of `path` that no other path in `all` ends
/// with: `dispatch.rs`, or `graphs/mod.rs` beside another `mod.rs`.
pub(super) fn short_name<'p>(path: &'p str, all: &[&str]) -> &'p str {
    let mut start = path.rfind('/').map_or(0, |at| at + 1);
    loop {
        let suffix = &path[start..];
        let clashes = all.iter().any(|other| {
            *other != path && (*other == suffix || other.ends_with(&format!("/{suffix}")))
        });
        if !clashes || start == 0 {
            return suffix;
        }
        start = path[..start - 1].rfind('/').map_or(0, |at| at + 1);
    }
}

pub(super) fn short_rev(rev: &str) -> &str {
    if rev.len() >= 7 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return &rev[..7];
    }
    rev
}

#[cfg(test)]
mod tests {
    use super::{common_parent, relative, short_name};

    #[test]
    fn a_common_parent_needs_two_paths() {
        let install = [
            "topos/cli/src/commands/install/status.rs",
            "topos/cli/src/commands/install/configure.rs",
        ];
        let parent = common_parent(install);
        assert_eq!(parent.as_deref(), Some("topos/cli/src/commands/install/"));
        assert_eq!(relative(install[0], parent.as_deref()), "status.rs");
        assert_eq!(
            common_parent(["topos/engine/a.rs", "topos/cli/b.rs"]).as_deref(),
            Some("topos/")
        );
        assert_eq!(common_parent(["a.rs", "src/b.rs"]), None);
        assert_eq!(common_parent(["src/a.rs", "src/a.rs"]), None);
    }

    #[test]
    fn a_short_name_is_the_shortest_unambiguous_suffix() {
        let all = [
            "topos/engine/src/graphs/mod.rs",
            "topos/cli/src/commands/mod.rs",
            "topos/engine/src/graphs/ast/dispatch.rs",
        ];
        assert_eq!(short_name(all[0], &all), "graphs/mod.rs");
        assert_eq!(short_name(all[2], &all), "dispatch.rs");
        assert_eq!(short_name("mod.rs", &["mod.rs", "src/mod.rs"]), "mod.rs");
    }
}
