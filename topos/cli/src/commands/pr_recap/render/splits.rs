//! The card's **splits** table: one block per cluster, a row for the
//! parent, a row for each child worth one, and a fold for the rest.
//!
//! `--verbose` unfolds every child and adds the function ledger — what
//! moved, what was renamed, what is new — under each block.

use topos_engine::functors::profunctors::uast::ledger::MatchKind;
use topos_engine::graphs::mdg::split::Reach;

use super::layout::{
    dim_line, line, pad, row, CHANGE_WIDTH, CONTENT, FILE_WIDTH, MATRIX_HEADER, TAIL,
};
use super::{medal_cell, pillar_matrix};
use crate::commands::pr_recap::model::{Cluster, ClusterChild, FileRecap};
use crate::commands::pr_recap::view::{basename, tally, tier_rank, ClusterView};
use crate::commands::render::{truncate_left, truncate_right, RenderOptions};

const WORST_WIDTH: usize = 9;
/// The SIMPLE gate on `ast.max_function_complexity`. A child arriving
/// with a function above it is worth a row of its own.
const SIMPLE_GATE: usize = 10;
/// At most this many child rows per cluster before folding.
const MAX_CHILD_ROWS: usize = 3;
const MAX_LEDGER_LINES: usize = 20;

/// A child and its scored file, as [`ClusterView::children`] holds them.
type Child<'a> = (&'a ClusterChild, Option<&'a FileRecap>);

pub(super) fn splits_header() -> String {
    row(
        "SPLIT",
        "PARENT → CHILDREN",
        "MEDAL",
        MATRIX_HEADER,
        &format!("{}DECISIONS", pad("WORST FN", WORST_WIDTH)),
    )
}

pub(super) fn cluster_block(
    view: &ClusterView<'_>,
    verbose: bool,
    options: RenderOptions,
) -> Vec<String> {
    let mut lines = vec![line(&cluster_row(view, verbose), options)];
    let (shown, hidden) = child_split(view, verbose);
    for (index, child) in shown.iter().enumerate() {
        let last = hidden.is_empty() && index + 1 == shown.len();
        lines.push(line(
            &child_row(view.cluster, child, if last { '└' } else { '├' }),
            options,
        ));
    }
    if !hidden.is_empty() {
        lines.push(line(&fold_row(&hidden, shown.is_empty()), options));
    }
    if verbose {
        for text in ledger_lines(view.cluster) {
            lines.push(dim_line(
                &format!("{}{text}", " ".repeat(CHANGE_WIDTH)),
                options,
            ));
        }
    }
    lines
}

fn cluster_row(view: &ClusterView<'_>, verbose: bool) -> String {
    let cluster = view.cluster;
    let change = format!("{} SPLIT", view.mark);
    let medal = view
        .parent
        .map_or_else(|| "unparsed".to_string(), medal_cell);
    let mut decisions = format!("{}→{}", cluster.decisions_before, cluster.decisions_after);
    if let Some(growth) = view.growth {
        decisions.push_str(&format!(" +{growth}%"));
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
        &pillar_matrix(view.parent),
        &format!("{}{decisions}", pad(&view.worst("→"), WORST_WIDTH)),
    )
}

/// A child earns its own row when it says something the fold cannot:
/// it is shared beyond its parent, it carries symbols that used to live
/// in the parent, it landed below GOLD, it is cosmetic, or it arrived
/// with a function above the SIMPLE gate.
fn notable((child, file): &Child<'_>) -> bool {
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

fn child_split<'v, 'a>(
    view: &'v ClusterView<'a>,
    verbose: bool,
) -> (Vec<&'v Child<'a>>, Vec<&'v Child<'a>>) {
    if verbose {
        return (view.children.iter().collect(), Vec::new());
    }
    let mut shown = Vec::new();
    let mut hidden = Vec::new();
    for child in &view.children {
        if notable(child) && shown.len() < MAX_CHILD_ROWS {
            shown.push(child);
        } else {
            hidden.push(child);
        }
    }
    (shown, hidden)
}

fn child_row(cluster: &Cluster, (child, file): &Child<'_>, connector: char) -> String {
    let name = truncate_left(basename(&child.path), FILE_WIDTH - 4);
    let medal = file.map_or_else(|| "unparsed".to_string(), medal_cell);
    row(
        "",
        &format!("{connector}─ {name}"),
        &medal,
        &pillar_matrix(*file),
        &child_fact(cluster, child, *file),
    )
}

/// Fixed priority, greedily fitted: a move-in fact beats sharing, and
/// sharing beats a bare worst-fn count, which only appears when nothing
/// else applies. The row may use the full remaining width up to the
/// line budget.
fn child_fact(cluster: &Cluster, child: &ClusterChild, file: Option<&FileRecap>) -> String {
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
    let room = CONTENT.saturating_sub(TAIL);
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
        .filter(|entry| entry.kind.is_move() || entry.kind == MatchKind::Renamed)
        .filter_map(|entry| {
            let after = entry.after.as_ref()?;
            if after.file != child.path {
                return None;
            }
            Some(Moved {
                name: after.qualified_name.as_str(),
                delta: entry.complexity_delta,
                before: entry
                    .before
                    .as_ref()
                    .map_or(0, |snapshot| snapshot.complexity),
                after: after.complexity,
                kind: entry.kind,
                named: entry.is_named() && !entry.is_nested(),
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

fn fold_row(hidden: &[&Child<'_>], all: bool) -> String {
    let subject = format!("└─ {} {}", hidden.len(), if all { "files" } else { "more" });
    // No dots on a fold row: the group is summarised, and a signature
    // that belonged to one of several files would be a lie. The tally is
    // indented to sit under the tier words of the rows above it.
    let medal = tally(
        hidden.iter().map(|(_, file)| {
            let tier = file
                .and_then(|file| file.medal_after.as_ref())
                .map_or_else(|| "unparsed".to_string(), |medal| medal.tier.clone());
            (tier.clone(), tier)
        }),
        true,
    );
    row("", &subject, &medal, "", "")
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
        if entry.is_nested() || !entry.is_named() {
            if entry.kind.is_move() {
                hidden_moved += 1;
            }
            continue;
        }
        let before = entry.before.as_ref();
        let after = entry.after.as_ref();
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
