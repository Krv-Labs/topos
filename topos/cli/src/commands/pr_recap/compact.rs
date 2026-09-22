//! Compact CI-log card for `topos pr-recap`.
//!
//! One screen of log, printed when stdout is not a TTY or `--compact` is
//! passed. A CI log is scrolled past, not read, so every row uses the
//! same grammar — `MARK WORD subject…` — and the mark and the word are
//! the only things a reader has to recognise. Every section is capped,
//! so the card never grows with the size of the change: the reason wraps
//! to [`MAX_REASON_LINES`], and at most [`MAX_LOCUS`] failures and
//! hotspots are listed before a `+N more` row.
//!
//! Like the full card, nothing here decides anything: the marks come
//! from `recap.readiness`, `cluster.mark` and the medal fields.

use console::Style;

use super::model::{Hotspot, PrRecap};
use super::render::{colorize, floor_line, mean_scores, pillar_table_header, pillar_table_rows};
use super::view::{hotspot_pillar, Failure, RecapView};
use crate::commands::render::{guide, paint, truncate_right, wrap_text, RenderOptions};

const WORD_WIDTH: usize = 8;
/// The headline reason wraps to at most this many lines.
const MAX_REASON_LINES: usize = 3;
/// Failures and hotspots listed before the rest fold into `+N more`.
const MAX_LOCUS: usize = 5;

pub(super) fn render_compact(recap: &PrRecap, options: RenderOptions) -> Vec<String> {
    let view = RecapView::new(recap);
    let width = options.width.clamp(24, 100);
    let content = width - 3;
    let mut body = vec![headline(&view)];
    body.extend(reason_lines(&recap.reason, content));
    if let Some(quality) = quality_line(recap) {
        body.push(quality);
    }
    if let Some(project) = &recap.project {
        body.push(String::new());
        body.push(pillar_table_header());
        body.extend(pillar_table_rows(project));
        body.push(String::new());
    }
    let locus = locus_lines(&view);
    if !locus.is_empty() && body.last().is_some_and(|text| !text.is_empty()) {
        body.push(String::new());
    }
    body.extend(locus);

    let mut lines = vec![paint(
        truncate_right(&header(&view), width),
        Style::new().bold(),
        options,
    )];
    for text in body {
        lines.push(if text.is_empty() {
            guide('│', options)
        } else {
            format!(
                "{}  {}",
                guide('│', options),
                colorize(&truncate_right(&text, content), options)
            )
        });
    }
    lines.push(format!(
        "{}  {}",
        guide('└', options),
        colorize(&truncate_right(&compact_floor(&view), content), options)
    ));
    lines
}

/// `MARK WORD  subject` — the one row shape this card has.
fn row(mark: &str, word: &str, subject: &str) -> String {
    let head = format!("{mark} {word}");
    let pad = WORD_WIDTH.saturating_sub(head.chars().count() - mark.chars().count());
    format!("{head}{}{subject}", " ".repeat(pad.max(2)))
}

/// Blank space as wide as `row(mark, word, "")`, so a continuation line
/// starts under its row's subject.
fn indent(mark: &str, word: &str) -> String {
    " ".repeat(row(mark, word, "").chars().count())
}

fn header(view: &RecapView<'_>) -> String {
    let scope = &view.recap.scope;
    format!(
        "◇  topos pr-recap {}  {} files +{}/-{}{} · {}",
        view.subject,
        scope.files_scored,
        scope.lines_added,
        scope.lines_removed,
        view.incomplete_note(),
        view.context.join(" · ")
    )
}

fn headline(view: &RecapView<'_>) -> String {
    let tally = &view.tally;
    format!(
        "{} {}  {} up · {} lost · {} new{} · {} cosmetic",
        view.recap.readiness.mark(),
        view.recap.readiness.word(),
        tally.up,
        tally.down,
        tally.new,
        if tally.new_medals.is_empty() {
            String::new()
        } else {
            format!(": {}", tally.new_medals)
        },
        tally.cosmetic
    )
}

/// `· WHY    <recap.reason>`, wrapped under its own subject column.
fn reason_lines(reason: &str, width: usize) -> Vec<String> {
    let continuation = indent("·", "WHY");
    let available = width.saturating_sub(continuation.len()).max(12);
    let mut chunks = wrap_text(reason, available);
    if chunks.len() > MAX_REASON_LINES {
        // Hand the overflow to the last line, which is then cut with an
        // ellipsis the reader can see.
        let rest = chunks.split_off(MAX_REASON_LINES - 1).join(" ");
        chunks.push(rest);
    }
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            if index == 0 {
                row("·", "WHY", &chunk)
            } else {
                format!("{continuation}{chunk}")
            }
        })
        .collect()
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
        &format!("{before:.0}% → {after:.0}%{arrow}"),
    ))
}

/// What fails the check, then where to look: one row per failure and
/// two per hotspot (the finding, then its fix).
fn locus_lines(view: &RecapView<'_>) -> Vec<String> {
    let spots = &view.recap.hotspots;
    let total = view.failures.len() + spots.len();
    let mut lines: Vec<String> = view
        .failures
        .iter()
        .map(failure_row)
        .chain(spots.iter().map(hotspot_rows))
        .take(MAX_LOCUS)
        .flatten()
        .collect();
    if total > MAX_LOCUS {
        lines.push(row(
            "·",
            "MORE",
            &format!("+{} more · --json", total - MAX_LOCUS),
        ));
    }
    lines
}

fn failure_row(failure: &Failure<'_>) -> Vec<String> {
    let (mark, word) = failure.word.split_once(' ').unwrap_or(("X", failure.word));
    let subject = match failure.split_into {
        Some(children) => format!("{} → {children} files", failure.path),
        None => failure.path.to_string(),
    };
    vec![row(mark, word, &format!("{subject} · {}", failure.cause))]
}

