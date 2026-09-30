//! The card's floor: the verdict line, the `Why` sentences under it, and
//! `Where to look` — one numbered item per hotspot, in `inspect`'s
//! grammar.

use console::Style;

use super::layout::{budget, push_wrapped};
use crate::commands::pr_recap::model::ProjectRollup;
use crate::commands::pr_recap::view::{headline_mark, hotspot_pillar, RecapView, PILLARS};
use crate::commands::render::{paint, truncate_left, RenderOptions};

/// At most this many `Why` sentences.
const MAX_DETAILS: usize = 2;

/// `· LATERAL · 🥉 BRONZE · SECURE · 46% → 58% average.`
pub(in crate::commands::pr_recap) fn floor_line(view: &RecapView<'_>) -> String {
    let recap = view.recap;
    let mut parts = vec![format!(
        "{} {}",
        headline_mark(recap.headline),
        recap.headline.word()
    )];
    if let Some(project) = &recap.project {
        parts.push(medal_phrase(project));
        if let Some((before, after)) = mean_scores(project) {
            parts.push(format!("{before:.0}% → {after:.0}% average."));
        }
    } else if let Some(added) = &recap.added {
        parts.push(format!(
            "{} new file{} · {}",
            added.files,
            if added.files == 1 { "" } else { "s" },
            added.medal.tier
        ));
    }
    parts.join(" · ")
}

/// `🥉 BRONZE → 🥈 SILVER · SECURE_NAVIGABLE`, or one medal when the tier
/// held. SLOP is already the failure tier, so it gets no emoji and no
/// second lattice-name echo — exactly what `evaluate`'s floor does.
fn medal_phrase(project: &ProjectRollup) -> String {
    let (before, after) = (&project.medal_before, &project.medal_after);
    if after.tier == "SLOP" {
        return "SLOP".to_string();
    }
    let tiers = if before.tier == after.tier {
        format!("{} {}", after.symbol, after.tier)
    } else if before.tier == "SLOP" {
        format!("SLOP → {} {}", after.symbol, after.tier)
    } else {
        format!(
            "{} {} → {} {}",
            before.symbol, before.tier, after.symbol, after.tier
        )
    };
    format!("{tiers} · {}", after.verdict)
}

pub(in crate::commands::pr_recap) fn mean_scores(project: &ProjectRollup) -> Option<(f64, f64)> {
    let count = project.pillars.len();
    if count == 0 {
        return None;
    }
    #[expect(clippy::cast_precision_loss, reason = "four pillars at most")]
    let divisor = count as f64;
    let before: f64 = project.pillars.values().map(|p| p.before_score).sum();
    let after: f64 = project.pillars.values().map(|p| p.after_score).sum();
    Some((before / divisor, after / divisor))
}

/// SECURE first: a security gate that stopped holding is the one a
/// reviewer must see before anything else.
fn regressed_pillars(project: &ProjectRollup) -> Vec<String> {
    let mut keys: Vec<&str> = PILLARS.to_vec();
    keys.sort_by_key(|key| usize::from(*key != "secure"));
    keys.into_iter()
        .filter(|key| {
            project
                .pillars
                .get(*key)
                .is_some_and(|pillar| pillar.before_passed && !pillar.after_passed)
        })
        .map(str::to_ascii_uppercase)
        .collect()
}

/// The sentences the floor used to carry: which pillar the touched set
/// lost, then the most severe failure and its cause — or, when nothing
/// failed, what the split did to the worst functions — then the files
/// whose scores moved while their syntax tree barely did.
fn detail_lines(view: &RecapView<'_>) -> Vec<String> {
    let recap = view.recap;
    let mut parts: Vec<String> = Vec::new();
    if let Some(project) = recap.project.as_ref().filter(|project| project.regression) {
        let pillars = regressed_pillars(project);
        if !pillars.is_empty() {
            parts.push(format!(
                "X lost {} across the touched files",
                pillars.join(", ")
            ));
        }
    }
    if let Some(failure) = view
        .failures
        .iter()
        .find(|failure| failure.word.starts_with('X'))
    {
        parts.push(match failure.split_into {
            Some(_) => format!("the split of {} failed: {}", failure.path, failure.cause),
            None => format!("{} {}", failure.path, failure.cause),
        });
    } else if !view.clusters.is_empty() {
        let (before, after) = view.decisions;
        if let Some((low, high)) = view.worst_drop {
            let span = if low == high {
                format!("{low}%")
            } else {
                format!("{low}–{high}%")
            };
            parts.push(format!(
                "worst functions down {span}, decisions {before}→{after}"
            ));
        } else {
            parts.push(format!("decisions {before}→{after}"));
        }
    }
    let cosmetic: Vec<&str> = recap
        .files
        .iter()
        .filter(|file| file.cosmetic)
        .map(|file| file.path.as_str())
        .collect();
    if !cosmetic.is_empty() {
        parts.push(format!("! cosmetic: {}", super::name_list(&cosmetic)));
    }
    parts.truncate(MAX_DETAILS);
    parts
}

/// The blocks under the floor line, in `inspect`'s grammar: the `Why`
/// sentences the floor used to carry, then `Where to look` — one
/// numbered item per hotspot, in the order the data builder ranked them.
///
/// A hotspot is where the reviewer should look, so its advice is never
/// folded away and never cut: it wraps, aligned under the label, the
/// way `inspect` wraps an interpretation. An empty string is a blank
/// separator line, printed without the card's guide rail.
pub(super) fn floor_blocks(view: &RecapView<'_>, options: RenderOptions) -> Vec<String> {
    let width = budget(options);
    let mut lines = Vec::new();
    let details = detail_lines(view);
    if !details.is_empty() {
        lines.push(String::new());
        for (index, detail) in details.iter().enumerate() {
            let label = if index == 0 { "  Why  " } else { "       " };
            push_wrapped(&mut lines, label, "       ", detail, width);
        }
    }

    let spots = &view.recap.hotspots;
    if !spots.is_empty() {
        lines.push(String::new());
        lines.push(paint(
            "  Where to look",
            Style::new().cyan().bold(),
            options,
        ));
        for (index, spot) in spots.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!(
                "  {}. {} FIX · {}",
                index + 1,
                paint("X", Style::new().red().bold(), options),
                hotspot_pillar(spot)
            ));
            push_wrapped(&mut lines, "     Why  ", "          ", &spot.detail, width);
            push_wrapped(&mut lines, "     Do   ", "          ", &spot.advice, width);
            let location = format!("{}:{}", spot.path, spot.line);
            lines.push(format!("     {}", truncate_left(&location, width - 5)));
        }
    }
    lines
}
