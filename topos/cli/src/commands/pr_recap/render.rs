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

use std::collections::BTreeMap;

use console::Style;
use topos_engine::evaluation::policies::gates::pillar_for_metric;
use topos_engine::functors::profunctors::uast::ledger::MatchKind;
use topos_engine::graphs::mdg::split::Reach;

use super::model::{
    Cluster, ClusterChild, ClusterMark, ClusterRole, FileRecap, Headline, PillarDelta, PrRecap,
    ProjectRollup, CLUSTER_GROWTH_WARN,
};
use crate::commands::evaluate::summary::{score_rail, status_text};
use crate::commands::render::{
    guide, paint, truncate_left, truncate_right, wrap_text, RenderOptions,
};

/// Pillar keys, in `Generator::ALL` order, with their column labels.
pub(super) const PILLARS: [(&str, &str); 4] = [
    ("simple", "SIMP"),
    ("composable", "COMP"),
    ("secure", "SECU"),
    ("navigable", "NAVI"),
];

/// Lattice tiers, worst first, so "a medal went up" is subtraction.
const TIERS: [&str; 5] = ["SLOP", "BRONZE", "SILVER", "GOLD", "PLATINUM"];

const CHANGE_WIDTH: usize = 13;
const FILE_WIDTH: usize = 31;
const MEDAL_WIDTH: usize = 18;
/// One pillar cell: a dot plus an optional movement arrow.
const PILLAR_COL: usize = 4;
const MATRIX_WIDTH: usize = PILLAR_COL * 4;
const WORST_WIDTH: usize = 9;
/// Offset where the splits-only columns begin.
const TAIL: usize = CHANGE_WIDTH + FILE_WIDTH + MEDAL_WIDTH + MATRIX_WIDTH;
/// Content columns available after the `│  ` rail at the target width.
const CONTENT: usize = 97;
const MATRIX_HEADER: &str = "S   C   E   N";

