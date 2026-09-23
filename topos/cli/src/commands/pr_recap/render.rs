//! Primary terminal card for `topos pr-recap`, on a terminal and in a
//! pipe alike: only the color changes.
//!
//! The default card is `evaluate`'s shape: a title, the meta line, one
//! verdict line, the project pillar table, the numbered findings, the
//! gate settings in the legend slot, and the `└` footer with the
//! readiness mark. `--verbose` adds the **Changed files** table, the
//! splits table and the waived findings inside the frame; `--info`
//! appends `inspect`'s recommended changes after it.
//!
//! Nothing here computes a verdict. Every mark is a field of
//! [`PrRecap`]: `recap.readiness`, `finding.severity`, `file.severity`,
//! `cluster.mark`, `PillarDelta::lost`/`cleared`. The renderer only
//! chooses which already-decided fact is the most useful one to show,
//! and how to fold the rest away.

mod changed;
mod findings;
mod layout;
mod splits;

use console::Style;

use super::gates::Readiness;
use super::model::{FileRecap, PrRecap, ProjectRollup};
use super::view::{
    coupling_tip, idle_waiver_text, visible_shift, waived_text, Item, RecapView, PILLARS,
};
use crate::commands::evaluate::summary::{score_rail, status_text};
use crate::commands::render::{guide, paint, RenderOptions};
use layout::{dim_wrapped, header_dim_line, line};

// The GitHub comment states the same facts in Markdown, so it takes
// its segments, medals and sentences from here rather than restating them.
pub(super) use changed::{change_text, changed_rows, kept_count};
pub(super) use findings::{facts, gate_line, headline, severity_mark};

/// Columns of the project pillar table, `evaluate`'s exact shape.
const PILLAR_NAME_WIDTH: usize = 13;
const RAIL_WIDTH: usize = 10;
/// At most this many tips under the card.
const MAX_TIPS: usize = 2;

/// How much of the card to print beyond the default view.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Detail {
    /// The changed-files table, every split child, and the ledger.
    pub(super) verbose: bool,
    /// The recommended change for each finding, after the card.
    pub(super) info: bool,
}

/// Every line `pr-recap` prints for the card, the tips included.
pub(super) fn render_card(recap: &PrRecap, detail: Detail, options: RenderOptions) -> Vec<String> {
    let view = RecapView::new(recap);
    let items = view.items();
    let mut lines = vec![paint(title(&view), Style::new().bold(), options)];
    lines.extend(dim_wrapped("", "", &meta(&view), options));
    lines.push(guide('│', options));
    lines.extend(findings::verdict_lines(&view, &items, options));
    lines.push(guide('│', options));

    if let Some(project) = &recap.project {
        lines.push(header_dim_line(&pillar_table_header(), options));
        for text in pillar_table_rows(project) {
            lines.push(line(&text, options));
        }
        lines.push(guide('│', options));
    }

    if !items.is_empty() {
        lines.extend(findings::list_lines(&view, &items, options));
        lines.push(guide('│', options));
    }

    if detail.verbose {
        let changed = changed::changed_lines(recap, options);
        if !changed.is_empty() {
            lines.extend(changed);
            lines.push(guide('│', options));
        }
        if !view.clusters.is_empty() {
            lines.push(header_dim_line(&splits::splits_header(), options));
            for cluster in &view.clusters {
                lines.extend(splits::cluster_block(cluster, detail.verbose, options));
            }
            lines.push(guide('│', options));
        }
        let waived = waived_lines(&view, options);
        if !waived.is_empty() {
            lines.extend(waived);
            lines.push(guide('│', options));
        }
    }

    if let Some(note) = view.waiver_note() {
        lines.extend(dim_wrapped("", "", &note, options));
    }
    lines.extend(dim_wrapped("", "", &findings::gate_line(recap), options));
    lines.push(footer(recap, options));

    if detail.info && !items.is_empty() {
        lines.push(String::new());
        lines.extend(findings::recommendations(recap, &items, options));
    }
    let tips = tips(&view, &items, detail);
    if !tips.is_empty() {
        lines.push(String::new());
        for tip in tips {
            lines.push(paint(tip, Style::new().dim(), options));
        }
    }
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
            if visible_shift(shift) {
                row.push_str(if shift > 0.0 { " ↑" } else { " ↓" });
            }
            Some(row)
        })
        .collect()
}

// ------------------------------------------------------------ title, meta

