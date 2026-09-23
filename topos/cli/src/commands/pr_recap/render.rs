//! Primary terminal card for `topos pr-recap`.
//!
//! Two headed tables and a floor. The **scores** table answers "what did
//! the lattice do to the files I touched"; the **splits** table answers
//! "what happened to the files that were broken apart". Either table is
//! omitted entirely when it has no rows, so a plain edit never prints a
//! `SPLIT` header and a pure refactor never prints an empty score table.
//!
//! Nothing here computes a verdict. Every glyph is a field of
//! [`PrRecap`]: `recap.headline`, `file.status`, `cluster.mark`,
//! `file.cosmetic`, `PillarDelta::lost`/`cleared`, `project.regression`.
//! The renderer only chooses which already-decided fact is the most
//! useful one to show, and how to fold the rest away.

mod floor;
mod layout;
mod splits;

use console::Style;

use super::model::{ClusterRole, FileRecap, Headline, PillarDelta, PrRecap, ProjectRollup};
use super::view::{change_word, headline_mark, RecapView, PILLARS};
use crate::commands::evaluate::summary::{score_rail, status_text};
use crate::commands::render::{guide, paint, truncate_left, RenderOptions};
pub(super) use floor::{floor_line, mean_scores};
pub(super) use layout::colorize;
use layout::{
    budget, clamp, dim_line, header_dim_line, line, pad, plain_budget, row, CHANGE_WIDTH,
    FILE_WIDTH, MATRIX_HEADER, PILLAR_COL,
};

const MAX_NAMES: usize = 3;
/// Columns of the project pillar table, `evaluate`'s exact shape.
const PILLAR_NAME_WIDTH: usize = 13;
const RAIL_WIDTH: usize = 10;
/// Longest branch name shown on the context line.
const MAX_BRANCH_CHARS: usize = 40;

pub(super) fn render_card(recap: &PrRecap, verbose: bool, options: RenderOptions) -> Vec<String> {
    let view = RecapView::new(recap);
    let mut lines = vec![
        paint(
            clamp(&header_line(recap), plain_budget(options)),
            Style::new().bold(),
            options,
        ),
        dim_line(&context_line(&view), options),
        guide('│', options),
        line(&headline_line(&view), options),
        guide('│', options),
    ];

    if let Some(project) = &recap.project {
        lines.push(header_dim_line(&pillar_table_header(), options));
        for text in pillar_table_rows(project) {
            lines.push(line(&text, options));
        }
        lines.push(guide('│', options));
    }

    let scored = score_rows(recap);
    if !scored.is_empty() {
        lines.push(header_dim_line(&scores_header(), options));
        for file in &scored {
            lines.extend(score_lines(file, verbose, options));
        }
        lines.push(guide('│', options));
    }

    if !view.clusters.is_empty() {
        lines.push(header_dim_line(&splits::splits_header(), options));
        for cluster in &view.clusters {
            lines.extend(splits::cluster_block(cluster, verbose, options));
        }
        lines.push(guide('│', options));
    }

    if let Some(text) = held_line(recap, verbose) {
        lines.push(line(&text, options));
        lines.push(guide('│', options));
    }

    lines.push(format!(
        "{}  {}",
        guide('└', options),
        colorize(&clamp(&floor_line(&view), budget(options)), options)
    ));
    lines.extend(floor::floor_blocks(&view, options));
    lines
}

// --------------------------------------------------- project pillar table

pub(super) fn pillar_table_header() -> String {
    format!(
        "{:<PILLAR_NAME_WIDTH$} {:<8}{:>7}{:>8}{:>10}   {}",
        "PILLAR", "STATUS", "BEFORE", "AFTER", "FAILING", "SCORE"
    )
}