/// The SIMPLE gate on `ast.max_function_complexity`. A child arriving
/// with a function above it is worth a row of its own.
const SIMPLE_GATE: usize = 10;
/// At most this many child rows per cluster before folding.
const MAX_CHILD_ROWS: usize = 3;
const MAX_LEDGER_LINES: usize = 20;
const MAX_NAMES: usize = 3;
/// `metric` of a hotspot that is a SECURE finding rather than a size one.
const SECURE_METRIC: &str = "cpg.dangerous_calls";
/// Columns of the project pillar table, `evaluate`'s exact shape.
const PILLAR_NAME_WIDTH: usize = 13;
const RAIL_WIDTH: usize = 10;
pub(super) fn render_card(recap: &PrRecap, verbose: bool, options: RenderOptions) -> Vec<String> {
    let mut lines = vec![
        paint(
            clamp(&header_line(recap), plain_budget(options)),
            Style::new().bold(),
            options,
        ),
        dim_line(&context_line(recap), options),
        guide('│', options),
        line(&headline_line(recap), options),
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

    if !recap.clusters.is_empty() {
        lines.push(header_dim_line(&splits_header(), options));
        for cluster in &recap.clusters {
            lines.extend(cluster_block(recap, cluster, verbose, options));
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
        colorize(&clamp(&floor_line(recap), budget(options)), options)
    ));
    for text in floor_blocks(recap, options) {
        lines.push(text);
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
        .filter_map(|(key, _)| {
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

// ---------------------------------------------------------------- layout

fn plain_budget(options: RenderOptions) -> usize {
    options.width.clamp(24, 100)
}

/// Content budget after the `│  ` rail.
fn budget(options: RenderOptions) -> usize {
    plain_budget(options) - 3
}

fn clamp(text: &str, width: usize) -> String {
    truncate_right(text, width)
}

fn line(content: &str, options: RenderOptions) -> String {
    format!(
        "{}  {}",
        guide('│', options),
        colorize(&clamp(content, budget(options)), options)
    )
}

fn header_dim_line(content: &str, options: RenderOptions) -> String {
    format!(
        "{}  {}",
        guide('│', options),
        paint(clamp(content, budget(options)), Style::new().bold().dim(), options)
    )
}

fn dim_line(content: &str, options: RenderOptions) -> String {
    format!(
        "{}  {}",
        guide('│', options),
        paint(clamp(content, budget(options)), Style::new().dim(), options)
    )
}

/// Pad to `width` columns. Every glyph used on the card is single-width,
/// so a char count is a column count. Content that overruns its column
/// gets a single separator space rather than colliding with the next
/// one; content that fills it exactly is already flush.
fn pad(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count > width {
        return format!("{text} ");
    }
    format!("{text}{}", " ".repeat(width - count))
}

/// `CHANGE | FILE | MEDAL | S C E N | <tail>`, trailing blanks trimmed.
fn row(change: &str, file: &str, medal: &str, matrix: &str, tail: &str) -> String {
    let mut out = pad(change, CHANGE_WIDTH);
    out.push_str(&pad(file, FILE_WIDTH));
    out.push_str(&pad(medal, MEDAL_WIDTH));
    out.push_str(&pad(matrix, MATRIX_WIDTH));
    out.push_str(tail);
    out.trim_end().to_string()
}

/// Paint the meaning-carrying glyphs, leaving everything else plain.
///
/// `X` is only painted when it stands alone, so a path or an identifier
/// containing an `X` is never mistaken for a failure mark.
pub(crate) fn colorize(content: &str, options: RenderOptions) -> String {
    if !options.styled {
        return content.to_string();
    }
    let chars: Vec<char> = content.chars().collect();
    let mut out = String::with_capacity(content.len());
    let mut index = 0;
    while index < chars.len() {
        let glyph = chars[index];
        if glyph.is_ascii_uppercase() {
            let mut end = index;
            while end < chars.len() && (chars[end].is_ascii_uppercase() || chars[end] == '_') {
                end += 1;
            }
            let word: String = chars[index..end].iter().collect();
            if word == "X" {
                // A lone `X` is the failure mark (`X FAIL`, `X LOST`), not a word.
                out.push_str(&paint("X", Style::new().red().bold(), options));
            } else if let Some(style) = tier_style(&word) {
                out.push_str(&paint(&word, style, options));
            } else {
                out.push_str(&word);
            }
            index = end;
            continue;
        }
        let free = |position: Option<&char>| position.is_none_or(|c| !c.is_alphanumeric());
        let standalone =
            free(index.checked_sub(1).and_then(|p| chars.get(p))) && free(chars.get(index + 1));
        match glyph {
            '✓' => out.push_str(&paint('✓', Style::new().green().bold(), options)),
            'X' if standalone => out.push_str(&paint('X', Style::new().red().bold(), options)),
            '!' => out.push_str(&paint('!', Style::new().yellow().bold(), options)),
            '~' if standalone => out.push_str(&paint('~', Style::new().yellow().bold(), options)),
            '·' => out.push_str(&paint('·', Style::new().dim(), options)),
            '●' => out.push_str(&paint('●', Style::new().green(), options)),
            '○' => out.push_str(&paint('○', Style::new().red(), options)),
            '↑' => out.push_str(&paint('↑', Style::new().green(), options)),
            '↓' => out.push_str(&paint('↓', Style::new().yellow(), options)),
            other => out.push(other),
        }
        index += 1;
    }
    out
}

/// A tier word carries its own verdict, so it is coloured wherever it
/// appears — medal cell, tally or floor. Everything else uppercase
/// (`IMPROVEMENT`, `NAVIGABLE`, `SPLIT`) is left alone.
fn tier_style(word: &str) -> Option<Style> {
    match word {
        "SLOP" => Some(Style::new().red().bold()),
        "GOLD" | "PLATINUM" | "IDEAL" => Some(Style::new().green()),
        "SIMPLE_COMPOSABLE"
        | "SIMPLE_SECURE"
        | "COMPOSABLE_SECURE"
        | "SIMPLE_COMPOSABLE_SECURE"
        | "SIMPLE_NAVIGABLE"
        | "COMPOSABLE_NAVIGABLE"
        | "SIMPLE_COMPOSABLE_NAVIGABLE"
        | "SECURE_NAVIGABLE"
        | "SIMPLE_SECURE_NAVIGABLE"
        | "COMPOSABLE_SECURE_NAVIGABLE" => Some(Style::new().green().bold()),
        _ => None,
    }
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

/// Longest branch name shown on the context line.
const MAX_BRANCH_CHARS: usize = 40;

fn context_line(recap: &PrRecap) -> String {
    let mut parts = Vec::new();
    if let Some(review) = &recap.review {
        // A long branch name must not push COMPOSABLE off the line; the
        // tail of the branch is the informative part, so trim its head.
        parts.push(format!(
            "#{} {} → {}",
            review.number,
            truncate_left(&review.head_ref, MAX_BRANCH_CHARS),
            review.base_ref
        ));
    } else {
        parts.push(format!(
            "{}…{}",
            short_rev(&recap.base),
            short_rev(&recap.head)
        ));
    }
    // Every file here is classified with `Priority::Secure`; saying so
    // is what makes the gates on this card comparable to `evaluate`'s.
    parts.push("priority secure".to_string());
    if recap.scope.coupling.measured {
        parts.push("COMPOSABLE measured".to_string());
    } else if recap.scope.coupling.note.is_empty() {
        parts.push("COMPOSABLE not measured".to_string());
    } else {
        parts.push(format!(
            "COMPOSABLE not measured ({})",
            recap.scope.coupling.note
        ));
    }
    if recap.scope.files_skipped > 0 {
        parts.push(format!("{} skipped", recap.scope.files_skipped));
    }
    if recap.scope.files_capped > 0 {
        parts.push(format!("{} over the file cap", recap.scope.files_capped));
    }
    parts.join(" · ")
}

pub(super) fn headline_mark(headline: Headline) -> char {
    match headline {
        Headline::Improvement | Headline::ImprovementScore => '✓',
        Headline::Regression
        | Headline::RegressionScore
        | Headline::SuspiciousNoStructuralChange => 'X',
        Headline::LateralMove => '·',
    }
}

/// `(up, down)` medal moves over files scored on both sides.
pub(super) fn medal_moves(recap: &PrRecap) -> (usize, usize) {
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
    (up, down)
}

/// `11 PLATINUM, 5 GOLD, 1 SILVER`, biggest group first.
pub(super) fn new_medal_tally(recap: &PrRecap) -> String {
    tally(
        recap
            .new_files()
            .into_iter()
            .filter_map(|file| file.medal_after.as_ref())
            .map(|medal| (medal.tier.clone(), medal.tier.clone())),
        false,
    )
}

/// `●●●● PLATINUM ×2, ○●●● GOLD` (multiply) or `11 PLATINUM, 5 GOLD`
/// (count). Entries are `(tier, label)`: the tier orders the groups, the
/// label is what the reader sees.
fn tally<I: Iterator<Item = (String, String)>>(entries: I, multiply: bool) -> String {
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

pub(super) fn secure_losses(recap: &PrRecap) -> usize {
    recap
        .files
        .iter()
        .filter(|file| file.pillars.get("secure").is_some_and(PillarDelta::lost))
        .count()
}

fn headline_line(recap: &PrRecap) -> String {
    let (up, down) = medal_moves(recap);
    let new = recap.new_files().len();
    let dipped = recap
        .files
        .iter()
        .filter(|file| file.status == Headline::RegressionScore)
        .count();
    let cosmetic = recap.files.iter().filter(|file| file.cosmetic).count();

    let mut phrases = vec![format!("{down} lost"), format!("{up} up")];
    if new > 0 {
        let tally = new_medal_tally(recap);
        phrases.push(if tally.is_empty() {
            format!("{new} new")
        } else {
            format!("{new} new ({tally})")
        });
    }
    if dipped > 0 {
        phrases.push(format!("{dipped} scores dipped"));
    }
    if cosmetic > 0 {
        phrases.push(format!("{cosmetic} cosmetic"));
    }
    let secure = secure_losses(recap);
    if secure > 0 {
        phrases.push(format!("{secure} SECURE lost"));
    }
    format!(
        "{} {}   {}",
        headline_mark(recap.headline),
        recap.headline.word(),
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

pub(super) fn change_word(file: &FileRecap) -> &'static str {
    if file.cosmetic {
        return "! COSMETIC";
    }
    if file.is_new() {
        return "✓ NEW";
    }
    match file.status {
        Headline::Regression => "X LOST",
        Headline::RegressionScore => "! DOWN",
        Headline::SuspiciousNoStructuralChange => "! SUSPECT",
        Headline::Improvement | Headline::ImprovementScore => "✓ UP",
        Headline::LateralMove => "· HELD",
    }
}

/// The tier word, and both tiers when the medal moved: `GOLD`,
/// `BRONZE → SILVER`, `unparsed`. Which pillars hold is the matrix's
/// job, not this column's.
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
        .map(|(key, _)| pad(&matrix_cell(file, key), PILLAR_COL))
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
            &short_path(file),
            &medal_cell(file),
            &pillar_matrix(Some(file)),
            "",
        ),
        options,
    )];
    if !verbose {
        return lines;
    }
    for (key, _) in PILLARS {
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

fn short_path(file: &FileRecap) -> String {
    truncate_left(&file.path, FILE_WIDTH - 1)
}

fn failing_names(file: &FileRecap) -> Vec<String> {
    PILLARS
        .iter()
        .filter(|(key, _)| {
            file.pillars
                .get(*key)
                .is_some_and(|delta| delta.after_passed == Some(false))
        })
        .map(|(key, _)| key.to_ascii_uppercase())
        .collect()
}

// --------------------------------------------------------- splits table

fn splits_header() -> String {
    row(
        "SPLIT",
        "PARENT → CHILDREN",
        "MEDAL",
        MATRIX_HEADER,
        &format!("{}DECISIONS", pad("WORST FN", WORST_WIDTH)),
    )
}

fn cluster_glyph(mark: ClusterMark) -> char {
    match mark {
        ClusterMark::Ok => '✓',
        ClusterMark::Warn => '!',
        ClusterMark::Fail => 'X',
    }
}

fn parent_file<'a>(recap: &'a PrRecap, cluster: &Cluster) -> Option<&'a FileRecap> {
    recap
        .files
        .iter()
        .find(|file| file.path == cluster.parent && file.cluster.is_some())
        .or_else(|| recap.files.iter().find(|file| file.path == cluster.parent))
}

fn cluster_block(
    recap: &PrRecap,
    cluster: &Cluster,
    verbose: bool,
    options: RenderOptions,
) -> Vec<String> {
    let mut lines = vec![line(&cluster_row(recap, cluster, verbose), options)];
    let (shown, hidden) = child_split(recap, cluster, verbose);
    for (index, child) in shown.iter().enumerate() {
        let last = hidden.is_empty() && index + 1 == shown.len();
        lines.push(line(
            &child_row(recap, cluster, child, if last { '└' } else { '├' }),
            options,
        ));
    }
    if !hidden.is_empty() {
        lines.push(line(&fold_row(recap, &hidden, shown.is_empty()), options));
    }
    if verbose {
        for text in ledger_lines(cluster) {
            lines.push(dim_line(
                &format!("{}{text}", " ".repeat(CHANGE_WIDTH)),
                options,
            ));
        }
    }
    lines
}

fn cluster_row(recap: &PrRecap, cluster: &Cluster, verbose: bool) -> String {
    let change = format!("{} SPLIT", cluster_glyph(cluster.mark));
    let parent = parent_file(recap, cluster);
    let medal = parent.map_or_else(|| "unparsed".to_string(), medal_cell);
    let worst = match (
        cluster.worst_function_before.as_ref(),
        cluster.worst_function_after.as_ref(),
    ) {
        (Some(before), Some(after)) => format!("{}→{}", before.complexity, after.complexity),
        (None, Some(after)) => format!("{}", after.complexity),
        _ => "·".to_string(),
    };
    let mut decisions = format!("{}→{}", cluster.decisions_before, cluster.decisions_after);
    if cluster.decisions_after > cluster.decisions_before && cluster.decisions_before > 0 {
        let grown = cluster.decisions_after - cluster.decisions_before;
        #[expect(clippy::cast_precision_loss, reason = "decision counts are small")]
        let fraction = grown as f64 / cluster.decisions_before as f64;
        if fraction > CLUSTER_GROWTH_WARN {
            decisions.push_str(&format!(" +{}%", grown * 100 / cluster.decisions_before));
        }
    }
    if verbose {
        if let Some(removed) = cluster
            .ledger
            .as_ref()
            .map(|ledger| ledger.totals.removed)
            .filter(|removed| *removed > 0)
        {
            decisions.push_str(&format!(" · {removed} removed"));
        }
    }
    row(
        &change,
        &truncate_left(&cluster.parent, FILE_WIDTH - 1),
        &medal,
        &pillar_matrix(parent),
        &format!("{}{decisions}", pad(&worst, WORST_WIDTH)),
    )
}

fn file_at<'a>(recap: &'a PrRecap, path: &str) -> Option<&'a FileRecap> {
    recap.files.iter().find(|file| file.path == path)
}

/// A child earns its own row when it says something the fold cannot:
/// it is shared beyond its parent, it carries symbols that used to live
/// in the parent, it landed below GOLD, it is cosmetic, or it arrived
/// with a function above the SIMPLE gate.
fn notable(recap: &PrRecap, child: &ClusterChild) -> bool {
    let file = file_at(recap, &child.path);
    let low = file
        .and_then(|file| file.medal_after.as_ref())
        .is_some_and(|medal| tier_rank(&medal.tier) < tier_rank("GOLD"));
    let heavy = file
        .and_then(|file| file.worst_function_after.as_ref())
        .is_some_and(|worst| worst.complexity > SIMPLE_GATE);
    child.reach == Some(Reach::Shared)
        || child.moved_in > 0
        || low
        || heavy
        || file.is_some_and(|file| file.cosmetic)
}

fn child_split<'a>(
    recap: &'a PrRecap,
    cluster: &'a Cluster,
    verbose: bool,
) -> (Vec<&'a ClusterChild>, Vec<&'a ClusterChild>) {
    if verbose {
        return (cluster.children.iter().collect(), Vec::new());
    }
    let mut shown = Vec::new();
    let mut hidden = Vec::new();
    for child in &cluster.children {
        if notable(recap, child) && shown.len() < MAX_CHILD_ROWS {
            shown.push(child);
        } else {
            hidden.push(child);
        }
    }
    (shown, hidden)
}

fn child_row(recap: &PrRecap, cluster: &Cluster, child: &ClusterChild, connector: char) -> String {
    let file = file_at(recap, &child.path);
    let name = truncate_left(basename(&child.path), FILE_WIDTH - 4);
    let medal = file.map_or_else(|| "unparsed".to_string(), medal_cell);
    row(
        "",
        &format!("{connector}─ {name}"),
        &medal,
        &pillar_matrix(file),
        &child_fact(cluster, child, file, TAIL),
    )
}

/// Fixed priority, greedily fitted: a move-in fact beats sharing, and
/// sharing beats a bare worst-fn count, which only appears when nothing
/// else applies. The row may use the full remaining width up to the
/// line budget.
fn child_fact(
    cluster: &Cluster,
    child: &ClusterChild,
    file: Option<&FileRecap>,
    offset: usize,
) -> String {
    let mut facts: Vec<String> = Vec::new();
    if let Some(fact) = moved_fact(cluster, child) {
        facts.push(fact);
    }
    if child.reach == Some(Reach::Shared) {
        facts.push(format!("shared ×{}", child.importers.len()));
    }
    if facts.is_empty() {
        if let Some(worst) = file.and_then(|file| file.worst_function_after.as_ref()) {
            if worst.complexity > SIMPLE_GATE {
                facts.push(format!("worst fn {}", worst.complexity));
            }
        }
    }
    let room = CONTENT.saturating_sub(offset);
    let mut out = String::new();
    for (index, fact) in facts.into_iter().enumerate() {
        let mut candidate = if out.is_empty() {
            fact
        } else {
            format!("{out} · {fact}")
        };
        // A long named arrival (`BookingCreationError.constructor in`) must
        // not silence the row: fall back to the bare count so the reader
        // still learns that code moved here.
        if index == 0 && candidate.chars().count() > room && child.moved_in > 0 {
            candidate = format!("{} in", child.moved_in);
        }
        if candidate.chars().count() > room {
            break;
        }
        out = candidate;
    }
    out
}

/// A moved/renamed match that got *more* complex on the way beats a
/// quiet one; a single named, non-nested arrival beats a bare count.
fn moved_fact(cluster: &Cluster, child: &ClusterChild) -> Option<String> {
    if child.moved_in == 0 {
        return None;
    }
    struct Moved<'a> {
        name: &'a str,
        delta: i64,
        before: usize,
        after: usize,
        kind: MatchKind,
        named: bool,
    }
    let moved: Vec<Moved> = cluster
        .ledger
        .iter()
        .flat_map(|ledger| &ledger.matches)
        .filter_map(|entry| {
            if !matches!(
                entry.kind,
                MatchKind::MovedIdentical | MatchKind::MovedModified | MatchKind::Renamed
            ) {
                return None;
            }
            let after = entry.after.as_ref()?;
            if after.file != child.path {
                return None;
            }
            let before = entry.before.as_ref();
            let nested = after.nested || before.is_some_and(|snapshot| snapshot.nested);
            let anonymous = after.name.starts_with("<anonymous>")
                || before.is_some_and(|snapshot| snapshot.name.starts_with("<anonymous>"));
            Some(Moved {
                name: after.qualified_name.as_str(),
                delta: entry.complexity_delta,
                before: before.map_or(0, |snapshot| snapshot.complexity),
                after: after.complexity,
                kind: entry.kind,
                named: !nested && !anonymous,
            })
        })
        .collect();
    if let Some(growth) = moved.iter().find(|entry| {
        matches!(entry.kind, MatchKind::MovedModified | MatchKind::Renamed) && entry.delta > 0
    }) {
        return Some(format!(
            "{} in, cx {}→{}",
            truncate_right(growth.name, 24),
            growth.before,
            growth.after
        ));
    }
    let named: Vec<&Moved> = moved.iter().filter(|entry| entry.named).collect();
    if let [only] = named.as_slice() {
        return Some(format!("{} in", truncate_right(only.name, 24)));
    }
    Some(format!("{} in", child.moved_in))
}