/// `◇  Reviewed #359  fix/ts-parser-cpg-precision → main`, or the two
/// revisions when there is no pull request. Branch names are shown whole.
fn title(view: &RecapView<'_>) -> String {
    match &view.recap.review {
        Some(review) => format!(
            "◇  Reviewed {}  {} → {}",
            view.subject, review.head_ref, review.base_ref
        ),
        None => format!("◇  Reviewed {}", view.subject),
    }
}

/// `3 files · +174/-1 · priority navigable · COMPOSABLE not measured`.
fn meta(view: &RecapView<'_>) -> String {
    let recap = view.recap;
    let scope = &recap.scope;
    let mut parts = vec![
        format!(
            "{} file{}",
            scope.files_scored,
            if scope.files_scored == 1 { "" } else { "s" }
        ),
        format!("+{}/-{}", scope.lines_added, scope.lines_removed),
    ];
    parts.extend(view.context.iter().cloned());
    if !recap.deleted.is_empty() {
        parts.push(format!("{} deleted", recap.deleted.len()));
    }
    format!("{}{}", parts.join(" · "), view.incomplete_note())
}

// ------------------------------------------------------------ waivers

/// `--verbose`'s **WAIVED** block: each waived finding with its waiver's
/// reason, then the waivers that waived nothing.
fn waived_lines(view: &RecapView<'_>, options: RenderOptions) -> Vec<String> {
    let idle = view.idle_waivers();
    if view.waived.is_empty() && idle.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![header_dim_line("WAIVED", options)];
    for item in &view.waived {
        let lead = item.lead();
        let mut place = match (lead.path.as_str(), lead.line) {
            ("", _) => String::new(),
            (path, Some(line)) => format!("{path}:{line} · "),
            (path, None) => format!("{path} · "),
        };
        if let Some(function) = lead.function.as_deref().filter(|_| !place.is_empty()) {
            place.push_str(&format!("{function} · "));
        }
        lines.extend(dim_wrapped(
            "",
            "  ",
            &format!("{place}{}", waived_text(item)),
            options,
        ));
    }
    for waiver in idle {
        lines.extend(dim_wrapped("", "  ", &idle_waiver_text(waiver), options));
    }
    lines
}

// ------------------------------------------------------------------ footer

/// `└  X 🥇 GOLD → 🥉 BRONZE · SECURE · 87% → 70% average.`, `evaluate`'s
/// floor with the readiness mark: only the mark and the lattice name are
/// colored, and SLOP gets neither an emoji nor a lattice-name echo.
fn footer(recap: &PrRecap, options: RenderOptions) -> String {
    let readiness = recap.readiness;
    let mark = paint(readiness.mark(), readiness_style(readiness), options);
    let rest = match (&recap.project, &recap.added) {
        (Some(project), _) => medal_phrase(project, options),
        (None, Some(added)) => format!(
            "{} new file{} · {}",
            added.files,
            if added.files == 1 { "" } else { "s" },
            added.medal.tier
        ),
        (None, None) => readiness.word().to_string(),
    };
    format!("{}  {mark} {rest}", guide('└', options))
}

fn readiness_style(readiness: Readiness) -> Style {
    match readiness {
        Readiness::Ready => Style::new().green().bold(),
        Readiness::NeedsAttention => Style::new().yellow().bold(),
        Readiness::Blocked => Style::new().red().bold(),
    }
}

