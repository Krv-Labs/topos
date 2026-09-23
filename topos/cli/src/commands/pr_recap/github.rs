//! GitHub sticky-comment Markdown for `topos pr-recap`.
//!
//! [`STICKY_MARKER`] is the first line so the Action can *edit* its own
//! comment instead of deleting and re-posting one on every push — the
//! behavior every competing bot gets wrong, and the reason a reviewer's
//! reply thread survives a force-push.
//!
//! The body is the card's document in Markdown: the readiness and the
//! one finding that decided it, what blocks and what needs attention
//! (info collapsed), the changed files with the card's MEDAL and CHANGE
//! columns, one cluster table, one `<details>` per cluster carrying the
//! symbol ledger, a Mermaid graph of the dependency shape the split
//! produced, and the gate settings in the footer. Every segment and
//! sentence comes from the card's own helpers, so the two never word a
//! fact differently. Nothing here paints: the comment carries no ANSI.
//!
//! GitHub caps a comment body at 65_536 characters and silently
//! truncates past it, so [`MAX_CHARS`] leaves headroom and the cluster
//! details are dropped smallest-first when a very large PR would
//! overflow; the verdict and the findings never are.

use std::fmt::Write as _;

use topos_engine::config::Severity;
use topos_engine::functors::profunctors::uast::ledger::MatchKind;
use topos_engine::graphs::mdg::split::Reach;

use super::gates::Finding;
use super::model::{Cluster, FileRecap, PrRecap};
use super::render::{
    change_text, changed_rows, facts, gate_line, headline, kept_count, medal_cell, severity_mark,
};
use super::view::{basename, worst_span, ClusterView, Item, RecapView};

pub(super) const STICKY_MARKER: &str = "<!-- topos-pr-recap:v2 -->";

/// GitHub's own limit is 65_536; stop well short of silent truncation.
const MAX_CHARS: usize = 60_000;
/// A symbol list longer than this is a wall, not evidence.
const MAX_SYMBOLS: usize = 30;
/// Blocking or needs-attention items listed before the rest fold into
/// `+N more`.
const MAX_LOCUS: usize = 5;
/// Notes listed inside their `<details>` before the rest fold away.
const MAX_NOTES: usize = 10;
/// Changed-file rows before the table folds into `+N more`.
const MAX_FILE_ROWS: usize = 40;

pub(super) fn render_github(recap: &PrRecap) -> String {
    let view = RecapView::new(recap);
    let items = view.items();
    let mut head = String::new();
    head.push_str(STICKY_MARKER);
    head.push('\n');
    let _ = writeln!(head, "{}\n", title(&view));
    let _ = writeln!(head, "<sub>{}</sub>\n", meta(&view));
    let _ = writeln!(head, "**Why:** {}.\n", headline(recap, &items));
    let summary = summary(&view);
    if !summary.is_empty() {
        let _ = writeln!(head, "{summary}\n");
    }
    let (blocking, attention): (Vec<&Item<'_>>, Vec<&Item<'_>>) = items
        .iter()
        .partition(|item| item.severity() == Severity::Block);
    head.push_str(&finding_list("Blocking", &blocking));
    head.push_str(&finding_list("Needs attention", &attention));
    head.push_str(&notes(&view.note_items()));
    head.push_str(&changed_table(recap));
    if !view.clusters.is_empty() {
        head.push_str(&cluster_table(&view));
        head.push('\n');
    }

    let details: Vec<String> = view.clusters.iter().map(cluster_details).collect();
    let mut tail = String::new();
    if let Some(graph) = dependency_graph(&view) {
        tail.push_str(&graph);
        tail.push('\n');
    }
    let _ = writeln!(tail, "{}", footer(recap));

    assemble(&head, &recap.clusters, details, &tail)
}

