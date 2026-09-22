//! Compact CI-log card for `topos pr-recap`.
//!
//! One screen of log, never more than [`MAX_LINES`] lines, printed when
//! stdout is not a TTY or `--compact` is passed. A CI log is scrolled
//! past, not read, so every row uses the same grammar — `MARK WORD
//! subject columns…` — and the mark and the word are the only things a
//! reader has to recognise.
//!
//! Like the full card, nothing here decides anything: the marks come
//! from `recap.headline`, `cluster.mark` and the medal fields.

use console::Style;

use super::model::{ClusterMark, Headline, PrRecap};
use super::render::{
    basename, cluster_decisions, headline_mark, medal_moves, name_list, new_medal_tally, stem,
    still_failing,
};
use crate::commands::render::{guide, paint, truncate_right, RenderOptions};

/// A CI card that needs scrolling has failed at its one job.
const MAX_LINES: usize = 12;
/// Header, headline and floor are never dropped.
const FIXED_LINES: usize = 3;
const MAX_HELD_NAMES: usize = 5;
const WORD_WIDTH: usize = 8;

pub(super) fn render_compact(recap: &PrRecap, options: RenderOptions) -> Vec<String> {
    let width = options.width.clamp(24, 100);
    let mut body = Vec::new();
    body.extend(cluster_rows(recap));
    body.extend(medal_rows(recap));
    body.extend(held_row(recap));
    body.extend(still_row(recap));
    body.extend(private_row(recap));

    let room = MAX_LINES - FIXED_LINES;
    if body.len() > room {
        let dropped = body.len() - (room - 1);
        body.truncate(room - 1);
        body.push(row(
            "·",
            "MORE",
            &format!("{dropped} rows omitted · --json"),
        ));
    }

    let mut lines = vec![paint(
        truncate_right(&header(recap), width),
        Style::new().bold(),
        options,
    )];
    for text in body {
        lines.push(format!(
            "{}  {}",
            guide('│', options),
            paint(truncate_right(&text, width - 3), Style::new(), options)
        ));
    }
    lines.insert(
        1,
        format!(
            "{}  {}",
            guide('│', options),
            paint(
                truncate_right(&headline(recap), width - 3),
                Style::new(),
                options
            )
        ),
    );
    lines.push(format!(
        "{}  {}",
        guide('└', options),
        paint(
            truncate_right(&floor(recap), width - 3),
            Style::new(),
            options
        )
    ));
    lines
}

/// `MARK WORD  subject` — the one row shape this card has.
fn row(mark: &str, word: &str, subject: &str) -> String {
    let head = format!("{mark} {word}");
    let pad = WORD_WIDTH.saturating_sub(head.chars().count() - mark.chars().count());
    format!("{head}{}{subject}", " ".repeat(pad.max(2)))
}

fn header(recap: &PrRecap) -> String {
    let scope = &recap.scope;
    let subject = recap.review.as_ref().map_or_else(
        || format!("{}…{}", short(&recap.base), short(&recap.head)),
        |review| format!("#{}", review.number),
    );
    format!(
        "◇  topos pr-recap {subject}  {} files +{}/-{} · COMPOSABLE {}",
        scope.files_scored,
        scope.lines_added,
        scope.lines_removed,
        if scope.coupling.measured {
            "measured"
        } else {
            "not measured"
        }
    )
}

fn short(rev: &str) -> &str {
    super::render::short_rev(rev)
}

fn headline(recap: &PrRecap) -> String {
    let (up, down) = medal_moves(recap);
    let new = recap.new_files().len();
    let cosmetic = recap.files.iter().filter(|file| file.cosmetic).count();
    let tally = new_medal_tally(recap);
    format!(
        "{} {}  {up} up · {down} lost · {new} new{} · {cosmetic} cosmetic",
        headline_mark(recap.headline),
        recap.headline.word(),
        if tally.is_empty() {
            String::new()
        } else {
            format!(": {tally}")
        }
    )
}

fn cluster_rows(recap: &PrRecap) -> Vec<String> {
    recap
        .clusters
        .iter()
        .map(|cluster| {
            let mark = match cluster.mark {
                ClusterMark::Ok => "✓",
                ClusterMark::Warn => "!",
                ClusterMark::Fail => "X",
            };
            let worst = match (
                cluster.worst_function_before.as_ref(),
                cluster.worst_function_after.as_ref(),
            ) {
                (Some(before), Some(after)) => {
                    format!("worst {}→{}", before.complexity, after.complexity)
                }
                _ => "worst ·".to_string(),
            };
            let lines = if cluster.lines_before == 0 {
                "lines ·".to_string()
            } else {
                let grown = cluster.lines_after as i64 - cluster.lines_before as i64;
                format!("lines {:+}%", grown * 100 / cluster.lines_before as i64)
            };
            let moved: usize = cluster.children.iter().map(|child| child.moved_in).sum();
            let moved_part = if moved == 0 {
                String::new()
            } else {
                format!("   moved {moved}")
            };
            row(
                mark,
                "SPLIT",
                &format!(
                    "{}  → {}   {worst}   decisions {}→{}   {lines}{moved_part}",
                    basename(&cluster.parent),
                    cluster.children.len(),
                    cluster.decisions_before,
                    cluster.decisions_after
                ),
            )
        })
        .collect()
}