fn fold_row(recap: &PrRecap, hidden: &[&ClusterChild], all: bool) -> String {
    let subject = format!("└─ {} {}", hidden.len(), if all { "files" } else { "more" });
    // No dots on a fold row: the group is summarised, and a signature
    // that belonged to one of several files would be a lie. The tally is
    // indented to sit under the tier words of the rows above it.
    let medal = tally(
        hidden.iter().map(|child| {
            let tier = file_at(recap, &child.path)
                .and_then(|file| file.medal_after.as_ref())
                .map_or_else(|| "unparsed".to_string(), |medal| medal.tier.clone());
            (tier.clone(), tier)
        }),
        true,
    );
    row("", &subject, &medal, "", "")
}

// ----------------------------------------------------------- held line

fn held_files(recap: &PrRecap) -> Vec<&FileRecap> {
    recap
        .unclustered_files()
        .into_iter()
        .filter(|file| file.status == Headline::LateralMove && !file.cosmetic)
        .collect()
}

fn held_line(recap: &PrRecap, verbose: bool) -> Option<String> {
    let held = held_files(recap);
    let deleted = &recap.deleted;
    if held.is_empty() && deleted.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !held.is_empty() {
        let names: Vec<&str> = held.iter().map(|file| file.path.as_str()).collect();
        parts.push(if verbose {
            format!("{} held their medal: {}", names.len(), names.join(", "))
        } else if names.len() == 1 {
            "1 file held its medal".to_string()
        } else {
            format!("{} files held their medal", names.len())
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

pub(super) fn name_list(names: &[&str]) -> String {
    if names.len() <= MAX_NAMES {
        return names.join(", ");
    }
    format!(
        "{}, +{} more",
        names[..MAX_NAMES].join(", "),
        names.len() - MAX_NAMES
    )
}

// --------------------------------------------------------------- ledger

fn ledger_kind(kind: MatchKind) -> Option<(&'static str, u8)> {
    match kind {
        MatchKind::InPlace => None,
        MatchKind::MovedIdentical => Some(("moved", 0)),
        MatchKind::MovedModified => Some(("moved+edited", 1)),
        MatchKind::Renamed => Some(("renamed", 2)),
        MatchKind::New => Some(("new", 3)),
        MatchKind::Removed => Some(("removed", 4)),
    }
}

fn ledger_lines(cluster: &Cluster) -> Vec<String> {
    let Some(ledger) = &cluster.ledger else {
        return Vec::new();
    };
    let mut rows: Vec<(u8, String, String)> = Vec::new();
    let mut hidden_moved = 0usize;
    for entry in &ledger.matches {
        let Some((word, rank)) = ledger_kind(entry.kind) else {
            continue;
        };
        let before = entry.before.as_ref();
        let after = entry.after.as_ref();
        let nested = after.is_some_and(|snapshot| snapshot.nested)
            || before.is_some_and(|snapshot| snapshot.nested);
        let anonymous = after.is_some_and(|snapshot| snapshot.name.starts_with("<anonymous>"))
            || before.is_some_and(|snapshot| snapshot.name.starts_with("<anonymous>"));
        if nested || anonymous {
            if matches!(
                entry.kind,
                MatchKind::MovedIdentical | MatchKind::MovedModified
            ) {
                hidden_moved += 1;
            }
            continue;
        }
        let name = after
            .or(before)
            .map_or_else(String::new, |snapshot| snapshot.qualified_name.clone());
        let text = match entry.kind {
            MatchKind::MovedIdentical | MatchKind::MovedModified => format!(
                "{word}  {name}  {} → {}  cx {}→{}",
                before.map_or("?", |snapshot| basename(&snapshot.file)),
                after.map_or("?", |snapshot| basename(&snapshot.file)),
                before.map_or(0, |snapshot| snapshot.complexity),
                after.map_or(0, |snapshot| snapshot.complexity)
            ),
            MatchKind::Renamed => format!(
                "{word} {} → {}  cx {}→{}",
                before.map_or("?", |snapshot| snapshot.qualified_name.as_str()),
                after.map_or("?", |snapshot| snapshot.qualified_name.as_str()),
                before.map_or(0, |snapshot| snapshot.complexity),
                after.map_or(0, |snapshot| snapshot.complexity)
            ),
            MatchKind::New => format!(
                "{word}  {name}  {}  cx {}",
                after.map_or("?", |snapshot| basename(&snapshot.file)),
                after.map_or(0, |snapshot| snapshot.complexity)
            ),
            MatchKind::Removed => format!(
                "{word}  {name}  {}  cx {}",
                before.map_or("?", |snapshot| basename(&snapshot.file)),
                before.map_or(0, |snapshot| snapshot.complexity)
            ),
            MatchKind::InPlace => unreachable!("filtered by ledger_kind"),
        };
        rows.push((rank, name, text));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let total = rows.len();
    let mut lines: Vec<String> = rows
        .into_iter()
        .take(MAX_LEDGER_LINES)
        .map(|(_, _, text)| text)
        .collect();
    if total > MAX_LEDGER_LINES {
        lines.push(format!("… {} more", total - MAX_LEDGER_LINES));
    }
    if hidden_moved > 0 {
        lines.push(format!(
            "… {hidden_moved} anonymous callbacks moved with them"
        ));
    }
    lines
}

// ----------------------------------------------------------------- floor

pub(super) fn worst_function_drop(recap: &PrRecap) -> Option<(usize, usize)> {
    let drops: Vec<usize> = recap
        .clusters
        .iter()
        .filter_map(|cluster| {
            let before = cluster.worst_function_before.as_ref()?.complexity;
            let after = cluster.worst_function_after.as_ref()?.complexity;
            (before > after && before > 0).then(|| (before - after) * 100 / before)
        })
        .collect();
    Some((*drops.iter().min()?, *drops.iter().max()?))
}

pub(super) fn cluster_decisions(recap: &PrRecap) -> (usize, usize) {
    recap
        .clusters
        .iter()
        .fold((0, 0), |(before, after), cluster| {
            (
                before + cluster.decisions_before,
                after + cluster.decisions_after,
            )
        })
}

/// SECURE first: a security gate that stopped holding is the one a
/// reviewer must see before anything else.
pub(super) fn regressed_pillars(project: &ProjectRollup) -> Vec<String> {
    let mut keys: Vec<&(&str, &str)> = PILLARS.iter().collect();
    keys.sort_by_key(|(key, _)| usize::from(*key != "secure"));
    keys.into_iter()
        .filter(|(key, _)| {
            project
                .pillars
                .get(*key)
                .is_some_and(|pillar| pillar.before_passed && !pillar.after_passed)
        })
        .map(|(key, _)| key.to_ascii_uppercase())
        .collect()
}

fn lost_files(recap: &PrRecap) -> Vec<&FileRecap> {
    recap
        .files
        .iter()
        .filter(|file| file.status == Headline::Regression)
        .collect()
}

/// `🥉 BRONZE → 🥈 SILVER · SECURE_NAVIGABLE`, or one medal when the tier
/// held. SLOP is already the failure tier, so it gets no emoji and no
/// second lattice-name echo — exactly what `evaluate`'s floor does.
pub(super) fn medal_phrase(project: &ProjectRollup) -> String {
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

pub(super) fn mean_scores(project: &ProjectRollup) -> Option<(f64, f64)> {
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

/// `· LATERAL · 🥉 BRONZE · SECURE · 46% → 58% average.`
pub(super) fn floor_line(recap: &PrRecap) -> String {
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
    }
    parts.join(" · ")
}

/// The sentences the floor used to carry: what the split did to the
/// worst functions, which pillar the touched set lost, which file lost it.
fn detail_lines(recap: &PrRecap) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(project) = &recap.project {
        if project.regression {
            let pillars = regressed_pillars(project);
            if !pillars.is_empty() {
                parts.push(format!(
                    "X lost {} across the touched files",
                    pillars.join(", ")
                ));
            }
        }
    }
    if let Some(file) = lost_files(recap).first() {
        let pillars = PILLARS
            .iter()
            .filter(|(key, _)| file.pillars.get(*key).is_some_and(PillarDelta::lost))
            .map(|(key, _)| key.to_ascii_uppercase())
            .collect::<Vec<_>>();
        if !pillars.is_empty() {
            parts.push(format!("{} lost {}", file.path, pillars.join(", ")));
        }
    } else if !recap.clusters.is_empty() {
        let (before, after) = cluster_decisions(recap);
        if let Some((low, high)) = worst_function_drop(recap) {
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
    parts
}

/// Parents that still fail pillars, grouped by the exact set they fail;
/// the largest group is the one worth a line.
pub(super) fn still_failing(recap: &PrRecap) -> Option<(Vec<String>, Vec<String>)> {
    let mut groups: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    for cluster in &recap.clusters {
        let Some(file) = parent_file(recap, cluster) else {
            continue;
        };
        let failing = failing_names(file);
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

fn continuation_lines(recap: &PrRecap) -> Vec<String> {
    let mut lines = detail_lines(recap);
    let cosmetic: Vec<&str> = recap
        .files
        .iter()
        .filter(|file| file.cosmetic)
        .map(|file| file.path.as_str())
        .collect();
    if !cosmetic.is_empty() {
        lines.push(format!("! cosmetic: {}", name_list(&cosmetic)));
    }
    lines.truncate(2);
    lines
}

/// The blocks under the floor line, in `inspect`'s grammar: the `Why`
/// sentences the floor used to carry, then `Where to look` — one
/// numbered item per hotspot, SECURE findings first.
///
/// A hotspot is where the reviewer should look, so its advice is never
/// folded away and never cut: it wraps, aligned under the label, the
/// way `inspect` wraps an interpretation. An empty string is a blank
/// separator line, printed without the card's guide rail.
fn floor_blocks(recap: &PrRecap, options: RenderOptions) -> Vec<String> {
    let width = budget(options);
    let mut lines = Vec::new();
    let details = continuation_lines(recap);
    if !details.is_empty() {
        lines.push(String::new());
        for (index, detail) in details.iter().enumerate() {
            let label = if index == 0 { "  Why  " } else { "       " };
            push_wrapped(&mut lines, label, "       ", detail, width);
        }
    }

    let mut spots: Vec<&super::model::Hotspot> = recap.hotspots.iter().collect();
    spots.sort_by_key(|spot| usize::from(spot.metric != SECURE_METRIC));
    if !spots.is_empty() {
        lines.push(String::new());
        lines.push(paint("  Where to look", Style::new().cyan().bold(), options));
        for (index, spot) in spots.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!(
                "  {}. {} FIX · {}",
                index + 1,
                paint("X", Style::new().red().bold(), options),
                pillar_for_metric(&spot.metric).to_ascii_uppercase()
            ));
            push_wrapped(&mut lines, "     Why  ", "          ", &spot.detail, width);
            push_wrapped(&mut lines, "     Do   ", "          ", &spot.advice, width);
            let location = format!("{}:{}", spot.path, spot.line);
            lines.push(format!("     {}", truncate_left(&location, width - 5)));
        }
    }
    lines
}

/// Wrap `text` under `label`, with every line after the first aligned
/// under the first line's text rather than under the label.
fn push_wrapped(
    lines: &mut Vec<String>,
    label: &str,
    continuation: &str,
    text: &str,
    width: usize,
) {
    let available = width.saturating_sub(label.chars().count()).max(12);
    for (index, chunk) in wrap_text(text, available).into_iter().enumerate() {
        let prefix = if index == 0 { label } else { continuation };
        lines.push(format!("{prefix}{chunk}"));
    }
}

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

// ---------------------------------------------------------------- shared

pub(super) fn tier_rank(tier: &str) -> usize {
    TIERS.iter().position(|known| *known == tier).unwrap_or(0)
}

pub(super) fn pillar_cell(delta: Option<&PillarDelta>) -> &'static str {
    let Some(delta) = delta else { return "·" };
    if !delta.measured || delta.after_passed.is_none() {
        return "·";
    }
    if delta.lost() {
        return "✓→X";
    }
    if delta.cleared() {
        return "X→✓";
    }
    if delta.after_passed == Some(true) {
        "✓"
    } else {
        "X"
    }
}

pub(super) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[allow(dead_code)]
pub(super) fn stem(path: &str) -> &str {
    let name = basename(path);
    name.split('.').next().unwrap_or(name)
}

pub(super) fn short_rev(rev: &str) -> &str {
    if rev.len() >= 7 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return &rev[..7];
    }
    rev
}

#[cfg(test)]
pub(super) fn medal_for(tier: &str) -> super::model::Medal {
    let symbol = match tier {
        "PLATINUM" => "🏆",
        "GOLD" => "🥇",
        "SILVER" => "🥈",
        "BRONZE" => "🥉",
        _ => "⚠",
    };
    super::model::Medal {
        symbol: symbol.to_string(),
        tier: tier.to_string(),
        verdict: match tier {
            "PLATINUM" => "SIMPLE_COMPOSABLE_SECURE_NAVIGABLE",
            "GOLD" => "COMPOSABLE_SECURE_NAVIGABLE",
            "SILVER" => "SECURE_NAVIGABLE",
            "BRONZE" => "SECURE",
            _ => "NONE",
        }
        .to_string(),
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------
// Hand-built documents, shared with `compact.rs` and `github.rs` so all
// three renderers are asserted against exactly the same numbers. The
// shape and the counts are PR #5 from
// `docs/decisions/pr-recap-refactor-tracing.md`.

#[cfg(test)]
use super::model::{
    ClusterMembership, CouplingStatus, FileChange, FunctionRef, Hotspot, PillarRollup,
    ProjectRollup as Rollup, PullRequest, Scope, SCHEMA,
};
#[cfg(test)]
use topos_engine::functors::profunctors::uast::ledger::{
    FunctionMatch, FunctionSnapshot, Ledger, LedgerTotals,
};
#[cfg(test)]
use topos_engine::graphs::uast::models::{NativeRef, SourceSpan, UASTNode};

#[cfg(test)]
pub(super) struct Spec<'a> {
    pub(super) path: &'a str,
    pub(super) change: FileChange,
    pub(super) status: Headline,
    pub(super) before: Option<&'a str>,
    pub(super) after: &'a str,
    /// `(before_passed, after_passed, before_score, after_score)` in
    /// simple, composable, secure, navigable order.
    pub(super) pillars: [(bool, bool, f64, f64); 4],
    pub(super) worst: (usize, usize),
    pub(super) decisions: (usize, usize),
    pub(super) cluster: Option<(&'a str, ClusterRole)>,
    pub(super) cosmetic: bool,
}

#[cfg(test)]
pub(super) fn build(spec: Spec<'_>) -> FileRecap {
    let is_new = spec.change == FileChange::Added;
    let pillars = PILLARS
        .iter()
        .zip(spec.pillars)
        .map(|((key, _), (before, after, before_score, after_score))| {
            (
                (*key).to_string(),
                PillarDelta {
                    measured: true,
                    before_passed: (!is_new).then_some(before),
                    after_passed: Some(after),
                    before_score: (!is_new).then_some(before_score),
                    after_score: Some(after_score),
                    lost_gate: None,
                },
            )
        })
        .collect();
    FileRecap {
        path: spec.path.to_string(),
        change: spec.change,
        status: spec.status,
        lines_before: 100,
        lines_after: 120,
        lines_added: 40,
        lines_removed: 20,
        medal_before: spec.before.map(medal_for),
        medal_after: Some(medal_for(spec.after)),
        pillars,
        structural_distance: Some(0.4),
        cosmetic: spec.cosmetic,
        complexity_relocated_within_file: false,
        worst_function_before: (!is_new).then(|| function("worstBefore", spec.worst.0)),
        worst_function_after: Some(function("worstAfter", spec.worst.1)),
        decisions_before: (!is_new).then_some(spec.decisions.0),
        decisions_after: Some(spec.decisions.1),
        fan_in_before: Some(1),
        fan_in_after: Some(2),
        fan_out_before: Some(3),
        fan_out_after: Some(4),
        cluster: spec.cluster.map(|(parent, role)| ClusterMembership {
            parent: parent.to_string(),
            role,
        }),
        hotspots: Vec::new(),
    }
}

#[cfg(test)]
fn function(name: &str, complexity: usize) -> FunctionRef {
    FunctionRef {
        name: name.to_string(),
        line: 42,
        complexity,
    }
}

#[cfg(test)]
fn child(path: &str, reach: Reach, importers: usize, moved_in: usize) -> ClusterChild {
    ClusterChild {
        path: path.to_string(),
        reach: Some(reach),
        importers: (0..importers)
            .map(|index| format!("caller{index}.ts"))
            .collect(),
        moved_in,
    }
}

#[cfg(test)]
fn snapshot(file: &str, name: &str, complexity: usize) -> FunctionSnapshot {
    FunctionSnapshot {
        file: file.to_string(),
        name: name.to_string(),
        qualified_name: name.to_string(),
        kind: "Function".to_string(),
        start_line: 1,
        end_line: 20,
        complexity,
        nested: false,
        structural_hash: 7,
        node: UASTNode {
            kind: "FunctionDecl".to_string(),
            lang: "typescript".to_string(),
            span: SourceSpan {
                file: None,
                start_byte: 0,
                end_byte: 0,
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 0,
            },
            native: NativeRef {
                parser: "tree-sitter".to_string(),
                parser_version: "0.22".to_string(),
                node_kind: "function_declaration".to_string(),
            },
            attributes: std::collections::HashMap::new(),
            children: Vec::new(),
            id: String::new(),
        },
    }
}

#[cfg(test)]
fn moved(from: &str, to: &str, name: &str, before: usize, after: usize) -> FunctionMatch {
    #[expect(clippy::cast_possible_wrap, reason = "fixture complexities are tiny")]
    let delta = after as i64 - before as i64;
    FunctionMatch {
        kind: if delta == 0 {
            MatchKind::MovedIdentical
        } else {
            MatchKind::MovedModified
        },
        before: Some(snapshot(from, name, before)),
        after: Some(snapshot(to, name, after)),
        similarity: 1.0,
        complexity_delta: delta,
    }
}

#[cfg(test)]
fn ledger(matches: Vec<FunctionMatch>) -> Ledger {
    Ledger {
        matches,
        totals: LedgerTotals {
            before_total: 83,
            after_total: 87,
            new_logic: 4,
            balanced: true,
            ..LedgerTotals::default()
        },
    }
}

#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "a fixture cluster has many facts"
)]
fn cluster_of(
    parent: &str,
    children: Vec<ClusterChild>,
    mark: ClusterMark,
    reasons: Vec<String>,
    worst: (usize, usize),
    decisions: (usize, usize),
    lines: (usize, usize),
    fan_out: (usize, usize),
    ledger: Option<Ledger>,
) -> Cluster {
    Cluster {
        parent: parent.to_string(),
        children,
        mark,
        reasons,
        lines_before: lines.0,
        lines_after: lines.1,
        decisions_before: decisions.0,
        decisions_after: decisions.1,
        worst_function_before: Some(function("parentWorst", worst.0)),
        worst_function_after: Some(function("parentWorst", worst.1)),
        parent_fan_out_before: Some(fan_out.0),
        parent_fan_out_after: Some(fan_out.1),
        parent_fan_out_after_excluding_children: Some(fan_out.0),
        symbols_moved: Vec::new(),
        symbols_new: Vec::new(),
        symbols_lost: Vec::new(),
        ledger,
    }
}

#[cfg(test)]
fn rollup(before: &str, after: &str, regression: bool, lost: &[&str]) -> Rollup {
    let (medal_before, medal_after) = (medal_for(before), medal_for(after));
    /// Per-pillar `(before_score, after_score)` on the displayed scale.
    const SCORES: [(f64, f64); 4] = [(11.0, 38.0), (23.0, 26.0), (100.0, 100.0), (29.0, 70.0)];
    let holds = |verdict: &str, key: &str| {
        verdict
            .split('_')
            .any(|part| part.eq_ignore_ascii_case(key))
    };
    let pillars = PILLARS
        .iter()
        .zip(SCORES)
        .map(|((key, _), (before_score, after_score))| {
            let lost = lost.contains(key);
            let before_passed = lost || holds(&medal_before.verdict, key);
            let after_passed = !lost && holds(&medal_after.verdict, key);
            (
                (*key).to_string(),
                PillarRollup {
                    before_passed,
                    after_passed,
                    before_score,
                    after_score,
                    files_before: 23,
                    files_after: 23,
                    failing_before: if before_passed { 0 } else { 3 },
                    failing_after: if after_passed { 0 } else { 3 },
                },
            )
        })
        .collect();
    Rollup {
        medal_before,
        medal_after,
        pillars,
        regression,
        files_before: 23,
        files_after: 23,
    }
}

#[cfg(test)]
fn scope(files: usize, new: usize, skipped: usize, measured: bool) -> Scope {
    Scope {
        files_scored: files,
        files_new: new,
        lines_added: 2539,
        lines_removed: 1864,
        files_skipped: skipped,
        files_deleted: 0,
        files_capped: 0,
        coupling: CouplingStatus {
            measured,
            note: if measured {
                "built from .git/topos-pr-5".to_string()
            } else {
                "gitnexus not installed".to_string()
            },
        },
    }
}

#[cfg(test)]
fn recap_of(
    headline: Headline,
    files: Vec<FileRecap>,
    clusters: Vec<Cluster>,
    project: Option<Rollup>,
    scope: Scope,
) -> PrRecap {
    PrRecap {
        schema: SCHEMA,
        base: "2e352d7aaaaaaa".to_string(),
        head: "7b18166bbbbbbb".to_string(),
        review: Some(PullRequest {
            number: 5,
            head_ref: "refactor/topos".to_string(),
            base_ref: "main".to_string(),
        }),
        headline,
        check: if headline.fails_check() {
            "fail"
        } else {
            "pass"
        },
        reason: "the split moved the worst functions down".to_string(),
        error: None,
        scope,
        project,
        clusters,
        files,
        skipped: Vec::new(),
        deleted: Vec::new(),
        hotspots: Vec::new(),
        non_claim: "Structural direction is not proof that tests or behavior still pass.",
    }
}

/// New child of a split: added, every pillar scored, no `before` side.
#[cfg(test)]
fn new_child(path: &str, tier: &str, pillars: [bool; 4], worst: usize, parent: &str) -> FileRecap {
    build(Spec {
        path,
        change: FileChange::Added,
        status: Headline::Improvement,
        before: None,
        after: tier,
        pillars: [
            (pillars[0], pillars[0], 0.0, 90.0),
            (pillars[1], pillars[1], 0.0, 90.0),
            (pillars[2], pillars[2], 0.0, 90.0),
            (pillars[3], pillars[3], 0.0, 90.0),
        ],
        worst: (0, worst),
        decisions: (0, 6),
        cluster: Some((parent, ClusterRole::Child)),
        cosmetic: false,
    })
}

/// PR #5: 23 files, four split clusters, one medal up, no regression.
#[cfg(test)]
pub(super) fn fixture_pr5() -> PrRecap {
    const POLL: &str = "components/polls/PollShell.tsx";
    const WEEK: &str = "components/polls/WeekGrid.tsx";
    const LINK: &str = "components/links/LinkForm.tsx";
    const CREATE: &str = "lib/bookings/create.ts";

    let mut files = vec![
        build(Spec {
            path: POLL,
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("BRONZE"),
            after: "BRONZE",
            pillars: [
                (false, false, 20.0, 20.0),
                (false, false, 30.0, 30.0),
                (true, true, 100.0, 100.0),
                (false, false, 40.0, 40.0),
            ],
            worst: (117, 85),
            decisions: (68, 71),
            cluster: Some((POLL, ClusterRole::Parent)),
            cosmetic: false,
        }),
        build(Spec {
            path: WEEK,
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("SILVER"),
            after: "SILVER",
            pillars: [
                (false, false, 22.0, 22.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (false, false, 44.0, 44.0),
            ],
            worst: (134, 93),
            decisions: (83, 87),
            cluster: Some((WEEK, ClusterRole::Parent)),
            cosmetic: false,
        }),
        build(Spec {
            path: LINK,
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("BRONZE"),
            after: "BRONZE",
            pillars: [
                (false, false, 18.0, 18.0),
                (false, false, 35.0, 35.0),
                (true, true, 100.0, 100.0),
                (false, false, 41.0, 41.0),
            ],
            worst: (117, 73),
            decisions: (33, 46),
            cluster: Some((LINK, ClusterRole::Parent)),
            cosmetic: false,
        }),
        build(Spec {
            path: CREATE,
            change: FileChange::Modified,
            status: Headline::Improvement,
            before: Some("BRONZE"),
            after: "SILVER",
            pillars: [
                (false, false, 10.0, 10.0),
                (false, false, 27.0, 0.0),
                (true, true, 100.0, 100.0),
                (false, true, 0.0, 100.0),
            ],
            worst: (33, 13),
            decisions: (17, 14),
            cluster: Some((CREATE, ClusterRole::Parent)),
            cosmetic: false,
        }),
        build(Spec {
            path: "lib/polls/ranges.ts",
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 30.0, 30.0),
                (true, true, 88.0, 88.0),
                (true, true, 100.0, 100.0),
                (true, true, 90.0, 90.0),
            ],
            worst: (9, 9),
            decisions: (12, 12),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "lib/polls/ranges.test.ts",
            change: FileChange::Modified,
            status: Headline::ImprovementScore,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 30.0, 30.0),
                (true, true, 88.0, 88.0),
                (true, true, 100.0, 100.0),
                (true, true, 91.0, 94.0),
            ],
            worst: (8, 8),
            decisions: (10, 10),
            cluster: None,
            cosmetic: false,
        }),
    ];

    let platinum = [true, true, true, true];
    let gold = [false, true, true, true];
    let silver = [false, true, true, false];
    files.extend([
        new_child(
            "components/polls/poll-shell-types.tsx",
            "PLATINUM",
            platinum,
            4,
            POLL,
        ),
        new_child(
            "components/polls/PollSubmittedView.tsx",
            "GOLD",
            gold,
            24,
            POLL,
        ),
        new_child(
            "components/polls/PollIdentifyView.tsx",
            "PLATINUM",
            platinum,
            5,
            POLL,
        ),
        new_child(
            "components/polls/KillCheckModal.tsx",
            "PLATINUM",
            platinum,
            4,
            POLL,
        ),
        new_child(
            "components/polls/ThinCoverageModal.tsx",
            "PLATINUM",
            platinum,
            3,
            POLL,
        ),
        new_child(
            "components/polls/week-grid-model.ts",
            "PLATINUM",
            platinum,
            9,
            WEEK,
        ),
        new_child(
            "components/polls/WeekGridLegend.tsx",
            "PLATINUM",
            platinum,
            6,
            WEEK,
        ),
        new_child(
            "components/polls/WeekGridDesktop.tsx",
            "GOLD",
            gold,
            9,
            WEEK,
        ),
        new_child("components/polls/WeekGridMobile.tsx", "GOLD", gold, 8, WEEK),
        new_child(
            "components/links/link-form-defaults.ts",
            "SILVER",
            silver,
            7,
            LINK,
        ),
        new_child(
            "components/links/form-controls.tsx",
            "PLATINUM",
            platinum,
            6,
            LINK,
        ),
        new_child(
            "components/links/MemberAvailabilitySection.tsx",
            "PLATINUM",
            platinum,
            8,
            LINK,
        ),
        new_child(
            "components/links/LivePreviewCard.tsx",
            "GOLD",
            gold,
            9,
            LINK,
        ),
        new_child(
            "lib/bookings/booking-error.ts",
            "PLATINUM",
            platinum,
            1,
            CREATE,
        ),
        new_child(
            "lib/bookings/booking-guards.ts",
            "PLATINUM",
            platinum,
            5,
            CREATE,
        ),
        new_child(
            "lib/bookings/booking-notify.ts",
            "PLATINUM",
            platinum,
            4,
            CREATE,
        ),
        new_child(
            "lib/bookings/booking-slots.ts",
            "PLATINUM",
            platinum,
            6,
            CREATE,
        ),
    ]);

    let clusters = vec![
        cluster_of(
            POLL,
            vec![
                child(
                    "components/polls/poll-shell-types.tsx",
                    Reach::Shared,
                    6,
                    12,
                ),
                child(
                    "components/polls/PollSubmittedView.tsx",
                    Reach::Private,
                    1,
                    0,
                ),
                child(
                    "components/polls/PollIdentifyView.tsx",
                    Reach::Private,
                    1,
                    0,
                ),
                child("components/polls/KillCheckModal.tsx", Reach::Private, 1, 0),
                child(
                    "components/polls/ThinCoverageModal.tsx",
                    Reach::Private,
                    1,
                    0,
                ),
            ],
            ClusterMark::Ok,
            Vec::new(),
            (117, 85),
            (68, 71),
            (1304, 1475),
            (15, 24),
            None,
        ),
        cluster_of(
            WEEK,
            vec![
                child("components/polls/week-grid-model.ts", Reach::Shared, 5, 17),
                child("components/polls/WeekGridLegend.tsx", Reach::Private, 1, 0),
                child("components/polls/WeekGridDesktop.tsx", Reach::Private, 1, 0),
                child("components/polls/WeekGridMobile.tsx", Reach::Private, 1, 0),
            ],
            ClusterMark::Ok,
            Vec::new(),
            (134, 93),
            (83, 87),
            (1191, 1439),
            (0, 5),
            Some(ledger(vec![moved(
                WEEK,
                "components/polls/week-grid-model.ts",
                "buildModel",
                13,
                13,
            )])),
        ),
        cluster_of(
            LINK,
            vec![
                child(
                    "components/links/link-form-defaults.ts",
                    Reach::Shared,
                    4,
                    14,
                ),
                child("components/links/form-controls.tsx", Reach::Private, 1, 0),
                child(
                    "components/links/MemberAvailabilitySection.tsx",
                    Reach::Private,
                    1,
                    0,
                ),
                child("components/links/LivePreviewCard.tsx", Reach::Private, 1, 0),
            ],
            ClusterMark::Warn,
            vec!["decisions rose 33→46 (+39%)".to_string()],
            (117, 73),
            (33, 46),
            (1138, 1305),
            (11, 19),
            None,
        ),
        cluster_of(
            CREATE,
            vec![
                child("lib/bookings/booking-error.ts", Reach::Private, 1, 0),
                child("lib/bookings/booking-guards.ts", Reach::Private, 1, 0),
                child("lib/bookings/booking-notify.ts", Reach::Private, 1, 0),
                child("lib/bookings/booking-slots.ts", Reach::Private, 1, 0),
            ],
            ClusterMark::Ok,
            Vec::new(),
            (33, 13),
            (17, 14),
            (313, 411),
            (13, 11),
            None,
        ),
    ];

    recap_of(
        Headline::Improvement,
        files,
        clusters,
        Some(rollup("BRONZE", "BRONZE", false, &[])),
        scope(23, 17, 1, true),
    )
}

/// A plain edit: two modified files, no splits at all.
#[cfg(test)]
pub(super) fn fixture_plain() -> PrRecap {
    let files = vec![
        build(Spec {
            path: "topos/cli/src/commands/config.rs",
            change: FileChange::Modified,
            status: Headline::ImprovementScore,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 40.0, 44.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 90.0, 90.0),
            ],
            worst: (14, 12),
            decisions: (30, 28),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/cli/src/commands/inspect.rs",
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("SILVER"),
            after: "SILVER",
            pillars: [
                (false, false, 30.0, 30.0),
                (true, true, 70.0, 70.0),
                (true, true, 100.0, 100.0),
                (false, false, 50.0, 50.0),
            ],
            worst: (20, 20),
            decisions: (40, 40),
            cluster: None,
            cosmetic: false,
        }),
    ];
    recap_of(
        Headline::ImprovementScore,
        files,
        Vec::new(),
        Some(rollup("SILVER", "SILVER", false, &[])),
        scope(2, 0, 0, false),
    )
}