/// Drop cluster details from the smallest cluster upward until the body
/// fits, then say how many were dropped rather than truncating mid-table.
fn assemble(head: &str, clusters: &[Cluster], details: Vec<String>, tail: &str) -> String {
    let mut order: Vec<usize> = (0..details.len()).collect();
    order.sort_by_key(|index| {
        (
            clusters[*index].children.len(),
            clusters[*index].symbols_moved.len(),
        )
    });
    let mut dropped = 0usize;
    loop {
        let mut body = String::from(head);
        for (index, block) in details.iter().enumerate() {
            if order[..dropped].contains(&index) {
                continue;
            }
            body.push_str(block);
            body.push('\n');
        }
        if dropped > 0 {
            let _ = writeln!(
                body,
                "{dropped} cluster details omitted for length; see --json\n"
            );
        }
        body.push_str(tail);
        if body.chars().count() <= MAX_CHARS {
            return body;
        }
        if dropped == details.len() {
            // Nothing left to fold away: cut on a line boundary so the
            // comment stays valid Markdown.
            let mut cut: String = body.chars().take(MAX_CHARS - 200).collect();
            if let Some(at) = cut.rfind('\n') {
                cut.truncate(at);
            }
            cut.push_str("\n\nTruncated for length; see --json.\n");
            return cut;
        }
        dropped += 1;
    }
}

/// `### X Blocked · Topos structural review of #359`: the card's
/// readiness mark and word, in sentence case.
fn title(view: &RecapView<'_>) -> String {
    let readiness = view.recap.readiness;
    format!(
        "### {} {} · Topos structural review of {}{}",
        readiness.mark(),
        sentence_case(readiness.word()),
        view.subject,
        view.incomplete_note()
    )
}

/// `NEEDS ATTENTION` → `Needs attention`.
fn sentence_case(word: &str) -> String {
    let lower = word.to_lowercase();
    let mut chars = lower.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// `3 files · +174/−1 · priority navigable · COMPOSABLE not measured ·
/// 1 skipped`, zero counts left out.
fn meta(view: &RecapView<'_>) -> String {
    let recap = view.recap;
    let scope = &recap.scope;
    let mut parts = vec![
        plural(scope.files_scored, "file", "files"),
        format!("+{}/−{}", scope.lines_added, scope.lines_removed),
    ];
    parts.extend(view.context.iter().cloned());
    if scope.files_skipped > 0 {
        parts.push(format!("{} skipped", scope.files_skipped));
    }
    if !recap.deleted.is_empty() {
        parts.push(format!("{} deleted", recap.deleted.len()));
    }
    parts.join(" · ")
}

/// Medal moves, splits and the project rollup in sentences; no zero
/// counts, and empty when nothing moved.
fn summary(view: &RecapView<'_>) -> String {
    let tally = &view.tally;
    let mut sentences = Vec::new();
    let moves: Vec<String> = [(tally.up, "up"), (tally.down, "down")]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, way)| format!("{} moved {way}", plural(count, "medal", "medals")))
        .collect();
    if !moves.is_empty() {
        sentences.push(format!("{}.", moves.join(", ")));
    }
    if !tally.new_medals.is_empty() {
        // `1 new file arrived as GOLD`, not `as 1 GOLD`.
        let medals = if tally.new == 1 {
            tally.new_medals.trim_start_matches("1 ")
        } else {
            &tally.new_medals
        };
        sentences.push(format!(
            "{} arrived as {medals}.",
            plural(tally.new, "new file", "new files")
        ));
    }
    if !view.clusters.is_empty() {
        let children: usize = view
            .clusters
            .iter()
            .map(|cluster| cluster.children.len())
            .sum();
        sentences.push(if view.clusters.len() == 1 {
            format!("1 file was split into {children}.")
        } else {
            format!("{} files were split into {children}.", view.clusters.len())
        });
        if let Some((low, high)) = view.worst_drop {
            sentences.push(if low == high {
                format!("Worst-function complexity fell {low}% across the clusters.")
            } else {
                format!("Worst-function complexity fell {low}–{high}% across the clusters.")
            });
        }
        let (before, after) = view.decisions;
        sentences.push(format!("Total decision count went {before} → {after}."));
    }
    if let Some((names, pillars)) = &view.still_failing {
        sentences.push(format!(
            "{} still fail {}.",
            names.join(", "),
            pillars.join(" and ")
        ));
    }
    if let Some(project) = &view.recap.project {
        sentences.push(format!(
            "Project rollup over the touched files: {} → {}{}.",
            project.medal_before.tier,
            project.medal_after.tier,
            if project.regression {
                " (a pillar the base passed now fails)"
            } else {
                ""
            }
        ));
    }
    sentences.join(" ")
}