/// `🥉 BRONZE → 🥈 SILVER · SECURE_NAVIGABLE · 46% → 58% average.`, or one
/// medal when the tier held.
fn medal_phrase(project: &ProjectRollup, options: RenderOptions) -> String {
    let (before, after) = (&project.medal_before, &project.medal_after);
    let average = mean_scores(project).map_or_else(String::new, |(before, after)| {
        format!(" · {before:.0}% → {after:.0}% average.")
    });
    if after.tier == "SLOP" {
        let slop = paint("SLOP", Style::new().red().bold(), options);
        return format!("{slop}{average}");
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
    let lattice = paint(&after.verdict, Style::new().green().bold(), options);
    format!("{tiers} · {lattice}{average}")
}

/// The mean pillar score before and after, `None` with no pillar measured.
fn mean_scores(project: &ProjectRollup) -> Option<(f64, f64)> {
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

/// The *medal* the card shows for a file: the tier, both tiers when it
/// moved (`BRONZE → SILVER`), or `unparsed`.
pub(super) fn medal_cell(file: &FileRecap) -> String {
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

// ------------------------------------------------------------------- tips

/// At most two lines under the card, each pointing at the next level of
/// detail, `evaluate`'s grammar. The `--json` pointer is the fallback
/// for a card that had nothing more useful to say.
fn tips(view: &RecapView<'_>, items: &[Item<'_>], detail: Detail) -> Vec<String> {
    let recap = view.recap;
    let mut tips = Vec::new();
    let changed = changed_rows(recap).len();
    let plural = if changed == 1 { "" } else { "s" };
    if !detail.verbose {
        match recap.readiness {
            Readiness::Blocked => {
                let path = items.first().map(|item| item.lead().path.as_str());
                tips.push(match path.filter(|path| !path.is_empty()) {
                    Some(path) => {
                        format!("Tip: add --verbose for every file, or run topos inspect {path}.")
                    }
                    None => "Tip: add --verbose for every file.".to_string(),
                });
            }
            Readiness::NeedsAttention => {
                let strict = if recap.gate.fail_on == "block" {
                    "; --strict makes warnings fail the check"
                } else {
                    ""
                };
                tips.push(format!(
                    "Tip: add --verbose for all {changed} changed file{plural}{strict}."
                ));
            }
            Readiness::Ready if changed > 0 => tips.push(format!(
                "Tip: add --verbose to see the {changed} file{plural} whose scores moved."
            )),
            Readiness::Ready => {}
        }
    } else if !detail.info && !items.is_empty() {
        tips.push("Tip: add --info for the recommended change at each finding.".to_string());
    }
    if !detail.verbose && view.waiver_note().is_some() {
        tips.push("Tip: --verbose lists the waived findings with their reasons.".to_string());
    }
    tips.extend(coupling_tip(recap));
    tips.truncate(MAX_TIPS);
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
    use super::{render_card, Detail, RenderOptions};
    use crate::commands::pr_recap::fixtures::{
        fixture_lateral_loss, fixture_losses, fixture_mixed, fixture_plain, fixture_pr5,
        LATERAL_LOSS,
    };
    use crate::commands::pr_recap::gates::Readiness;
    use crate::commands::pr_recap::model::{CouplingReason, CouplingStatus, PrRecap};
    use topos_engine::config::Severity;

    const VERBOSE: Detail = Detail {
        verbose: true,
        info: false,
    };
    const INFO: Detail = Detail {
        verbose: false,
        info: true,
    };

    fn options() -> RenderOptions {
        RenderOptions {
            styled: false,
            width: 100,
        }
    }

    fn card(recap: &PrRecap, detail: Detail) -> Vec<String> {
        render_card(recap, detail, options())
    }

    /// `fixture_mixed` with its block finding dropped: only warnings
    /// remain, so the change needs attention and still passes.
    fn needs_attention() -> PrRecap {
        let mut recap = fixture_mixed();
        recap
            .findings
            .retain(|finding| finding.severity != Severity::Block);
        recap.readiness = Readiness::NeedsAttention;
        recap
    }

    /// The numbered finding rows, `│  1. X …`.
    fn numbered(lines: &[String]) -> Vec<&String> {
        lines
            .iter()
            .filter(|line| {
                line.trim_start_matches(['│', ' '])
                    .split_once(". ")
                    .is_some_and(|(n, _)| n.parse::<usize>().is_ok())
            })
            .collect()
    }

    #[test]
    fn a_blocked_change_names_its_worst_finding_first() {
        let text = card(&fixture_losses(), Detail::default()).join("\n");
        assert!(
            text.contains("│  X BLOCKED   taint.rs lost SIMPLE and SECURE (GOLD → BRONZE)"),
            "{text}"
        );
        assert!(
            text.contains("│  1. X topos/engine/src/functors/probes/cpg/taint.rs"),
            "{text}"
        );
        assert!(text.contains("│  gate: recommended\n"), "{text}");
        assert!(text.contains("└  X 🥇 GOLD → 🥈 SILVER"), "{text}");
        assert!(
            text.contains("Tip: add --verbose for every file, or run topos inspect"),
            "{text}"
        );
        assert!(!text.contains("CHANGED FILES"), "{text}");
        assert!(!text.contains("SPLIT        PARENT"), "{text}");
    }

    #[test]
    fn a_change_needing_attention_says_why_it_still_passes() {
        let lines = card(&needs_attention(), Detail::default());
        let text = lines.join("\n");
        assert!(
            text.contains("│  ! NEEDS ATTENTION   freshness.rs SIMPLE fell 45 → 30"),
            "{text}"
        );
        let rows = numbered(&lines);
        assert_eq!(rows.len(), 2, "{text}");
        assert!(rows.iter().all(|row| row.contains(". ! ")), "{text}");
        assert!(
            text.contains("│  gate: recommended · warnings don't fail the check"),
            "{text}"
        );
        assert!(text.contains("└  ! "), "{text}");
        assert!(
            text.contains("--strict makes warnings fail the check"),
            "{text}"
        );
    }

    #[test]
    fn a_ready_change_lists_nothing() {
        let lines = card(&fixture_pr5(), Detail::default());
        let text = lines.join("\n");
        assert!(
            text.contains("│  ✓ READY   no pillar or medal lost"),
            "{text}"
        );
        assert!(numbered(&lines).is_empty(), "{text}");
        assert!(text.contains("│  gate: recommended\n"), "{text}");
        assert!(!text.contains("warnings don't fail"), "{text}");
        assert!(
            text.contains("└  ✓ 🥉 BRONZE · SECURE · 41% → 58% average."),
            "{text}"
        );
        assert!(
            text.contains("Tip: add --verbose to see the 2 files whose scores moved."),
            "{text}"
        );
    }

    /// The project table is `evaluate`'s, to the column.
    #[test]
    fn the_pillar_table_reads_like_an_evaluate_card() {
        let text = card(&fixture_pr5(), Detail::default()).join("\n");
        assert!(text.contains("priority secure"), "{text}");
        assert!(
            text.contains("PILLAR        STATUS   BEFORE   AFTER   FAILING   SCORE"),
            "{text}"
        );
        assert!(
            text.contains("SIMPLE        X FAIL      11%     38%    3 / 23   ━━━◆────── ↑"),
            "{text}"
        );
        assert!(
            text.contains("SECURE        ✓ PASS     100%    100%    0 / 23"),
            "{text}"
        );
    }

    /// A pillar whose head score moved at least a point says so, and one
    /// that did not stays quiet.
    #[test]
    fn a_moved_pillar_score_carries_an_arrow() {
        let lines = card(&fixture_pr5(), Detail::default());
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

    #[test]
    fn the_list_stops_at_max_hotspots_and_counts_the_rest() {
        let full = card(&fixture_mixed(), Detail::default());
        assert_eq!(numbered(&full).len(), 3, "{full:#?}");

        let mut recap = fixture_mixed();
        recap.gate.max_hotspots = 1;
        let lines = card(&recap, Detail::default());
        let text = lines.join("\n");
        let rows = numbered(&lines);
        assert_eq!(rows.len(), 1, "{text}");
        assert!(rows[0].contains("1. X "), "{text}");
        assert!(text.contains("│       2 more"), "{text}");
    }

    /// Findings at the same place are one numbered row whose facts name
    /// every one of them.
    #[test]
    fn findings_at_one_place_merge_into_one_row() {
        let recap = fixture_losses();
        let at_taint = recap
            .findings
            .iter()
            .filter(|finding| finding.severity == Severity::Block)
            .filter(|finding| finding.path.ends_with("taint.rs"))
            .count();
        assert!(at_taint >= 2, "{:#?}", recap.findings);
        let lines = card(&recap, Detail::default());
        let text = lines.join("\n");
        assert_eq!(numbered(&lines).len(), 1, "{text}");
        assert!(text.contains("SIMPLE lost"), "{text}");
        assert!(text.contains("SECURE lost"), "{text}");
    }

    #[test]
    fn a_pillar_lost_while_another_is_gained_reads_as_a_trade() {
        let recap = fixture_lateral_loss();
        let text = card(&recap, Detail::default()).join("\n");
        assert!(
            text.contains("│  X BLOCKED   lattice.rs traded SIMPLE for NAVIGABLE"),
            "{text}"
        );
        assert!(
            text.contains(&format!("│  1. X {LATERAL_LOSS}  SIMPLE lost")),
            "{text}"
        );

        let verbose = card(&recap, VERBOSE).join("\n");
        let row = verbose
            .lines()
            .find(|line| line.contains("lattice.rs") && line.contains("SILVER"))
            .expect("the file has a CHANGED FILES row");
        assert!(
            row.contains("X SIMPLE lost · ✓ NAVIGABLE gained"),
            "{verbose}"
        );
    }

    #[test]
    fn verbose_lists_every_changed_file_and_its_medal() {
        let lines = card(&fixture_mixed(), VERBOSE);
        let text = lines.join("\n");
        assert!(text.contains("│  CHANGED FILES  topos/"), "{text}");
        assert!(text.contains("│  FILE  "), "{text}");
        let depgraph = lines
            .iter()
            .skip_while(|line| !line.contains("CHANGED FILES"))
            .find(|line| line.contains("tools/depgraph.rs") && line.contains("GOLD → SILVER"))
            .expect("the file that lost a medal has a row");
        assert!(depgraph.starts_with("│  X "), "{depgraph}");
        assert!(depgraph.ends_with("X SIMPLE lost"), "{depgraph}");
        assert!(
            lines
                .iter()
                .any(|line| line.contains("context_budget.rs") && line.ends_with("new")),
            "{text}"
        );
        assert!(text.contains("│  2 files kept their medal"), "{text}");
        assert!(text.contains("SPLIT        PARENT → CHILDREN"), "{text}");
        assert!(
            text.contains("Tip: add --info for the recommended change at each finding."),
            "{text}"
        );
    }

    /// Within a severity the table follows the finding list, so the
    /// largest drop leads rather than the first path.
    #[test]
    fn changed_files_follow_the_finding_order_within_a_severity() {
        let warned = |recap: &PrRecap| -> Vec<String> {
            super::changed_rows(recap)
                .into_iter()
                .filter(|file| file.severity == Some(Severity::Warn))
                .map(|file| file.path.clone())
                .collect()
        };
        let mut recap = fixture_mixed();
        recap
            .findings
            .sort_by(|a, b| b.severity.cmp(&a.severity).then(b.path.cmp(&a.path)));
        let descending = warned(&recap);
        assert!(descending.len() >= 2, "{descending:?}");
        assert!(descending.windows(2).all(|w| w[0] > w[1]), "{descending:?}");

        recap
            .findings
            .sort_by(|a, b| b.severity.cmp(&a.severity).then(a.path.cmp(&b.path)));
        let ascending = warned(&recap);
        assert!(ascending.windows(2).all(|w| w[0] < w[1]), "{ascending:?}");
    }

    #[test]
    fn a_plain_edit_prints_no_split_table() {
        let text = card(&fixture_plain(), VERBOSE).join("\n");
        assert!(text.contains("CHANGED FILES"), "{text}");
        assert!(!text.contains("SPLIT        PARENT"), "{text}");
    }

    #[test]
    fn info_appends_the_recommended_changes() {
        let recap = fixture_mixed();
        let plain = card(&recap, Detail::default()).join("\n");
        assert!(!plain.contains("Recommended changes"), "{plain}");

        let lines = card(&recap, INFO);
        let text = lines.join("\n");
        let at = lines
            .iter()
            .position(|line| line.trim() == "Recommended changes")
            .expect("--info appends the recommendations");
        let footer = lines
            .iter()
            .position(|line| line.starts_with('└'))
            .expect("the card closes");
        assert!(footer < at, "{text}");
        assert!(text.contains("1. X FIX · SIMPLE"), "{text}");
        assert!(text.contains("2. ~ IMPROVE · SIMPLE"), "{text}");
        assert!(text.contains("topos/mcp/src/tools/depgraph.rs"), "{text}");

        let ready = card(&fixture_pr5(), INFO).join("\n");
        assert!(!ready.contains("Recommended changes"), "{ready}");
    }

    /// `fixture_plain` with COMPOSABLE settled for `reason`.
    fn with_coupling(reason: CouplingReason, note: &str, estimate_ms: Option<u64>) -> PrRecap {
        let mut recap = fixture_plain();
        recap.scope.coupling = CouplingStatus {
            measured: matches!(reason, CouplingReason::Built | CouplingReason::Cached),
            note: note.to_string(),
            reason,
            estimate_ms,
        };
        recap
    }

    fn coupling_tips(recap: &PrRecap) -> Vec<String> {
        card(recap, Detail::default())
            .into_iter()
            .filter(|line| line.starts_with("Tip: ") && line.contains("oupling"))
            .collect()
    }

    #[test]
    fn a_declined_build_tips_the_yes_flag_with_its_wait() {
        let quoted = with_coupling(CouplingReason::Declined, "graphs not built", Some(25_000));
        assert_eq!(
            coupling_tips(&quoted),
            ["Tip: re-run with --yes to build the coupling graphs (~25 s once)."]
        );
        let unknown = with_coupling(CouplingReason::NotAsked, "graphs not built", None);
        assert_eq!(
            coupling_tips(&unknown),
            ["Tip: re-run with --yes to build the coupling graphs (usually 10–60 s once)."]
        );
        let text = card(&quoted, Detail::default()).join("\n");
        assert!(!text.contains("install GitNexus"), "{text}");
        assert!(
            text.contains("COMPOSABLE not measured (graphs not built)"),
            "{text}"
        );
    }

    #[test]
    fn a_missing_gitnexus_tips_the_install() {
        let recap = with_coupling(
            CouplingReason::GitnexusMissing,
            "gitnexus not installed (npm install -g gitnexus)",
            None,
        );
        let text = card(&recap, Detail::default()).join("\n");
        assert!(
            text.contains("Tip: install GitNexus (npm install -g gitnexus) to measure COMPOSABLE."),
            "{text}"
        );
    }

    #[test]
    fn a_failed_build_points_at_its_error() {
        let recap = with_coupling(
            CouplingReason::Error,
            "gitnexus analyze failed: out of memory\nat step 3\nat step 4",
            None,
        );
        assert_eq!(
            coupling_tips(&recap),
            ["Tip: building the coupling graphs failed (gitnexus analyze failed: out of memory); \
              --json has the full error."]
        );
        let text = card(&recap, Detail::default()).join("\n");
        assert!(
            text.contains("COMPOSABLE not measured (graph build failed)"),
            "{text}"
        );
        assert!(!text.contains("at step 3"), "{text}");
    }

    #[test]
    fn a_measured_skipped_or_prless_run_has_no_coupling_tip() {
        for (reason, note) in [
            (CouplingReason::Built, "built from /tmp/topos-pr-1"),
            (CouplingReason::Cached, "reused from /tmp/topos-pr-1"),
            (CouplingReason::Flag, "--no-coupling"),
            (
                CouplingReason::NoPr,
                "pass a pull request number to measure COMPOSABLE",
            ),
        ] {
            let recap = with_coupling(reason, note, None);
            let text = card(&recap, Detail::default()).join("\n");
            assert!(coupling_tips(&recap).is_empty(), "{reason:?}: {text}");
            assert!(!text.contains("install GitNexus"), "{reason:?}: {text}");
        }
    }

    fn every_card() -> Vec<(PrRecap, Detail)> {
        let details = [
            Detail::default(),
            VERBOSE,
            Detail {
                verbose: true,
                info: true,
            },
        ];
        [
            fixture_pr5(),
            fixture_plain(),
            fixture_lateral_loss(),
            fixture_mixed(),
            fixture_losses(),
            needs_attention(),
        ]
        .into_iter()
        .flat_map(|recap| details.map(|detail| (recap.clone(), detail)))
        .collect()
    }

    /// Everything but the tips, which stay whole so their commands paste.
    #[test]
    fn every_card_stays_inside_the_terminal() {
        for (recap, detail) in every_card() {
            for line in card(&recap, detail)
                .into_iter()
                .filter(|line| !line.starts_with("Tip: "))
            {
                assert!(line.chars().count() <= 100, "{line}");
            }
        }
    }

    #[test]
    fn no_color_output_carries_no_escapes() {
        for (recap, detail) in every_card() {
            for line in card(&recap, detail) {
                assert!(!line.contains('\u{1b}'), "{line}");
            }
        }
    }

    /// Color never moves a line: stripped of its escapes, the styled card
    /// is the plain one.
    #[test]
    fn styled_output_has_the_plain_line_structure() {
        let styled = RenderOptions {
            styled: true,
            width: 100,
        };
        for (recap, detail) in every_card() {
            let painted = render_card(&recap, detail, styled);
            assert!(painted.iter().any(|line| line.contains('\u{1b}')));
            let stripped: Vec<String> = painted
                .iter()
                .map(|line| console::strip_ansi_codes(line).into_owned())
                .collect();
            assert_eq!(stripped, card(&recap, detail));
        }
    }

    #[test]
    fn styled_output_paints_the_marks_and_headers() {
        let styled = RenderOptions {
            styled: true,
            width: 100,
        };
        let text = render_card(&fixture_mixed(), VERBOSE, styled).join("\n");
        assert!(text.contains("\u{1b}[31m\u{1b}[1mX\u{1b}[0m"), "{text}");
        assert!(text.contains("\u{1b}[1m\u{1b}[2mPILLAR"), "{text}");
        assert!(text.contains("\u{1b}[1m\u{1b}[2mFILE"), "{text}");
        assert!(text.contains("\u{1b}[1m\u{1b}[2mSPLIT"), "{text}");
    }
}
