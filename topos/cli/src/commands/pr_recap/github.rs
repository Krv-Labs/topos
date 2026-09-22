//! GitHub sticky-comment Markdown for `topos pr-recap`.
//!
//! [`STICKY_MARKER`] is the first line so the Action can *edit* its own
//! comment instead of deleting and re-posting one on every push — the
//! behaviour every competing bot gets wrong, and the reason a reviewer's
//! reply thread survives a force-push.
//!
//! The body is the verdict and its reason, what fails the check, where
//! to look, one cluster table, one `<details>` per cluster carrying the
//! symbol ledger, and a Mermaid graph of the dependency shape the split
//! produced. GitHub caps a comment body at 65_536 characters and
//! silently truncates past it, so [`MAX_CHARS`] leaves headroom and the
//! cluster details are dropped smallest-first when a very large PR would
//! overflow; the verdict, the failures and the hotspots never are.

use std::fmt::Write as _;

use topos_engine::functors::profunctors::uast::ledger::MatchKind;
use topos_engine::graphs::mdg::split::Reach;

use super::model::{Cluster, FileRecap, PillarDelta, PrRecap};
use super::view::{basename, hotspot_pillar, worst_span, ClusterView, RecapView, PILLARS};

pub(super) const STICKY_MARKER: &str = "<!-- topos-pr-recap:v2 -->";

/// GitHub's own limit is 65_536; stop well short of silent truncation.
const MAX_CHARS: usize = 60_000;
/// A symbol list longer than this is a wall, not evidence.
const MAX_SYMBOLS: usize = 30;
/// Failures, and hotspots, listed before the rest fold into `+N more`.
const MAX_LOCUS: usize = 5;