/// One row per measured pillar, before → after, in `Generator::ALL` order.
///
/// The status, the rail and the failing count all describe *head*: the
/// before column is the only backward-looking cell, which is what makes
/// this table a sibling of `evaluate`'s rather than a diff of two.
pub(super) fn pillar_table_rows(project: &ProjectRollup) -> Vec<String> {
    PILLARS
        .iter()
        .filter_map(|key| {
            let pillar = project.pillars.get(*key)?;
            // Plain text: `line` colorizes the marks itself, and `clamp`
            // counts chars, so an escape sequence here would mis-measure.
            let status = status_text(
                pillar.after_passed,
                pillar.after_score / 100.0,
                RenderOptions {
                    styled: false,
                    width: 100,
                },
            );
            let failing = format!("{} / {}", pillar.failing_after, pillar.files_after);
            let mut row = format!(
                "{:<PILLAR_NAME_WIDTH$} {status}  {:>6.0}%{:>7.0}%{failing:>10}   {}",
                key.to_ascii_uppercase(),
                pillar.before_score,
                pillar.after_score,
                score_rail(pillar.after_score / 100.0, RAIL_WIDTH),
            );
            let shift = pillar.after_score - pillar.before_score;
            if shift >= 1.0 {
                row.push_str(" ↑");
            } else if shift <= -1.0 {
                row.push_str(" ↓");
            }
            Some(row)
        })
        .collect()
}

// -------------------------------------------------------------- headline

fn header_line(recap: &PrRecap) -> String {
    let count = recap.scope.files_scored;
    format!(
        "◇  Reviewed {count} changed file{}  +{}/-{}",
        if count == 1 { "" } else { "s" },
        recap.scope.lines_added,
        recap.scope.lines_removed
    )
}

fn context_line(view: &RecapView<'_>) -> String {
    // A long branch name must not push COMPOSABLE off the line; the tail
    // of the branch is the informative part, so trim its head.
    let subject = view.recap.review.as_ref().map_or_else(
        || view.subject.clone(),
        |review| {
            format!(
                "{} {} → {}",
                view.subject,
                truncate_left(&review.head_ref, MAX_BRANCH_CHARS),
                review.base_ref
            )
        },
    );
    let mut parts = vec![subject];
    parts.extend(view.context.iter().cloned());
    format!("{}{}", parts.join(" · "), view.incomplete_note())
}

fn headline_line(view: &RecapView<'_>) -> String {
    let tally = &view.tally;
    let mut phrases = vec![format!("{} lost", tally.down), format!("{} up", tally.up)];
    if tally.new > 0 {
        phrases.push(if tally.new_medals.is_empty() {
            format!("{} new", tally.new)
        } else {
            format!("{} new ({})", tally.new, tally.new_medals)
        });
    }
    if tally.dipped > 0 {
        phrases.push(format!("{} scores dipped", tally.dipped));
    }
    if tally.cosmetic > 0 {
        phrases.push(format!("{} cosmetic", tally.cosmetic));
    }
    if tally.secure_lost > 0 {
        phrases.push(format!("{} SECURE lost", tally.secure_lost));
    }
    format!(
        "{} {}   {}",
        headline_mark(view.recap.headline),
        view.recap.headline.word(),
        phrases.join(" · ")
    )
}

// --------------------------------------------------------- scores table

fn scores_header() -> String {
    row("CHANGE", "FILE", "MEDAL", MATRIX_HEADER, "")
}

fn medal_changed(file: &FileRecap) -> bool {
    match (&file.medal_before, &file.medal_after) {
        (Some(before), Some(after)) => before.tier != after.tier,
        _ => false,
    }
}

/// Every non-child file that moved, plus split parents whose medal moved
/// (those appear in both tables — the split is one story, the lattice
/// move is another).
fn score_rows(recap: &PrRecap) -> Vec<&FileRecap> {
    let mut rows: Vec<&FileRecap> = recap
        .files
        .iter()
        .filter(|file| match &file.cluster {
            None => file.status != Headline::LateralMove,
            Some(member) => member.role == ClusterRole::Parent && medal_changed(file),
        })
        .collect();
    rows.sort_by_key(|file| {
        let lost = file.status == Headline::Regression;
        let secure = file.pillars.get("secure").is_some_and(PillarDelta::lost);
        (
            if lost {
                0
            } else if file.is_new() {
                2
            } else {
                1
            },
            usize::from(!(lost && secure)),
            file.status.rank(),
            file.path.clone(),
        )
    });
    rows
}