/// One cluster plus every unclustered row word the card can print.
#[cfg(test)]
pub(super) fn fixture_mixed() -> PrRecap {
    const PARENT: &str = "topos/mcp/src/evaluation/depgraph.rs";
    let mut files = vec![
        build(Spec {
            path: "topos/mcp/src/tools/depgraph.rs",
            change: FileChange::Modified,
            status: Headline::Regression,
            before: Some("GOLD"),
            after: "SILVER",
            pillars: [
                (true, false, 62.0, 38.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 90.0, 90.0),
            ],
            worst: (10, 14),
            decisions: (50, 58),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/engine/src/graphs/mdg/object.rs",
            change: FileChange::Modified,
            status: Headline::RegressionScore,
            before: Some("SILVER"),
            after: "SILVER",
            pillars: [
                (false, false, 40.0, 38.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (false, false, 50.0, 50.0),
            ],
            worst: (18, 19),
            decisions: (60, 62),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/mcp/src/evaluation/freshness.rs",
            change: FileChange::Modified,
            status: Headline::RegressionScore,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 45.0, 30.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 77.0, 74.0),
            ],
            worst: (12, 13),
            decisions: (20, 22),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/cli/src/commands/composable.rs",
            change: FileChange::Modified,
            status: Headline::SuspiciousNoStructuralChange,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 40.0, 40.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 70.0, 74.0),
            ],
            worst: (9, 9),
            decisions: (14, 14),
            cluster: None,
            cosmetic: true,
        }),
        build(Spec {
            path: "topos/engine/src/adapters/gitnexus.rs",
            change: FileChange::Modified,
            status: Headline::Improvement,
            before: Some("BRONZE"),
            after: "SILVER",
            pillars: [
                (false, false, 30.0, 30.0),
                (false, false, 40.0, 40.0),
                (true, true, 100.0, 100.0),
                (false, true, 55.0, 81.0),
            ],
            worst: (22, 15),
            decisions: (44, 40),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/mcp/src/context_budget.rs",
            change: FileChange::Added,
            status: Headline::Improvement,
            before: None,
            after: "PLATINUM",
            pillars: [
                (true, true, 0.0, 95.0),
                (true, true, 0.0, 95.0),
                (true, true, 0.0, 100.0),
                (true, true, 0.0, 92.0),
            ],
            worst: (0, 6),
            decisions: (0, 8),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: "topos/cli/src/commands/depgraph.rs",
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 40.0, 40.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 90.0, 90.0),
            ],
            worst: (10, 10),
            decisions: (18, 18),
            cluster: None,
            cosmetic: false,
        }),
        build(Spec {
            path: PARENT,
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("GOLD"),
            after: "GOLD",
            pillars: [
                (false, false, 40.0, 40.0),
                (true, true, 80.0, 80.0),
                (true, true, 100.0, 100.0),
                (true, true, 90.0, 90.0),
            ],
            worst: (41, 18),
            decisions: (52, 54),
            cluster: Some((PARENT, ClusterRole::Parent)),
            cosmetic: false,
        }),
    ];
    let platinum = [true, true, true, true];
    let gold = [false, true, true, true];
    files.extend([
        new_child(
            "topos/mcp/src/evaluation/gitref.rs",
            "PLATINUM",
            platinum,
            4,
            PARENT,
        ),
        new_child(
            "topos/mcp/src/evaluation/window.rs",
            "PLATINUM",
            platinum,
            5,
            PARENT,
        ),
        new_child("topos/mcp/src/evaluation/store.rs", "GOLD", gold, 7, PARENT),
    ]);

    let clusters = vec![cluster_of(
        PARENT,
        vec![
            child("topos/mcp/src/evaluation/gitref.rs", Reach::Shared, 2, 4),
            child("topos/mcp/src/evaluation/window.rs", Reach::Private, 1, 0),
            child("topos/mcp/src/evaluation/store.rs", Reach::Private, 1, 0),
        ],
        ClusterMark::Ok,
        Vec::new(),
        (41, 18),
        (52, 54),
        (520, 610),
        (6, 9),
        None,
    )];

    let mut recap = recap_of(
        Headline::Regression,
        files,
        clusters,
        Some(rollup("SILVER", "SILVER", false, &[])),
        scope(11, 3, 0, true),
    );
    recap.deleted = vec!["topos/mcp/src/legacy.rs".to_string()];
    recap.hotspots = vec![Hotspot {
        path: "topos/mcp/src/tools/depgraph.rs".to_string(),
        line: 212,
        metric: "ast.max_function_complexity".to_string(),
        detail: "cap_generation_detail complexity 14, gate 10".to_string(),
        advice: "Extract a decision so this function clears the gate.".to_string(),
    }];
    recap
}

