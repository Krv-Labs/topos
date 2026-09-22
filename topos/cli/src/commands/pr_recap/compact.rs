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

use super::model::PrRecap;
use super::render::{
    cluster_decisions, floor_line, headline_mark, mean_scores, medal_moves, new_medal_tally,
    pillar_table_header, pillar_table_rows,
};
use crate::commands::render::{guide, paint, truncate_right, RenderOptions};

/// A CI card that needs scrolling has failed at its one job.
const MAX_LINES: usize = 12;
/// Header, headline, floor = 3 fixed.
const FIXED_LINES: usize = 3;
const WORD_WIDTH: usize = 8;

pub(super) fn render_compact(recap: &PrRecap, options: RenderOptions) -> Vec<String> {
    let width = options.width.clamp(24, 100);
    let mut body = Vec::new();

    // Quality trend line (single line)
    if let Some(q) = quality_line(recap) {
        body.push(q);
    }

    // Pillar table: header + up to 4 rows
    if let Some(project) = &recap.project {
        body.push(String::new());
        body.push(pillar_table_header());
        body.extend(pillar_table_rows(project));
        body.push(String::new());
    }

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
        if text.is_empty() {
            lines.push(guide('│', options));
        } else {
            lines.push(format!(
                "{}  {}",
                guide('│', options),
                super::render::colorize(&truncate_right(&text, width - 3), options)
            ));
        }
    }
    lines.insert(
        1,
        format!(
            "{}  {}",
            guide('│', options),
            super::render::colorize(&truncate_right(&headline(recap), width - 3), options)
        ),
    );
    lines.push(format!(
        "{}  {}",
        guide('└', options),
        super::render::colorize(&truncate_right(&compact_floor(recap), width - 3), options)
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

/// Quality trend: "41% → 59% ↑" showing overall average score change.
fn quality_line(recap: &PrRecap) -> Option<String> {
    let (before, after) = mean_scores(recap.project.as_ref()?)?;
    let arrow = if after - before >= 1.0 {
        " ↑"
    } else if after - before <= -1.0 {
        " ↓"
    } else {
        " →"
    };
    Some(row(
        "·",
        "QUALITY",
        &format!("{:.0}% → {:.0}%{}", before, after, arrow),
    ))
}

/// Compact floor: extends render's floor_line with cluster decisions, exit code, and --json pointer.
fn compact_floor(recap: &PrRecap) -> String {
    let mut parts = vec![floor_line(recap)];
    if !recap.clusters.is_empty() {
        let (before, after) = cluster_decisions(recap);
        parts.push(format!("decisions {before}→{after}"));
    }
    parts.push(format!("exit {}", i32::from(recap.headline.fails_check())));
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
            width: 200,
        }
    }

    #[test]
    fn pr5_fits_a_ci_log() {
        let lines = render_compact(&fixture_pr5(), options());
        let text = lines.join("\n");
        // Budget: 12 lines (header, headline, quality, 2 spacers, 4 pillar rows + header, floor = 11 lines)
        assert!(lines.len() <= 12, "{} lines:\n{text}", lines.len());
        assert!(text.contains("✓ IMPROVEMENT"), "{text}");
        assert!(text.contains("· QUALITY"), "{text}");
        assert!(text.contains("PILLAR"), "{text}");
        assert!(!text.contains("✓ SPLIT"), "{text}");
        assert!(!text.contains("· PRIV"), "{text}");
        assert!(!text.contains("· HELD"), "{text}");

        // Spacer guide lines above and below pillar table
        let pillar_idx = lines
            .iter()
            .position(|l| l.contains("PILLAR"))
            .expect("pillar table header");
        assert_eq!(lines[pillar_idx - 1].trim(), "│");

        let last_pillar_idx = lines
            .iter()
            .position(|l| l.contains("NAVIGABLE"))
            .expect("last pillar row");
        assert_eq!(lines[last_pillar_idx + 1].trim(), "│");
    }

    /// The CI floor carries the same medal the card does.
    #[test]
    fn the_floor_wears_the_medal() {
        let text = render_compact(&fixture_pr5(), options()).join("\n");
        assert!(text.contains("🥉 BRONZE"), "{text}");
        assert!(text.contains("SECURE"), "{text}");
        // Floor includes full verdict (may be truncated in narrow terminals)
        assert!(text.contains("IMPROVEMENT · 🥉 BRONZE · SECURE"), "{text}");
        assert!(text.contains("41% → 58% average"), "{text}");
    }

    #[test]
    fn a_regression_exits_one() {
        let text = render_compact(&fixture_losses(), options()).join("\n");
        // Floor now shows full verdict with exit code (may be truncated in narrow terminals)
        assert!(text.contains("X REGRESSION"), "{text}");
        assert!(text.contains("REGRESSION · 🥇 GOLD → 🥈 SILVER"), "{text}");
    }

    #[test]
    fn a_huge_pr_folds_rows_rather_than_scrolling() {
        let lines = render_compact(&fixture_many_clusters(40), options());
        let text = lines.join("\n");
        assert!(lines.len() <= 12, "{} lines:\n{text}", lines.len());
        assert!(!text.contains("✓ SPLIT"), "{text}");
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
