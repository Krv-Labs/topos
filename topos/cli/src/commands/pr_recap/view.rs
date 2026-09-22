//! The facts every `topos pr-recap` renderer shows, derived once.
//!
//! The terminal card, the compact CI card and the GitHub comment each
//! print the same document in their own grammar. Anything a renderer
//! would otherwise *derive* from [`PrRecap`] — the tallies, the
//! worst-function span, a cluster's decision growth, which items fail
//! the check and why — is computed here, in [`RecapView::new`], so the
//! three can never drift apart on a number.
//!
//! Like the renderers, nothing here decides a verdict: every mark still
//! comes from a field the data builder set.

use std::collections::BTreeMap;

use topos_engine::config::Severity;
use topos_engine::evaluation::policies::gates::pillar_for_metric;

use super::model::{
    percent_change, Cluster, ClusterChild, ClusterMark, FileRecap, FunctionRef, Headline, Hotspot,
    PillarDelta, PrRecap, CLUSTER_GROWTH_WARN,
};

/// Pillar keys in `Generator::ALL` order.
pub(super) const PILLARS: [&str; 4] = ["simple", "composable", "secure", "navigable"];

/// Lattice tiers, worst first, so "a medal went up" is subtraction.
const TIERS: [&str; 5] = ["SLOP", "BRONZE", "SILVER", "GOLD", "PLATINUM"];

pub(super) struct RecapView<'a> {
    pub(super) recap: &'a PrRecap,
    /// `#5` for a pull request, `2e352d7…7b18166` for two revisions.
    pub(super) subject: String,
    /// `priority secure`, `COMPOSABLE measured`, `1 skipped` — what the
    /// verdict was computed with. Files over `--max-files` are
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
    /// Everything that fails the check, most severe first.
    pub(super) failures: Vec<Failure<'a>>,
}

/// The counts the headline row and the summary sentence are made of.
pub(super) struct Tally {
    /// Medal moves over files scored on both sides.
    pub(super) up: usize,
    pub(super) down: usize,
    pub(super) new: usize,
    /// `11 PLATINUM, 5 GOLD, 1 SILVER`, biggest group first; empty when
    /// no file was added.
    pub(super) new_medals: String,
    /// Files that kept their medal while a pillar score went down.
    pub(super) dipped: usize,
    pub(super) cosmetic: usize,
    pub(super) secure_lost: usize,
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

/// One reason the check fails, in the card's own row grammar.
pub(super) struct Failure<'a> {
    /// `X LOST`, `X NEW`, `X SPLIT`, `! DOWN`, `! SUSPECT`, `! COSMETIC`.
    pub(super) word: &'static str,
    /// The file, or the parent of the failed split.
    pub(super) path: &'a str,
    /// How many files a failed split became; `None` for a file.
    pub(super) split_into: Option<usize>,
    /// `lost SIMPLE, SECURE`, `fails SECURE`, `parent lost SECURE`.
    pub(super) cause: String,
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
            failures: failures(recap),
            clusters,
        }
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
    } else if coupling.note.is_empty() {
        parts.push("COMPOSABLE not measured".to_string());
    } else {
        parts.push(format!("COMPOSABLE not measured ({})", coupling.note));
    }
    if recap.scope.files_skipped > 0 {
        parts.push(format!("{} skipped", recap.scope.files_skipped));
    }
    parts
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
    let count = |keep: fn(&FileRecap) -> bool| recap.files.iter().filter(|f| keep(f)).count();
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
        dipped: count(|file| file.status == Headline::RegressionScore),
        cosmetic: count(|file| file.cosmetic),
        secure_lost: count(secure_lost),
    }
}