pub(super) fn render_github(recap: &PrRecap) -> String {
    let view = RecapView::new(recap);
    let mut head = String::new();
    head.push_str(STICKY_MARKER);
    head.push('\n');
    let _ = writeln!(head, "{}\n", title(&view));
    let _ = writeln!(head, "<sub>{}</sub>\n", view.context.join(" · "));
    let _ = writeln!(head, "**Why:** {}\n", recap.reason);
    let _ = writeln!(head, "{}\n", summary(&view));
    head.push_str(&failures(&view));
    head.push_str(&hotspots(recap));
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

fn title(view: &RecapView<'_>) -> String {
    let recap = view.recap;
    let scope = &recap.scope;
    format!(
        "### {} {} · Topos structural review of {} · {} files · +{}/−{}{}",
        recap.readiness.mark(),
        recap.readiness.word(),
        view.subject,
        scope.files_scored,
        scope.lines_added,
        scope.lines_removed,
        view.incomplete_note()
    )
}

fn summary(view: &RecapView<'_>) -> String {
    let tally = &view.tally;
    let mut sentences = Vec::new();
    sentences.push(format!(
        "{} medal{} moved up, {} moved down{}.",
        tally.up,
        if tally.up == 1 { "" } else { "s" },
        tally.down,
        if tally.new_medals.is_empty() {
            String::new()
        } else {
            format!("; {} new files arrived as {}", tally.new, tally.new_medals)
        }
    ));
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

/// `+2 more — see --json.` under a list or table that was capped.
fn more_line(total: usize) -> String {
    if total > MAX_LOCUS {
        format!("+{} more — see `--json`.\n\n", total - MAX_LOCUS)
    } else {
        String::new()
    }
}

/// One bullet per item that fails the check, in the card's row words.
fn failures(view: &RecapView<'_>) -> String {
    if view.failures.is_empty() {
        return String::new();
    }
    let mut out = String::from("**Failing the check**\n\n");
    for failure in view.failures.iter().take(MAX_LOCUS) {
        let split = failure
            .split_into
            .map_or_else(String::new, |children| format!(" → {children} files"));
        let _ = writeln!(
            out,
            "- {} `{}`{split} — {}",
            failure.word, failure.path, failure.cause
        );
    }
    out.push('\n');
    out.push_str(&more_line(view.failures.len()));
    out
}

/// Every hotspot's location, finding and fix, in the order the data
/// builder ranked them.
fn hotspots(recap: &PrRecap) -> String {
    if recap.hotspots.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "**Where to look**\n\n| # | Location | Pillar | Finding | Fix |\n|---|---|---|---|---|\n",
    );
    for (index, spot) in recap.hotspots.iter().take(MAX_LOCUS).enumerate() {
        let _ = writeln!(
            out,
            "| {} | `{}:{}` | {} | {} | {} |",
            index + 1,
            escape(&spot.path),
            spot.line,
            hotspot_pillar(spot),
            escape(&spot.detail),
            escape(&spot.advice)
        );
    }
    out.push('\n');
    out.push_str(&more_line(recap.hotspots.len()));
    out
}

fn escape(text: &str) -> String {
    text.replace('|', "\\|")
}

fn medal_of(file: Option<&FileRecap>) -> String {
    file.and_then(|file| file.medal_after.as_ref()).map_or_else(
        || "unparsed".to_string(),
        |medal| format!("{} {}", medal.symbol, medal.tier),
    )
}

fn symbol_of(file: Option<&FileRecap>) -> &str {
    file.and_then(|file| file.medal_after.as_ref())
        .map_or("·", |medal| medal.symbol.as_str())
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

/// `✓`, `X`, or `✓→X` when the pillar was lost; `·` when not measured.
fn pillar_cell(delta: Option<&PillarDelta>) -> &'static str {
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

fn pillar_columns(file: Option<&FileRecap>) -> String {
    PILLARS
        .iter()
        .map(|key| pillar_cell(file.and_then(|file| file.pillars.get(*key))))
        .collect::<Vec<_>>()
        .join(" ")
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
        "| File | Medal | S C E N | Worst fn | Fan-in | Reach |\n|---|---|---|---|---|---|\n",
    );
    let _ = writeln!(
        out,
        "| `{}` | {} | {} | {} | {} | parent |",
        escape(&cluster.parent),
        medal_of(cv.parent),
        pillar_columns(cv.parent),
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
            pillar_columns(*file),
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

fn footer(recap: &PrRecap) -> String {
    let subject = recap.review.as_ref().map_or_else(
        || "--base &lt;rev&gt;".to_string(),
        |review| review.number.to_string(),
    );
    format!(
        "<sub>Deterministic, no LLM. {} <code>topos pr-recap {subject} --json</code> reproduces this document.</sub>",
        recap.non_claim
    )
}

#[cfg(test)]
mod tests {
    use super::{render_github, MAX_CHARS, STICKY_MARKER};
    use crate::commands::pr_recap::fixtures::{
        fixture_lateral_loss, fixture_losses, fixture_many_clusters, fixture_plain, fixture_pr5,
        hotspot, LATERAL_LOSS,
    };

    #[test]
    fn pr5_is_a_sticky_comment() {
        let body = render_github(&fixture_pr5());
        assert!(body.starts_with(STICKY_MARKER), "{body}");
        assert!(body.contains("### ✓ READY"), "{body}");
        assert!(
            body.contains("<sub>priority secure · COMPOSABLE measured · 1 skipped</sub>"),
            "{body}"
        );
        assert!(body.contains("<details>"), "{body}");
        assert!(body.contains("```mermaid"), "{body}");
        assert!(body.contains("graph LR"), "{body}");
        assert!(body.contains("| Cluster | Medal | Worst fn |"), "{body}");
        assert!(body.contains("| File | Medal | S C E N |"), "{body}");
        assert!(body.contains("reproduces this document"), "{body}");
        assert!(!body.contains("**Failing the check**"), "{body}");
        assert!(body.chars().count() < MAX_CHARS, "{}", body.chars().count());
    }

    /// A red check must say which file failed, why, and what to change,
    /// above the cluster sections a length cap could drop.
    #[test]
    fn a_regression_names_the_file_the_cause_and_the_fix() {
        let mut recap = fixture_losses();
        recap.hotspots = vec![hotspot(
            "topos/engine/src/functors/probes/cpg/taint.rs",
            88,
            "cpg.dangerous_calls",
        )];
        let body = render_github(&recap);
        assert!(body.starts_with(STICKY_MARKER), "{body}");
        assert!(
            body.contains(&format!("**Why:** {}\n", recap.reason)),
            "{body}"
        );
        assert!(
            body.contains(
                "- X LOST `topos/engine/src/functors/probes/cpg/taint.rs` — lost SIMPLE, SECURE"
            ),
            "{body}"
        );
        assert!(
            body.contains("- X SPLIT `topos/engine/src/functors/probes/cpg/taint.rs` → 2 files — "),
            "{body}"
        );
        assert!(
            body.contains(
                "| 1 | `topos/engine/src/functors/probes/cpg/taint.rs:88` | SECURE | finding at line 88 | Change line 88 so it clears the gate. |"
            ),
            "{body}"
        );
        let locus = body.find("**Where to look**").expect("a hotspot table");
        assert!(locus < body.find("| Cluster |").expect("a cluster table"));
    }

    /// A file that lost SIMPLE while gaining NAVIGABLE fails the check as
    /// a loss, not as a held medal with an unexplained score move.
    #[test]
    fn a_pillar_lost_while_another_is_gained_fails_as_a_loss() {
        let body = render_github(&fixture_lateral_loss());
        assert!(body.contains("**Failing the check**"), "{body}");
        assert!(
            body.contains(&format!("- X LOST `{LATERAL_LOSS}` — lost SIMPLE\n")),
            "{body}"
        );
        assert!(!body.contains("· HELD"), "{body}");
    }

    #[test]
    fn a_long_hotspot_list_folds_into_more() {
        let mut recap = fixture_plain();
        recap.hotspots = (1..=7)
            .map(|line| hotspot("src/a.rs", line, "ast.max_function_complexity"))
            .collect();
        let body = render_github(&recap);
        assert!(body.contains("| 5 | `src/a.rs:5` |"), "{body}");
        assert!(!body.contains("| 6 |"), "{body}");
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
}