/// A split that went wrong: the parent lost SECURE and SIMPLE, one child
/// landed SLOP, and a moved function got more complex on the way.
#[cfg(test)]
pub(super) fn fixture_losses() -> PrRecap {
    const PARENT: &str = "topos/engine/src/functors/probes/cpg/taint.rs";
    let mut files = vec![build(Spec {
        path: PARENT,
        change: FileChange::Modified,
        status: Headline::Regression,
        before: Some("GOLD"),
        after: "BRONZE",
        pillars: [
            (true, false, 70.0, 30.0),
            (true, true, 80.0, 80.0),
            (true, false, 90.0, 40.0),
            (true, true, 90.0, 90.0),
        ],
        worst: (30, 48),
        decisions: (40, 61),
        cluster: Some((PARENT, ClusterRole::Parent)),
        cosmetic: false,
    })];
    files.extend([
        new_child(
            "topos/engine/src/functors/probes/cpg/taint_sinks.rs",
            "SLOP",
            [false, false, false, false],
            21,
            PARENT,
        ),
        new_child(
            "topos/engine/src/functors/probes/cpg/taint_walk.rs",
            "GOLD",
            [false, true, true, true],
            19,
            PARENT,
        ),
    ]);
    let clusters = vec![cluster_of(
        PARENT,
        vec![
            child(
                "topos/engine/src/functors/probes/cpg/taint_sinks.rs",
                Reach::Private,
                1,
                0,
            ),
            child(
                "topos/engine/src/functors/probes/cpg/taint_walk.rs",
                Reach::Private,
                1,
                3,
            ),
        ],
        ClusterMark::Fail,
        vec!["parent lost SECURE".to_string()],
        (30, 48),
        (40, 61),
        (400, 520),
        (4, 9),
        Some(ledger(vec![moved(
            PARENT,
            "topos/engine/src/functors/probes/cpg/taint_walk.rs",
            "walk",
            12,
            19,
        )])),
    )];
    recap_of(
        Headline::Regression,
        files,
        clusters,
        Some(rollup("GOLD", "SILVER", true, &["simple", "secure"])),
        scope(3, 2, 0, true),
    )
}