fn secure_lost(file: &FileRecap) -> bool {
    file.pillars.get("secure").is_some_and(PillarDelta::lost)
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

/// Files with a blocking finding (a lost pillar, SECURE first, then a
/// failing new file), then failed splits, then files with a warning —
/// the order a reviewer should read them in.
fn failures(recap: &PrRecap) -> Vec<Failure<'_>> {
    let mut failing: Vec<&FileRecap> = recap
        .files
        .iter()
        .filter(|file| file.severity >= Some(Severity::Warn))
        .collect();
    failing.sort_by_key(|file| {
        (
            file.status.rank(),
            !secure_lost(file),
            file.is_new(),
            file.path.clone(),
        )
    });
    let (severe, warned): (Vec<&FileRecap>, Vec<&FileRecap>) = failing
        .into_iter()
        .partition(|file| file.severity == Some(Severity::Block));
    let splits = recap
        .clusters
        .iter()
        .filter(|cluster| cluster.mark == ClusterMark::Fail)
        .map(|cluster| Failure {
            word: "X SPLIT",
            path: &cluster.parent,
            split_into: Some(cluster.children.len()),
            cause: cluster
                .reasons
                .first()
                .cloned()
                .unwrap_or_else(|| "the split failed".to_string()),
        });
    severe
        .into_iter()
        .map(file_failure)
        .chain(splits)
        .chain(warned.into_iter().map(file_failure))
        .collect()
}

fn file_failure(file: &FileRecap) -> Failure<'_> {
    let word = change_word(file);
    let cause = if file.is_new() {
        if file
            .pillars
            .get("secure")
            .is_some_and(|delta| delta.after_passed == Some(false))
        {
            "arrived failing SECURE".to_string()
        } else {
            "arrived as SLOP".to_string()
        }
    } else if file.status == Headline::Regression || lost_a_pillar(file) {
        let lost = pillar_names(file, PillarDelta::lost);
        if lost.is_empty() {
            "lost a pillar".to_string()
        } else {
            format!("lost {}", lost.join(", "))
        }
    } else if file.status == Headline::RegressionScore {
        let fell: Vec<String> = PILLARS
            .iter()
            .filter_map(|key| {
                let delta = file.pillars.get(*key)?;
                let (before, after) = delta.before_score.zip(delta.after_score)?;
                (after < before)
                    .then(|| format!("{} {before:.0}% → {after:.0}%", key.to_ascii_uppercase()))
            })
            .collect();
        if fell.is_empty() {
            "a pillar score went down".to_string()
        } else {
            fell.join(", ")
        }
    } else {
        "scores moved while the syntax tree barely changed".to_string()
    };
    Failure {
        word,
        path: &file.path,
        split_into: None,
        cause,
    }
}

// ---------------------------------------------------------------- shared

pub(super) fn cluster_mark(mark: ClusterMark) -> char {
    match mark {
        ClusterMark::Ok => '✓',
        ClusterMark::Warn => '!',
        ClusterMark::Fail => 'X',
    }
}

/// The mark and word a file's row leads with.
pub(super) fn change_word(file: &FileRecap) -> &'static str {
    // A lost pillar blocks and a cosmetic edit only warns, so the loss
    // leads even when another pillar was gained.
    if !file.is_new() && lost_a_pillar(file) {
        return "X LOST";
    }
    if file.cosmetic {
        return "! COSMETIC";
    }
    if file.is_new() {
        return if file.status == Headline::Regression {
            "X NEW"
        } else {
            "✓ NEW"
        };
    }
    match file.status {
        Headline::Regression => "X LOST",
        Headline::RegressionScore => "! DOWN",
        Headline::SuspiciousNoStructuralChange => "! SUSPECT",
        Headline::Improvement | Headline::ImprovementScore => "✓ UP",
        Headline::LateralMove => "· HELD",
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

/// `●●●● PLATINUM ×2, ○●●● GOLD` (multiply) or `11 PLATINUM, 5 GOLD`
/// (count). Entries are `(tier, label)`: the tier orders the groups, the
/// label is what the reader sees.
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

/// `SECURE` for a dangerous-call finding, `SIMPLE` for a complexity one.
pub(super) fn hotspot_pillar(spot: &Hotspot) -> String {
    pillar_for_metric(&spot.metric).to_ascii_uppercase()
}

pub(super) fn tier_rank(tier: &str) -> usize {
    TIERS.iter().position(|known| *known == tier).unwrap_or(0)
}

pub(super) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub(super) fn short_rev(rev: &str) -> &str {
    if rev.len() >= 7 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return &rev[..7];
    }
    rev
}