fn hotspot_rows(spot: &Hotspot) -> Vec<String> {
    vec![
        row(
            "X",
            "FIX",
            &format!(
                "{}:{} · {} · {}",
                spot.path,
                spot.line,
                hotspot_pillar(spot),
                spot.detail
            ),
        ),
        format!("{}{}", indent("X", "FIX"), spot.advice),
    ]
}

/// Compact floor: extends the card's floor line with cluster decisions,
/// the exit code, and a `--json` pointer.
fn compact_floor(view: &RecapView<'_>) -> String {
    let mut parts = vec![floor_line(view)];
    if !view.clusters.is_empty() {
        let (before, after) = view.decisions;
        parts.push(format!("decisions {before}→{after}"));
    }
    parts.push(format!("exit {}", view.recap.exit_code));
    parts.push("--json for the full document".to_string());
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::render_compact;
    use crate::commands::pr_recap::fixtures::{
        fixture_losses, fixture_many_clusters, fixture_mixed, fixture_pr5, hotspot,
    };
    use crate::commands::render::RenderOptions;

    /// Header, headline, three reason lines, quality, the seven-line
    /// pillar table, five two-line hotspots, `+N more`, floor.
    const MAX_LINES: usize = 25;

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
        assert!(lines.len() <= MAX_LINES, "{} lines:\n{text}", lines.len());
        assert!(text.contains("✓ READY"), "{text}");
        assert!(text.contains("· WHY"), "{text}");
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

    #[test]
    fn the_header_says_how_it_was_scored() {
        let header = &render_compact(&fixture_pr5(), options())[0];
        assert!(
            header.contains("· priority secure · COMPOSABLE measured"),
            "{header}"
        );
    }

    #[test]
    fn a_capped_recap_says_it_is_incomplete() {
        let mut recap = fixture_pr5();
        assert!(!render_compact(&recap, options())[0].contains("incomplete"));
        recap.incomplete = true;
        recap.scope.files_capped = 3;
        let header = &render_compact(&recap, options())[0];
        assert!(header.contains("· incomplete, 3 unscored ·"), "{header}");
    }

    /// The CI floor carries the same medal the card does.
    #[test]
    fn the_floor_wears_the_medal() {
        let text = render_compact(&fixture_pr5(), options()).join("\n");
        assert!(text.contains("READY · 🥉 BRONZE · SECURE"), "{text}");
        assert!(text.contains("41% → 58% average"), "{text}");
    }

    #[test]
    fn a_regression_exits_one() {
        let text = render_compact(&fixture_losses(), options()).join("\n");
        assert!(text.contains("X BLOCKED"), "{text}");
        assert!(text.contains("BLOCKED · 🥇 GOLD → 🥈 SILVER"), "{text}");
        assert!(text.contains("· exit 1"), "{text}");
    }

    /// A red CI log must say which file failed, why, and what to change:
    /// the lost file and the failed split with their causes, then the
    /// hotspot's location, finding and fix.
    #[test]
    fn a_regression_names_the_file_the_cause_and_the_fix() {
        let mut recap = fixture_losses();
        recap.hotspots = vec![hotspot(
            "topos/engine/src/functors/probes/cpg/taint.rs",
            88,
            "cpg.dangerous_calls",
        )];
        let lines = render_compact(&recap, options());
        let text = lines.join("\n");
        assert!(
            text.contains(&format!("· WHY    {}", recap.reason)),
            "{text}"
        );
        assert!(
            text.contains(
                "X LOST   topos/engine/src/functors/probes/cpg/taint.rs · lost SIMPLE, SECURE"
            ),
            "{text}"
        );
        assert!(
            text.contains("X SPLIT  topos/engine/src/functors/probes/cpg/taint.rs → 2 files · "),
            "{text}"
        );
        assert!(
            text.contains("X FIX    topos/engine/src/functors/probes/cpg/taint.rs:88 · SECURE · "),
            "{text}"
        );
        let fix = lines
            .iter()
            .position(|line| line.contains("X FIX"))
            .expect("a fix row");
        assert!(
            lines[fix + 1].ends_with(&recap.hotspots[0].advice),
            "{text}"
        );
    }

    #[test]
    fn a_long_locus_folds_into_more() {
        let mut recap = fixture_losses();
        recap.hotspots = (1..=6)
            .map(|line| hotspot("src/a.rs", line, "ast.max_function_complexity"))
            .collect();
        let lines = render_compact(&recap, options());
        let text = lines.join("\n");
        // Two failures leave room for three of the six hotspots.
        assert_eq!(text.matches("X FIX").count(), 3, "{text}");
        assert!(text.contains("· MORE   +3 more · --json"), "{text}");
        assert!(lines.len() <= MAX_LINES, "{} lines:\n{text}", lines.len());
    }

    #[test]
    fn a_huge_pr_folds_rows_rather_than_scrolling() {
        let lines = render_compact(&fixture_many_clusters(40), options());
        let text = lines.join("\n");
        assert!(lines.len() <= MAX_LINES, "{} lines:\n{text}", lines.len());
        assert!(!text.contains("✓ SPLIT"), "{text}");
    }

    #[test]
    fn every_card_stays_inside_the_budget() {
        for recap in [fixture_pr5(), fixture_mixed(), fixture_losses()] {
            let lines = render_compact(&recap, options());
            assert!(lines.len() <= MAX_LINES, "{} lines", lines.len());
            for line in lines {
                assert!(!line.contains('\u{1b}'), "{line}");
                assert!(line.chars().count() <= 100, "{line}");
            }
        }
    }
}