/// The tier word, and both tiers when the medal moved: `GOLD`,
/// `BRONZE → SILVER`, `unparsed`. Which pillars hold is the matrix's
/// job, not this column's.
fn medal_cell(file: &FileRecap) -> String {
    let Some(after) = &file.medal_after else {
        return "unparsed".to_string();
    };
    match &file.medal_before {
        Some(before) if before.tier != after.tier => {
            format!("{} → {}", before.tier, after.tier)
        }
        _ => after.tier.clone(),
    }
}

/// The four-pillar matrix of the *head* state, in simple, composable,
/// secure, navigable order: `●` passes, `○` fails, `·` not measured.
///
/// A modified file also gets a movement arrow when that pillar's
/// displayed score shifted by at least one point — `●↑` still passing
/// and better, `○↓` still failing and worse. An added file has no before
/// side, so it never carries an arrow.
fn pillar_matrix(file: Option<&FileRecap>) -> String {
    PILLARS
        .iter()
        .map(|key| pad(&matrix_cell(file, key), PILLAR_COL))
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn matrix_cell(file: Option<&FileRecap>, key: &str) -> String {
    let Some(delta) = file.and_then(|file| file.pillars.get(key)) else {
        return "·".to_string();
    };
    if !delta.measured || delta.after_passed.is_none() {
        return "·".to_string();
    }
    let dot = if delta.after_passed == Some(true) {
        '●'
    } else {
        '○'
    };
    if file.is_some_and(FileRecap::is_new) {
        return dot.to_string();
    }
    match delta.shift() {
        Some(shift) if shift >= 1.0 => format!("{dot}↑"),
        Some(shift) if shift <= -1.0 => format!("{dot}↓"),
        _ => dot.to_string(),
    }
}

fn percent(value: Option<f64>) -> String {
    value.map_or_else(|| "·".to_string(), |score| format!("{score:.0}%"))
}

/// One row per file. The matrix carries every pillar, so a file never
/// needs a continuation row; `--verbose` spells the moved scores out
/// underneath instead.
fn score_lines(file: &FileRecap, verbose: bool, options: RenderOptions) -> Vec<String> {
    let mut lines = vec![line(
        &row(
            change_word(file),
            &truncate_left(&file.path, FILE_WIDTH - 1),
            &medal_cell(file),
            &pillar_matrix(Some(file)),
            "",
        ),
        options,
    )];
    if !verbose {
        return lines;
    }
    for key in PILLARS {
        let Some(delta) = file.pillars.get(key) else {
            continue;
        };
        if delta.shift().is_none_or(|shift| shift.abs() < 1.0) {
            continue;
        }
        lines.push(dim_line(
            &format!(
                "{}{} {} → {}",
                " ".repeat(CHANGE_WIDTH),
                key.to_ascii_uppercase(),
                percent(delta.before_score),
                percent(delta.after_score)
            ),
            options,
        ));
    }
    lines
}

// ----------------------------------------------------------- held line

fn held_line(recap: &PrRecap, verbose: bool) -> Option<String> {
    let held: Vec<&str> = recap
        .unclustered_files()
        .into_iter()
        .filter(|file| file.status == Headline::LateralMove && !file.cosmetic)
        .map(|file| file.path.as_str())
        .collect();
    let deleted = &recap.deleted;
    if held.is_empty() && deleted.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !held.is_empty() {
        parts.push(if verbose {
            format!("{} held their medal: {}", held.len(), held.join(", "))
        } else if held.len() == 1 {
            "1 file held its medal".to_string()
        } else {
            format!("{} files held their medal", held.len())
        });
    }
    if !deleted.is_empty() {
        let names: Vec<&str> = deleted.iter().map(String::as_str).collect();
        parts.push(if verbose {
            format!("deleted, not scored: {}", name_list(&names))
        } else {
            format!("{} deleted, not scored", names.len())
        });
    }
    Some(format!("· {}", parts.join(" · ")))
}

fn name_list(names: &[&str]) -> String {
    if names.len() <= MAX_NAMES {
        return names.join(", ");
    }
    format!(
        "{}, +{} more",
        names[..MAX_NAMES].join(", "),
        names.len() - MAX_NAMES
    )
}

// ----------------------------------------------------------------- tips

/// The one or two lines printed under the card, `evaluate`'s grammar.
///
/// First match wins down the list, at most two; the `--json` pointer is
/// the fallback for a card that had nothing more useful to say.
pub(super) fn tips(recap: &PrRecap, verbose: bool) -> Vec<String> {
    let mut tips = Vec::new();
    if !recap.clusters.is_empty() && !verbose {
        tips.push(
            "Tip: add --verbose to list the functions that moved and each score that changed."
                .to_string(),
        );
    }
    let attention = score_rows(recap)
        .into_iter()
        .find(|file| matches!(change_word(file), "X LOST" | "! DOWN"))
        .map(|file| file.path.clone())
        .or_else(|| recap.hotspots.first().map(|spot| spot.path.clone()));
    if let Some(path) = attention {
        tips.push(format!(
            "Tip: run topos inspect {path} for the gate and the fix."
        ));
    }
    if !recap.scope.coupling.measured && recap.review.is_some() {
        tips.push(
            "Tip: install GitNexus (npm install -g gitnexus) to measure COMPOSABLE.".to_string(),
        );
    }
    tips.truncate(2);
    if tips.is_empty() {
        tips.push(
            "Tip: --json reproduces this document; --format github renders the PR comment."
                .to_string(),
        );
    }
    tips
}

#[cfg(test)]
mod tests {
    use super::{render_card, RenderOptions};
    use crate::commands::pr_recap::fixtures::{
        fixture_losses, fixture_mixed, fixture_plain, fixture_pr5,
    };

    fn options() -> RenderOptions {
        RenderOptions {
            styled: false,
            width: 100,
        }
    }

    fn card(recap: &super::PrRecap, verbose: bool) -> Vec<String> {
        render_card(recap, verbose, options())
    }

    #[test]
    fn pr5_card_leads_with_the_change_and_the_splits() {
        let lines = card(&fixture_pr5(), false);
        let text = lines.join("\n");
        assert!(text.contains("◇  Reviewed 23 changed files"), "{text}");
        assert!(text.contains("CHANGE       FILE"), "{text}");
        assert!(text.contains("SPLIT        PARENT → CHILDREN"), "{text}");
        assert!(text.contains("S   C   E   N"), "{text}");
        assert!(text.contains("✓ SPLIT"), "{text}");
        assert!(text.contains("├─ poll-shell-types.tsx"), "{text}");
        assert!(text.contains("└─ 3 more"), "{text}");
        assert!(text.contains("33→46 +39%"), "{text}");
        assert!(text.contains("BRONZE → SILVER"), "{text}");
        assert!(text.contains("· 1 file held its medal"), "{text}");
        assert!(text.contains("└  ✓ IMPROVEMENT"), "{text}");
    }

    /// The project table, the legend and the floor are `evaluate`'s, to
    /// the column and to the sentence.
    #[test]
    fn pr5_card_reads_like_an_evaluate_card() {
        let lines = card(&fixture_pr5(), false);
        let text = lines.join("\n");
        assert!(text.contains("priority secure"), "{text}");
        assert!(
            text.contains("PILLAR        STATUS   BEFORE   AFTER   FAILING   SCORE"),
            "{text}"
        );
        assert!(
            text.contains("SIMPLE        X FAIL      11%     38%    3 / 23   "),
            "{text}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains('━') && line.contains('◆')),
            "{text}"
        );
        assert!(
            text.contains("SECURE        ✓ PASS     100%    100%    0 / 23"),
            "{text}"
        );
        assert!(
            text.contains("🥉 BRONZE · SECURE · 41% → 58% average."),
            "{text}"
        );
        assert!(text.contains("average."), "{text}");
    }

    /// A pillar whose head score moved at least a point says so, and one
    /// that did not stays quiet.
    #[test]
    fn a_moved_pillar_score_carries_an_arrow() {
        let lines = card(&fixture_pr5(), false);
        let navigable = lines
            .iter()
            .find(|line| line.contains("NAVIGABLE     "))
            .expect("the project table has a NAVIGABLE row");
        assert!(navigable.ends_with('↑'), "{navigable}");
        let secure = lines
            .iter()
            .find(|line| line.contains("SECURE        "))
            .expect("the project table has a SECURE row");
        assert!(!secure.ends_with('↑') && !secure.ends_with('↓'), "{secure}");
    }

    /// The floor's hotspots read like `inspect`'s recommended changes,
    /// and advice is guidance, so it is never cut: it wraps instead.
    #[test]
    fn hotspot_advice_is_never_truncated() {
        let mut recap = fixture_mixed();
        let advice = "Extract the nested branch into a named helper so the function clears \
the SIMPLE gate here."
            .to_string();
        assert_eq!(advice.chars().count(), 90, "{advice}");
        recap.hotspots[0].advice = advice.clone();
        let narrow = RenderOptions {
            styled: false,
            width: 60,
        };
        let lines = render_card(&recap, false, narrow);
        let tail: Vec<&String> = lines
            .iter()
            .skip_while(|line| line.trim() != "Where to look")
            .collect();
        assert!(!tail.is_empty(), "{lines:?}");
        let rendered = tail
            .iter()
            .map(|line| line.trim().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(rendered.contains("Where to look"), "{rendered}");
        assert!(rendered.contains("1. X FIX · SIMPLE"), "{rendered}");
        assert!(
            tail.iter().any(|line| line.starts_with("     Do   ")),
            "{tail:?}"
        );
        assert!(!rendered.contains('…'), "{rendered}");
        for word in advice.split_whitespace() {
            assert!(rendered.contains(word), "{word} missing from {rendered}");
        }
        assert!(tail.len() >= 3, "{tail:?}");
        for line in lines {
            assert!(line.chars().count() <= 60, "{line}");
        }
    }

    #[test]
    fn tips_point_at_the_next_command() {
        let pr5 = super::tips(&fixture_pr5(), false);
        assert_eq!(
            pr5.first().map(String::as_str),
            Some(
                "Tip: add --verbose to list the functions that moved and each score that changed."
            )
        );
        let mixed = super::tips(&fixture_mixed(), true);
        assert!(
            mixed.iter().any(|tip| tip
                == "Tip: run topos inspect topos/mcp/src/tools/depgraph.rs for the gate and the fix."),
            "{mixed:?}"
        );
        let plain = super::tips(&fixture_plain(), true);
        assert_eq!(
            plain,
            vec!["Tip: install GitNexus (npm install -g gitnexus) to measure COMPOSABLE."]
        );
    }

    #[test]
    fn pr5_card_shows_the_pillar_matrix() {
        let text = card(&fixture_pr5(), false).join("\n");
        assert!(!text.contains("○○●○"), "{text}");
        assert!(text.contains("●↑"), "{text}");
        assert!(text.contains("○↓"), "{text}");
        let fold = card(&fixture_pr5(), false)
            .into_iter()
            .find(|line| line.contains("└─ 4 files"))
            .expect("create.ts folds every child");
        assert!(fold.contains("PLATINUM"), "{fold}");
        assert!(!fold.contains('●') && !fold.contains('○'), "{fold}");
    }

    #[test]
    fn pr5_card_stays_inside_the_terminal() {
        let lines = card(&fixture_pr5(), false);
        assert!(lines.len() <= 36, "{} lines", lines.len());
        for line in &lines {
            assert!(
                line.chars().count() <= 100,
                "{} cols: {line}",
                line.chars().count()
            );
        }
    }

    #[test]
    fn verbose_unfolds_and_names_the_moved_functions() {
        let text = card(&fixture_pr5(), true).join("\n");
        assert!(text.contains("buildModel"), "{text}");
        assert!(text.contains("├─ WeekGridLegend.tsx"), "{text}");
    }

    #[test]
    fn a_plain_edit_prints_no_split_table() {
        let text = card(&fixture_plain(), false).join("\n");
        assert!(!text.contains("SPLIT"), "{text}");
        assert!(text.contains("CHANGE       FILE"), "{text}");
    }

    #[test]
    fn a_mixed_change_prints_both_tables_and_every_row_word() {
        let lines = card(&fixture_mixed(), false);
        let text = lines.join("\n");
        assert!(text.contains("CHANGE       FILE"), "{text}");
        assert!(text.contains("SPLIT        PARENT → CHILDREN"), "{text}");
        assert!(text.contains("S   C   E   N"), "{text}");
        assert!(text.contains("X LOST"), "{text}");
        assert!(text.contains("! DOWN"), "{text}");
        assert!(text.contains("! COSMETIC"), "{text}");
        assert!(text.contains("✓ UP"), "{text}");
        assert!(text.contains("✓ NEW"), "{text}");
        for line in &lines {
            assert!(line.chars().count() <= 100, "{line}");
        }
    }

    #[test]
    fn verbose_spells_out_the_moved_scores() {
        let text = card(&fixture_mixed(), true).join("\n");
        assert!(text.contains("NAVIGABLE 77% → 74%"), "{text}");
        assert!(text.contains("SIMPLE 45% → 30%"), "{text}");
    }

    #[test]
    fn losses_sort_first_and_are_named_everywhere() {
        let lines = card(&fixture_losses(), false);
        let text = lines.join("\n");
        let first_row = lines
            .iter()
            .position(|line| line.contains("taint.rs"))
            .expect("the parent has a row");
        assert!(lines[first_row].contains("X LOST"), "{text}");
        assert!(text.contains("SECURE lost"), "{text}");
        assert!(text.contains("X SPLIT"), "{text}");
        assert!(text.contains("SLOP"), "{text}");
        assert!(text.contains("in, cx 12→19"), "{text}");
        assert!(text.contains("lost SECURE, SIMPLE"), "{text}");
    }

    #[test]
    fn no_color_output_carries_no_escapes() {
        for recap in [fixture_pr5(), fixture_mixed(), fixture_losses()] {
            for verbose in [false, true] {
                for line in card(&recap, verbose) {
                    assert!(!line.contains('\u{1b}'), "{line}");
                }
            }
        }
    }

    #[test]
    fn styled_output_paints_only_the_marks() {
        let styled = render_card(
            &fixture_pr5(),
            false,
            RenderOptions {
                styled: true,
                width: 100,
            },
        );
        let text = styled.join("\n");
        assert!(styled.iter().any(|line| line.contains('\u{1b}')));
        // Green ● dot for passed pillar, red ○ dot for failed pillar
        assert!(text.contains("\u{1b}[32m●\u{1b}[0m"), "{text}");
        assert!(text.contains("\u{1b}[31m○\u{1b}[0m"), "{text}");
        // Bold marks matching evaluate/summary conventions
        assert!(text.contains("\u{1b}[32m\u{1b}[1m✓\u{1b}[0m"), "{text}");
        assert!(text.contains("\u{1b}[31m\u{1b}[1mX\u{1b}[0m"), "{text}");
        // Bold dim table headers matching evaluate summary headers
        assert!(text.contains("\u{1b}[1m\u{1b}[2mPILLAR"), "{text}");
        assert!(text.contains("\u{1b}[1m\u{1b}[2mCHANGE"), "{text}");
        assert!(text.contains("\u{1b}[1m\u{1b}[2mSPLIT"), "{text}");
    }
}
