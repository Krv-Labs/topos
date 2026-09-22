//! GitHub sticky-comment Markdown for `topos pr-recap`.
//!
//! [`STICKY_MARKER`] is the first line so the Action can *edit* its own
//! comment instead of deleting and re-posting one on every push — the
//! behaviour every competing bot gets wrong, and the reason a reviewer's
//! reply thread survives a force-push.
//!
//! The body is a summary, one cluster table, one `<details>` per cluster
//! carrying the symbol ledger, and a Mermaid graph of the dependency
//! shape the split produced. GitHub caps a comment body at 65_536
//! characters and silently truncates past it, so [`MAX_CHARS`] leaves
//! headroom and the cluster details are dropped smallest-first when a
//! very large PR would overflow.

use std::fmt::Write as _;

use topos_engine::functors::profunctors::uast::ledger::MatchKind;
use topos_engine::graphs::mdg::split::Reach;

use super::model::{Cluster, ClusterMark, FileRecap, PrRecap};
use super::render::{
    basename, cluster_decisions, headline_mark, medal_moves, new_medal_tally, pillar_cell,
    short_rev, still_failing, worst_function_drop, PILLARS,
};

pub(super) const STICKY_MARKER: &str = "<!-- topos-pr-recap:v2 -->";

/// GitHub's own limit is 65_536; stop well short of silent truncation.
const MAX_CHARS: usize = 60_000;
/// A symbol list longer than this is a wall, not evidence.
const MAX_SYMBOLS: usize = 30;