fn medal_rows(recap: &PrRecap) -> Vec<String> {
    recap
        .files
        .iter()
        .filter_map(|file| {
            let (before, after) = (file.medal_before.as_ref()?, file.medal_after.as_ref()?);
            if before.tier == after.tier {
                return None;
            }
            let up = super::render::tier_rank(&after.tier) > super::render::tier_rank(&before.tier);
            let moved = super::render::PILLARS.iter().find_map(|(key, _)| {
                let delta = file.pillars.get(*key)?;
                if delta.cleared() {
                    Some(format!("cleared {}", key.to_ascii_uppercase()))
                } else if delta.lost() {
                    Some(format!("lost {}", key.to_ascii_uppercase()))
                } else {
                    None
                }
            });
            Some(row(
                if up { "✓" } else { "X" },
                if up { "UP" } else { "DOWN" },
                &format!(
                    "{}  {}→{}{}",
                    basename(&file.path),
                    before.tier,
                    after.tier,
                    moved.map_or_else(String::new, |text| format!("  {text}"))
                ),
            ))
        })
        .collect()
}

fn held_row(recap: &PrRecap) -> Option<String> {
    let held: Vec<String> = recap
        .files
        .iter()
        .filter(|file| file.status == Headline::LateralMove && !file.cosmetic && !file.is_new())
        .filter_map(|file| {
            let tier = &file.medal_after.as_ref()?.tier;
            Some(format!("{} {tier}", stem(&file.path)))
        })
        .collect();
    if held.is_empty() {
        return None;
    }
    let shown = held.len().min(MAX_HELD_NAMES);
    let mut text = held[..shown].join(" · ");
    if held.len() > shown {
        text.push_str(&format!(" · +{} more", held.len() - shown));
    }
    Some(row("·", "HELD", &text))
}

fn still_row(recap: &PrRecap) -> Option<String> {
    let (names, pillars) = still_failing(recap)?;
    let verb = if names.len() == 1 { "fails" } else { "fail" };
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    Some(row(
        "!",
        "STILL",
        &format!("{} {verb} {}", name_list(&names), pillars.join(" + ")),
    ))
}

/// "The split did not publish a seam" is a fact only this tool has.
fn private_row(recap: &PrRecap) -> Option<String> {
    let children: Vec<&super::model::ClusterChild> = recap
        .clusters
        .iter()
        .flat_map(|cluster| &cluster.children)
        .collect();
    if children.is_empty() || children.iter().all(|child| child.reach.is_none()) {
        return None;
    }
    let private = children
        .iter()
        .filter(|child| child.reach == Some(topos_engine::graphs::mdg::split::Reach::Private))
        .count();
    if private == 0 {
        return None;
    }
    Some(row(
        "·",
        "PRIV",
        &format!(
            "{private} of {} new files are imported by their parent only",
            children.len()
        ),
    ))
}

fn floor(recap: &PrRecap) -> String {
    let fails = recap.headline.fails_check();
    let mut parts = vec![format!(
        "{} {}",
        if fails { 'X' } else { '✓' },
        if fails { "FAIL" } else { "PASS" }
    )];
    if let Some(project) = &recap.project {
        let (before, after) = (&project.medal_before, &project.medal_after);
        parts.push(if before.tier == after.tier {
            format!("project {} {}", after.symbol, after.tier)
        } else {
            format!(
                "project {} {} → {} {}",
                before.symbol, before.tier, after.symbol, after.tier
            )
        });
    }
    if !recap.clusters.is_empty() {
        let (before, after) = cluster_decisions(recap);
        parts.push(format!("decisions {before}→{after}"));
    }
    parts.push(format!("exit {}", i32::from(fails)));
    parts.push("--json for the full document".to_string());
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::render_compact;
    use crate::commands::pr_recap::render::{
        fixture_losses, fixture_many_clusters, fixture_mixed, fixture_pr5,
    };
    use crate::commands::render::RenderOptions;

    fn options() -> RenderOptions {
        RenderOptions {
            styled: false,
            width: 100,
        }
    }

    #[test]
    fn pr5_fits_a_ci_log() {
        let lines = render_compact(&fixture_pr5(), options());
        let text = lines.join("\n");
        assert!(lines.len() <= 12, "{} lines:\n{text}", lines.len());
        assert!(text.contains("✓ PASS"), "{text}");
        assert!(text.contains("decisions 201→218"), "{text}");
        assert!(text.contains("✓ SPLIT"), "{text}");
        assert!(text.contains("· PRIV"), "{text}");
    }

    /// The CI floor carries the same medal the card does.
    #[test]
    fn the_floor_wears_the_medal() {
        let text = render_compact(&fixture_pr5(), options()).join("\n");
        assert!(text.contains("project 🥉 BRONZE"), "{text}");
        assert!(!text.contains('…'), "{text}");
    }

    #[test]
    fn a_regression_exits_one() {
        let text = render_compact(&fixture_losses(), options()).join("\n");
        assert!(text.contains("X FAIL"), "{text}");
        assert!(text.contains("exit 1"), "{text}");
    }

    #[test]
    fn a_huge_pr_folds_rows_rather_than_scrolling() {
        let lines = render_compact(&fixture_many_clusters(40), options());
        let text = lines.join("\n");
        assert!(lines.len() <= 12, "{} lines:\n{text}", lines.len());
        // 40 cluster rows + STILL + PRIV, of which the first 8 survive.
        assert!(text.contains("· MORE"), "{text}");
        assert!(text.contains("34 rows omitted"), "{text}");
    }

    #[test]
    fn every_card_stays_inside_the_budget() {
        for recap in [fixture_pr5(), fixture_mixed(), fixture_losses()] {
            let lines = render_compact(&recap, options());
            assert!(lines.len() <= 12, "{} lines", lines.len());
            for line in lines {
                assert!(!line.contains('\u{1b}'), "{line}");
                assert!(line.chars().count() <= 100, "{line}");
            }
        }
    }
}