/// `+2 more — see --json.` under a list or table capped at `cap`.
fn more_line(total: usize, cap: usize) -> String {
    if total > cap {
        format!("+{} more — see `--json`.\n\n", total - cap)
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------- findings

/// `**Blocking**` or `**Needs attention**` and one bullet per item, in
/// the recap's order; nothing when there are none.
fn finding_list(heading: &str, items: &[&Item<'_>]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = format!("**{heading}**\n\n");
    for item in items.iter().take(MAX_LOCUS) {
        let _ = writeln!(out, "- {}", item_line(item));
    }
    out.push('\n');
    out.push_str(&more_line(items.len(), MAX_LOCUS));
    out
}

/// The info findings, collapsed: they never change the verdict.
fn notes(items: &[Item<'_>]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "<details>\n<summary>{}</summary>\n\n",
        plural(items.len(), "note", "notes")
    );
    for item in items.iter().take(MAX_NOTES) {
        let _ = writeln!(out, "- {}", item_line(item));
    }
    if items.len() > MAX_NOTES {
        let _ = writeln!(out, "- +{} more — see `--json`.", items.len() - MAX_NOTES);
    }
    out.push_str("\n</details>\n\n");
    out
}

/// `` `dispatch.rs:62` · `sanitize` — SIMPLE 32 > 10 · lift … ``: the
/// card's facts after the place, or the facts alone for a finding about
/// the whole range.
fn item_line(item: &Item<'_>) -> String {
    let facts = facts(item);
    match location(item.lead()) {
        Some(place) => format!("{place} — {facts}"),
        None => facts,
    }
}

fn location(finding: &Finding) -> Option<String> {
    if finding.path.is_empty() {
        return None;
    }
    let mut place = match finding.line {
        Some(line) => format!("`{}:{line}`", finding.path),
        None => format!("`{}`", finding.path),
    };
    if let Some(function) = &finding.function {
        let _ = write!(place, " · `{function}`");
    }
    Some(place)
}

// ----------------------------------------------------------- changed files

/// The card's **Changed files** table: the worst finding's mark, the
/// MEDAL and the CHANGE segments.
fn changed_table(recap: &PrRecap) -> String {
    let rows = changed_rows(recap);
    let kept = kept_count(recap, &rows);
    if rows.is_empty() && kept == 0 {
        return String::new();
    }
    let mut out = String::from("**Changed files**\n\n");
    if !rows.is_empty() {
        out.push_str("| File | Medal | Change |\n|---|---|---|\n");
        for file in rows.iter().take(MAX_FILE_ROWS) {
            let _ = writeln!(
                out,
                "| {} | {} | {} |",
                marked_path(file),
                medal_cell(file),
                escape(&change_text(file).join(" · "))
            );
        }
        out.push('\n');
        out.push_str(&more_line(rows.len(), MAX_FILE_ROWS));
    }
    if kept > 0 {
        let _ = writeln!(
            out,
            "{}\n",
            if kept == 1 {
                "1 file kept its medal.".to_string()
            } else {
                format!("{kept} files kept their medal.")
            }
        );
    }
    out
}

/// `` X `path` `` for a file with a block, `` ! `path` `` with a warning,
/// the bare path otherwise.
fn marked_path(file: &FileRecap) -> String {
    let path = format!("`{}`", escape(&file.path));
    match severity_mark(file.severity.unwrap_or(Severity::Off)) {
        ' ' => path,
        mark => format!("{mark} {path}"),
    }
}

fn escape(text: &str) -> String {
    text.replace('|', "\\|")
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

// ---------------------------------------------------------------- clusters

fn medal_of(file: Option<&FileRecap>) -> String {
    file.map_or_else(|| "unparsed".to_string(), medal_cell)
}

fn symbol_of(file: Option<&FileRecap>) -> &str {
    file.and_then(|file| file.medal_after.as_ref())
        .map_or("·", |medal| medal.symbol.as_str())
}

fn change_of(file: Option<&FileRecap>) -> String {
    file.map_or_else(
        || "·".to_string(),
        |file| escape(&change_text(file).join(" · ")),
    )
}

fn cluster_table(view: &RecapView<'_>) -> String {
    let mut out = String::from(
        "| Cluster | Medal | Worst fn | Decisions | Lines | Moved / new | Parent fan-out |\n\
         |---|---|---|---|---|---|---|\n",
    );
    for cv in &view.clusters {
        let cluster = cv.cluster;
        let fan_out = match (cluster.parent_fan_out_before, cluster.parent_fan_out_after) {
            (Some(before), Some(after)) => format!("{before} → {after}"),
            _ => "·".to_string(),
        };
        let moved: usize = cluster.children.iter().map(|child| child.moved_in).sum();
        let _ = writeln!(
            out,
            "| {} `{}` → {} files | {} | {} | {} → {} | {} → {} | {moved} / {} | {fan_out} |",
            cv.mark,
            escape(&cluster.parent),
            cluster.children.len(),
            medal_of(cv.parent),
            cv.worst(" → "),
            cluster.decisions_before,
            cluster.decisions_after,
            cluster.lines_before,
            cluster.lines_after,
            cluster.symbols_new.len()
        );
    }
    out
}

fn worst_column(file: Option<&FileRecap>) -> String {
    file.map_or_else(
        || "·".to_string(),
        |file| {
            worst_span(
                file.worst_function_before.as_ref(),
                file.worst_function_after.as_ref(),
                " → ",
            )
        },
    )
}

fn symbol_list(names: &[String]) -> String {
    let shown: Vec<String> = names
        .iter()
        .take(MAX_SYMBOLS)
        .map(|name| format!("`{name}`"))
        .collect();
    let mut text = shown.join(", ");
    if names.len() > MAX_SYMBOLS {
        let _ = write!(text, ", +{} more", names.len() - MAX_SYMBOLS);
    }
    text
}

/// Names of everything that travelled, preferring the coupling graphs
/// and falling back to the UAST ledger when they were not built.
fn moved_names(cluster: &Cluster) -> Vec<String> {
    if !cluster.symbols_moved.is_empty() {
        return cluster
            .symbols_moved
            .iter()
            .map(|moved| format!("{} → {}", moved.name, basename(&moved.to)))
            .collect();
    }
    cluster
        .ledger
        .iter()
        .flat_map(|ledger| &ledger.matches)
        .filter(|entry| entry.kind.is_move() || entry.kind == MatchKind::Renamed)
        .filter_map(|entry| {
            let after = entry.after.as_ref()?;
            Some(format!("{} → {}", after.name, basename(&after.file)))
        })
        .collect()
}

fn cluster_details(cv: &ClusterView<'_>) -> String {
    let cluster = cv.cluster;
    let mut out = format!(
        "<details>\n<summary>{} <code>{}</code> → {} files</summary>\n\n",
        cv.mark,
        escape(&cluster.parent),
        cluster.children.len()
    );
    out.push_str(
        "| File | Medal | Change | Worst fn | Fan-in | Reach |\n|---|---|---|---|---|---|\n",
    );
    let _ = writeln!(
        out,
        "| `{}` | {} | {} | {} | {} | parent |",
        escape(&cluster.parent),
        medal_of(cv.parent),
        change_of(cv.parent),
        worst_column(cv.parent),
        cv.parent
            .and_then(|file| file.fan_in_after)
            .map_or_else(|| "·".to_string(), |value| value.to_string())
    );
    for (child, file) in &cv.children {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} |",
            escape(&child.path),
            medal_of(*file),
            change_of(*file),
            worst_column(*file),
            child.importers.len(),
            match child.reach {
                Some(Reach::Shared) => "shared",
                Some(Reach::Private) => "private",
                None => "·",
            }
        );
    }
    out.push('\n');
    let moved = moved_names(cluster);
    if !moved.is_empty() {
        let _ = writeln!(out, "**Moved:** {}\n", symbol_list(&moved));
    }
    if !cluster.symbols_new.is_empty() {
        let names: Vec<String> = cluster
            .symbols_new
            .iter()
            .map(|symbol| symbol.name.clone())
            .collect();
        let _ = writeln!(out, "**New:** {}\n", symbol_list(&names));
    }
    if !cluster.symbols_lost.is_empty() {
        let _ = writeln!(out, "**Lost:** {}\n", symbol_list(&cluster.symbols_lost));
    }
    out.push_str("</details>\n");
    out
}

/// Mermaid renders inside a PR comment, so the dependency shape a split
/// produced can be *seen* rather than described.
fn dependency_graph(view: &RecapView<'_>) -> Option<String> {
    let measured = view
        .clusters
        .iter()
        .any(|cv| cv.children.iter().any(|(child, _)| child.reach.is_some()));
    if !measured {
        return None;
    }
    let mut out = String::from(
        "<details>\n<summary>Dependency shape after the split</summary>\n\n```mermaid\ngraph LR\n",
    );
    for (index, cv) in view.clusters.iter().enumerate() {
        let parent = format!("p{index}");
        let _ = writeln!(
            out,
            "  {parent}[\"{} {}\"]",
            symbol_of(cv.parent),
            basename(&cv.cluster.parent)
        );
        for (slot, (child, file)) in cv.children.iter().enumerate() {
            let _ = writeln!(
                out,
                "  {parent} -->|{}| c{index}_{slot}[\"{} {}\"]",
                child.importers.len(),
                symbol_of(*file),
                basename(&child.path)
            );
        }
    }
    out.push_str("```\n\n</details>\n");
    Some(out)
}

// ------------------------------------------------------------------ footer

/// The gate settings that produced the verdict, then how to reproduce
/// the document.
fn footer(recap: &PrRecap) -> String {
    let subject = recap.review.as_ref().map_or_else(
        || "--base &lt;rev&gt;".to_string(),
        |review| review.number.to_string(),
    );
    format!(
        "<sub>{}<br>Deterministic, no LLM. {} <code>topos pr-recap {subject} --json</code> reproduces this document.</sub>",
        gate_line(recap),
        recap.non_claim
    )
}

#[cfg(test)]
mod tests {
    use super::{render_github, MAX_CHARS, STICKY_MARKER};
    use crate::commands::pr_recap::fixtures::{
        fixture_lateral_loss, fixture_losses, fixture_many_clusters, fixture_mixed, fixture_plain,
        fixture_pr5, LATERAL_LOSS,
    };
    use crate::commands::pr_recap::gates::Readiness;
    use crate::commands::pr_recap::model::PrRecap;
    use topos_engine::config::{GateId, Severity};

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

    /// The Markdown between `**{heading}**` and the next blank-line
    /// paragraph break.
    fn section<'b>(body: &'b str, heading: &str) -> &'b str {
        let start = body
            .find(&format!("**{heading}**\n\n"))
            .unwrap_or_else(|| panic!("no {heading} section: {body}"));
        let rest = &body[start + heading.len() + 6..];
        &rest[..rest.find("\n\n").unwrap_or(rest.len())]
    }

    #[test]
    fn pr5_is_a_ready_sticky_comment() {
        let body = render_github(&fixture_pr5());
        assert!(body.starts_with(STICKY_MARKER), "{body}");
        assert!(
            body.contains("\n### ✓ Ready · Topos structural review of #5\n"),
            "{body}"
        );
        assert!(
            body.contains("<sub>23 files · +"),
            "the scope moved from the title to the meta line: {body}"
        );
        assert!(
            body.contains("priority secure · COMPOSABLE measured · 1 skipped</sub>"),
            "{body}"
        );
        assert!(body.contains("**Why:** no pillar or medal lost"), "{body}");
        assert!(body.contains("<details>"), "{body}");
        assert!(body.contains("```mermaid"), "{body}");
        assert!(body.contains("graph LR"), "{body}");
        assert!(body.contains("| Cluster | Medal | Worst fn |"), "{body}");
        assert!(
            body.contains("| File | Medal | Change | Worst fn | Fan-in | Reach |"),
            "{body}"
        );
        assert!(!body.contains("S C E N"), "{body}");
        assert!(!body.contains("**Blocking**"), "{body}");
        assert!(!body.contains("**Needs attention**"), "{body}");
        assert!(body.chars().count() < MAX_CHARS, "{}", body.chars().count());
    }

    /// The marker is what lets the Action edit its own comment; a new
    /// value would orphan every comment already posted.
    #[test]
    fn the_sticky_marker_is_unchanged() {
        assert_eq!(STICKY_MARKER, "<!-- topos-pr-recap:v2 -->");
    }

    /// A red check says which place blocks, what was measured and what to
    /// change, above the cluster sections a length cap could drop.
    #[test]
    fn a_blocked_change_lists_its_blocking_items() {
        let recap = fixture_losses();
        let body = render_github(&recap);
        assert!(
            body.contains("\n### X Blocked · Topos structural review of #"),
            "{body}"
        );
        assert!(
            body.contains("**Why:** taint.rs lost SIMPLE and SECURE (GOLD → BRONZE).\n"),
            "{body}"
        );
        let blocking = section(&body, "Blocking");
        assert!(
            blocking.starts_with("- `topos/engine/src/functors/probes/cpg/taint.rs"),
            "{blocking}"
        );
        assert!(blocking.contains("SIMPLE lost"), "{blocking}");
        assert!(blocking.contains("SECURE lost"), "{blocking}");
        assert!(!body.contains("**Failing the check**"), "{body}");
        assert!(!body.contains("**Where to look**"), "{body}");
        let listed = body.find("**Blocking**").expect("a blocking list");
        assert!(listed < body.find("| Cluster |").expect("a cluster table"));
    }

    #[test]
    fn a_change_needing_attention_lists_warnings_only() {
        let body = render_github(&needs_attention());
        assert!(
            body.contains("\n### ! Needs attention · Topos structural review of #"),
            "{body}"
        );
        assert!(!body.contains("**Blocking**"), "{body}");
        let attention = section(&body, "Needs attention");
        assert_eq!(attention.lines().count(), 2, "{attention}");
        assert!(
            attention.lines().all(|line| line.starts_with("- `")),
            "{attention}"
        );
        assert!(
            body.contains("<sub>gate: recommended · warnings don't fail the check<br>"),
            "{body}"
        );
    }

    #[test]
    fn a_mixed_change_lists_blocks_before_warnings() {
        let body = render_github(&fixture_mixed());
        let blocking = body.find("**Blocking**").expect("a blocking list");
        let attention = body.find("**Needs attention**").expect("a warning list");
        assert!(blocking < attention, "{body}");
        assert!(
            section(&body, "Blocking").contains("`topos/mcp/src/tools/depgraph.rs"),
            "{body}"
        );
    }

    /// Info findings never change the verdict, so they are folded away.
    #[test]
    fn info_findings_are_collapsed() {
        let mut recap = fixture_mixed();
        let notes = recap
            .findings
            .iter()
            .filter(|finding| finding.severity == Severity::Info)
            .count();
        if notes == 0 {
            let mut note = recap.findings[0].clone();
            note.severity = Severity::Info;
            note.path = "topos/cli/src/noted.rs".to_string();
            note.line = None;
            note.function = None;
            recap.findings.push(note);
        }
        let body = render_github(&recap);
        let at = body
            .find("<details>\n<summary>")
            .expect("a collapsed notes block");
        let block = &body[at..at + body[at..].find("</details>").expect("closed")];
        assert!(block.contains(" note"), "{block}");
        assert!(block.contains("\n- `"), "{block}");
        assert!(
            at > body.find("**Needs attention**").expect("warnings first"),
            "{body}"
        );
    }

    /// A dip under a point is noise on the card, so the comment hides it
    /// too: no note, and no file row when it is the file's only change.
    #[test]
    fn dips_under_a_point_are_hidden() {
        const TINY: &str = "topos/engine/src/evaluation/suggestions.rs";
        const SMALL: &str = "topos/cli/src/commands/install/harness.rs";
        let mut recap = fixture_mixed();
        let template = recap.findings[0].clone();
        for (path, before, after) in [(TINY, 55.8, 55.7), (SMALL, 52.5, 47.5)] {
            let mut dip = template.clone();
            dip.gate = GateId::ScoreDrop;
            dip.severity = Severity::Info;
            dip.material = false;
            dip.path = path.to_string();
            dip.line = None;
            dip.function = None;
            dip.pillar = Some("simple".to_string());
            dip.before = Some(before);
            dip.after = Some(after);
            recap.findings.push(dip);
        }
        let mut file = recap
            .files
            .iter()
            .find(|file| !file.is_new() && !file.is_split_child())
            .expect("an existing file")
            .clone();
        file.path = TINY.to_string();
        file.severity = Some(Severity::Info);
        file.cosmetic = false;
        file.medal_before = file.medal_after.clone();
        for delta in file.pillars.values_mut() {
            delta.before_passed = delta.after_passed;
            delta.before_score = delta.after_score;
        }
        let simple = file.pillars.get_mut("simple").expect("a SIMPLE delta");
        simple.before_score = Some(55.8);
        simple.after_score = Some(55.7);
        recap.files.push(file);

        let body = render_github(&recap);
        let at = body.find("<details>\n<summary>").expect("a notes block");
        let block = &body[at..at + body[at..].find("</details>").expect("closed")];
        assert!(
            block.contains(SMALL),
            "a dip of a point or more stays: {block}"
        );
        assert!(!body.contains(TINY), "{body}");
        assert!(!body.contains("55.8"), "{body}");
    }

    /// The file table is the card's: the worst finding's mark, the MEDAL
    /// with both tiers when it moved, and the CHANGE segments.
    #[test]
    fn the_file_table_has_medal_and_change_columns() {
        let body = render_github(&fixture_mixed());
        assert!(
            body.contains("**Changed files**\n\n| File | Medal | Change |\n|---|---|---|\n"),
            "{body}"
        );
        assert!(
            body.contains(
                "| X `topos/mcp/src/tools/depgraph.rs` | GOLD → SILVER | X SIMPLE lost |"
            ),
            "{body}"
        );
        assert!(body.contains("files kept their medal."), "{body}");
    }

    /// A trade keeps its tier, and the CHANGE cell still says what was
    /// lost and what was gained.
    #[test]
    fn a_trade_row_names_the_loss_and_the_gain() {
        let body = render_github(&fixture_lateral_loss());
        let row = body
            .lines()
            .find(|line| line.starts_with(&format!("| X `{LATERAL_LOSS}` |")))
            .unwrap_or_else(|| panic!("no row for the trade: {body}"));
        assert!(
            row.ends_with("| SILVER | X SIMPLE lost · ✓ NAVIGABLE gained |"),
            "{row}"
        );
        assert!(
            body.contains("**Why:** lattice.rs traded SIMPLE for NAVIGABLE."),
            "{body}"
        );
    }

    #[test]
    fn the_gate_settings_sit_in_the_footer() {
        let body = render_github(&fixture_losses());
        let footer = body.lines().last().expect("a footer");
        assert!(
            footer.starts_with("<sub>gate: recommended<br>Deterministic, no LLM."),
            "{footer}"
        );
        assert!(
            footer.ends_with("reproduces this document.</sub>"),
            "{footer}"
        );
    }

    #[test]
    fn a_long_blocking_list_folds_into_more() {
        let mut recap = fixture_losses();
        let lead = recap.findings[0].clone();
        recap.findings = (1..=7)
            .map(|line| {
                let mut finding = lead.clone();
                finding.line = Some(line * 10);
                finding
            })
            .collect();
        let body = render_github(&recap);
        let blocking = section(&body, "Blocking");
        assert_eq!(blocking.lines().count(), 5, "{blocking}");
        assert!(body.contains("+2 more — see `--json`."), "{body}");
    }

    #[test]
    fn a_huge_pr_drops_details_instead_of_overflowing() {
        let body = render_github(&fixture_many_clusters(40));
        assert!(body.chars().count() < MAX_CHARS, "{}", body.chars().count());
        assert!(
            body.contains("omitted for length"),
            "tail: {}",
            &body[body.len() - 400..]
        );
        assert!(body.starts_with(STICKY_MARKER));
    }

    #[test]
    fn a_plain_edit_has_no_cluster_sections() {
        let body = render_github(&fixture_plain());
        assert!(!body.contains("```mermaid"), "{body}");
        assert!(!body.contains("| Cluster |"), "{body}");
        assert!(body.starts_with(STICKY_MARKER));
    }

    /// The comment is Markdown for a browser: no escape sequence, ever.
    #[test]
    fn no_comment_carries_ansi() {
        for recap in [
            fixture_pr5(),
            fixture_plain(),
            fixture_lateral_loss(),
            fixture_mixed(),
            fixture_losses(),
            needs_attention(),
        ] {
            let body = render_github(&recap);
            assert!(!body.contains('\u{1b}'), "{body}");
        }
    }
}