pub(super) fn render_github(recap: &PrRecap) -> String {
    let mut head = String::new();
    head.push_str(STICKY_MARKER);
    head.push('\n');
    let _ = writeln!(head, "{}\n", title(recap));
    let _ = writeln!(head, "{}\n", summary(recap));
    if !recap.clusters.is_empty() {
        head.push_str(&cluster_table(recap));
        head.push('\n');
    }

    let details: Vec<String> = recap
        .clusters
        .iter()
        .map(|cluster| cluster_details(recap, cluster))
        .collect();
    let mut tail = String::new();
    if let Some(graph) = dependency_graph(recap) {
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

fn title(recap: &PrRecap) -> String {
    let scope = &recap.scope;
    let subject = recap.review.as_ref().map_or_else(
        || format!("{}…{}", short_rev(&recap.base), short_rev(&recap.head)),
        |review| format!("#{}", review.number),
    );
    format!(
        "### {} {} · Topos structural review of {subject} · {} files · +{}/−{}",
        headline_mark(recap.headline),
        recap.headline.word(),
        scope.files_scored,
        scope.lines_added,
        scope.lines_removed
    )
}

fn summary(recap: &PrRecap) -> String {
    let (up, down) = medal_moves(recap);
    let mut sentences = Vec::new();
    let tally = new_medal_tally(recap);
    sentences.push(format!(
        "{up} medal{} moved up, {down} moved down{}.",
        if up == 1 { "" } else { "s" },
        if tally.is_empty() {
            String::new()
        } else {
            format!("; {} new files arrived as {tally}", recap.new_files().len())
        }
    ));
    if !recap.clusters.is_empty() {
        let children: usize = recap
            .clusters
            .iter()
            .map(|cluster| cluster.children.len())
            .sum();
        sentences.push(format!(
            "{} files were split into {children}.",
            recap.clusters.len()
        ));
        if let Some((low, high)) = worst_function_drop(recap) {
            sentences.push(if low == high {
                format!("Worst-function complexity fell {low}% across the clusters.")
            } else {
                format!("Worst-function complexity fell {low}–{high}% across the clusters.")
            });
        }
        let (before, after) = cluster_decisions(recap);
        sentences.push(format!("Total decision count went {before} → {after}."));
    }
    if let Some((names, pillars)) = still_failing(recap) {
        sentences.push(format!(
            "{} still fail {}.",
            names.join(", "),
            pillars.join(" and ")
        ));
    }
    if let Some(project) = &recap.project {
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

fn escape(path: &str) -> String {
    path.replace('|', "\\|")
}

fn medal_of(file: Option<&FileRecap>) -> String {
    file.and_then(|file| file.medal_after.as_ref()).map_or_else(
        || "unparsed".to_string(),
        |medal| format!("{} {}", medal.symbol, medal.tier),
    )
}

fn file_at<'a>(recap: &'a PrRecap, path: &str) -> Option<&'a FileRecap> {
    recap.files.iter().find(|file| file.path == path)
}

fn cluster_mark(mark: ClusterMark) -> char {
    match mark {
        ClusterMark::Ok => '✓',
        ClusterMark::Warn => '!',
        ClusterMark::Fail => 'X',
    }
}

fn cluster_table(recap: &PrRecap) -> String {
    let mut out = String::from(
        "| Cluster | Medal | Worst fn | Decisions | Lines | Moved / new | Parent fan-out |\n\
         |---|---|---|---|---|---|---|\n",
    );
    for cluster in &recap.clusters {
        let worst = match (
            cluster.worst_function_before.as_ref(),
            cluster.worst_function_after.as_ref(),
        ) {
            (Some(before), Some(after)) => format!("{} → {}", before.complexity, after.complexity),
            _ => "·".to_string(),
        };
        let fan_out = match (cluster.parent_fan_out_before, cluster.parent_fan_out_after) {
            (Some(before), Some(after)) => format!("{before} → {after}"),
            _ => "·".to_string(),
        };
        let moved: usize = cluster.children.iter().map(|child| child.moved_in).sum();
        let _ = writeln!(
            out,
            "| {} `{}` → {} files | {} | {worst} | {} → {} | {} → {} | {moved} / {} | {fan_out} |",
            cluster_mark(cluster.mark),
            escape(&cluster.parent),
            cluster.children.len(),
            medal_of(file_at(recap, &cluster.parent)),
            cluster.decisions_before,
            cluster.decisions_after,
            cluster.lines_before,
            cluster.lines_after,
            cluster.symbols_new.len()
        );
    }
    out
}

fn pillar_columns(file: Option<&FileRecap>) -> String {
    PILLARS
        .iter()
        .map(|(key, _)| pillar_cell(file.and_then(|file| file.pillars.get(*key))))
        .collect::<Vec<_>>()
        .join(" ")
}

fn worst_column(file: Option<&FileRecap>) -> String {
    match file.map(|file| (&file.worst_function_before, &file.worst_function_after)) {
        Some((Some(before), Some(after))) => {
            format!("{} → {}", before.complexity, after.complexity)
        }
        Some((None, Some(after))) => format!("{}", after.complexity),
        _ => "·".to_string(),
    }
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
        .filter(|entry| {
            matches!(
                entry.kind,
                MatchKind::MovedIdentical | MatchKind::MovedModified | MatchKind::Renamed
            )
        })
        .filter_map(|entry| {
            let after = entry.after.as_ref()?;
            Some(format!("{} → {}", after.name, basename(&after.file)))
        })
        .collect()
}

fn cluster_details(recap: &PrRecap, cluster: &Cluster) -> String {
    let mut out = format!(
        "<details>\n<summary>{} <code>{}</code> → {} files</summary>\n\n",
        cluster_mark(cluster.mark),
        escape(&cluster.parent),
        cluster.children.len()
    );
    out.push_str(
        "| File | Medal | S C E N | Worst fn | Fan-in | Reach |\n|---|---|---|---|---|---|\n",
    );
    let parent = file_at(recap, &cluster.parent);
    let _ = writeln!(
        out,
        "| `{}` | {} | {} | {} | {} | parent |",
        escape(&cluster.parent),
        medal_of(parent),
        pillar_columns(parent),
        worst_column(parent),
        parent
            .and_then(|file| file.fan_in_after)
            .map_or_else(|| "·".to_string(), |value| value.to_string())
    );
    for child in &cluster.children {
        let file = file_at(recap, &child.path);
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} |",
            escape(&child.path),
            medal_of(file),
            pillar_columns(file),
            worst_column(file),
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
fn dependency_graph(recap: &PrRecap) -> Option<String> {
    let measured = recap
        .clusters
        .iter()
        .any(|cluster| cluster.children.iter().any(|child| child.reach.is_some()));
    if !measured {
        return None;
    }
    let mut out = String::from(
        "<details>\n<summary>Dependency shape after the split</summary>\n\n```mermaid\ngraph LR\n",
    );
    for (index, cluster) in recap.clusters.iter().enumerate() {
        let parent = format!("p{index}");
        let _ = writeln!(
            out,
            "  {parent}[\"{} {}\"]",
            file_at(recap, &cluster.parent)
                .and_then(|file| file.medal_after.as_ref())
                .map_or("·", |medal| medal.symbol.as_str()),
            basename(&cluster.parent)
        );
        for (slot, child) in cluster.children.iter().enumerate() {
            let _ = writeln!(
                out,
                "  {parent} -->|{}| c{index}_{slot}[\"{} {}\"]",
                child.importers.len(),
                file_at(recap, &child.path)
                    .and_then(|file| file.medal_after.as_ref())
                    .map_or("·", |medal| medal.symbol.as_str()),
                basename(&child.path)
            );
        }
    }
    out.push_str("```\n\n</details>\n");
    Some(out)
}

fn footer(recap: &PrRecap) -> String {
    let subject = recap.review.as_ref().map_or_else(
        || "--base <rev>".to_string(),
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
    use crate::commands::pr_recap::render::{fixture_many_clusters, fixture_plain, fixture_pr5};

    #[test]
    fn pr5_is_a_sticky_comment() {
        let body = render_github(&fixture_pr5());
        assert!(body.starts_with(STICKY_MARKER), "{body}");
        assert!(body.contains("### ✓ IMPROVEMENT"), "{body}");
        assert!(body.contains("<details>"), "{body}");
        assert!(body.contains("```mermaid"), "{body}");
        assert!(body.contains("graph LR"), "{body}");
        assert!(body.contains("| Cluster | Medal | Worst fn |"), "{body}");
        assert!(body.contains("| File | Medal | S C E N |"), "{body}");
        assert!(body.contains("reproduces this document"), "{body}");
        assert!(body.chars().count() < MAX_CHARS, "{}", body.chars().count());
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