/// `count` near-identical clusters, for the GitHub length budget.
#[cfg(test)]
pub(super) fn fixture_many_clusters(count: usize) -> PrRecap {
    let platinum = [true, true, true, true];
    let mut files = Vec::new();
    let mut clusters = Vec::new();
    for index in 0..count {
        let parent = format!("topos/engine/src/functors/profunctors/module_{index:02}/mod.rs");
        files.push(build(Spec {
            path: &parent,
            change: FileChange::Modified,
            status: Headline::LateralMove,
            before: Some("BRONZE"),
            after: "BRONZE",
            pillars: [
                (false, false, 20.0, 20.0),
                (false, false, 30.0, 30.0),
                (true, true, 100.0, 100.0),
                (false, false, 40.0, 40.0),
            ],
            worst: (80, 40),
            decisions: (60, 62),
            cluster: Some((&parent, ClusterRole::Parent)),
            cosmetic: false,
        }));
        let mut children = Vec::new();
        for slot in 0..8 {
            let path =
                format!("topos/engine/src/functors/profunctors/module_{index:02}/part_{slot}.rs");
            files.push(new_child(&path, "PLATINUM", platinum, 5, &parent));
            children.push(child(&path, Reach::Shared, 3, 4));
        }
        let mut cluster = cluster_of(
            &parent,
            children,
            ClusterMark::Ok,
            Vec::new(),
            (80, 40),
            (60, 62),
            (900, 1100),
            (9, 14),
            None,
        );
        cluster.symbols_moved = (0..30)
            .map(|slot| topos_engine::graphs::mdg::split::SymbolMove {
                name: format!("relocatedProfunctorHelper{slot:02}"),
                kind: "Function".to_string(),
                from: parent.clone(),
                to: format!(
                    "topos/engine/src/functors/profunctors/module_{index:02}/part_{}.rs",
                    slot % 8
                ),
            })
            .collect();
        cluster.symbols_new = (0..20)
            .map(|slot| topos_engine::graphs::mdg::split::NewSymbol {
                name: format!("freshlyIntroducedBinding{slot:02}"),
                kind: "Function".to_string(),
                file: format!(
                    "topos/engine/src/functors/profunctors/module_{index:02}/part_{}.rs",
                    slot % 8
                ),
            })
            .collect();
        clusters.push(cluster);
    }
    recap_of(
        Headline::Improvement,
        files,
        clusters,
        Some(rollup("BRONZE", "BRONZE", false, &[])),
        scope(count * 9, count * 8, 0, true),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        fixture_losses, fixture_mixed, fixture_plain, fixture_pr5, render_card, RenderOptions,
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
