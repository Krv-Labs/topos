//! Pass B: group split parents with the children carved out of them,
//! and mark each split from the moved-function ledger.

use std::collections::BTreeMap;
use std::path::Path;

use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::evaluation::policies::gates::GATE_SPECS;
use topos_engine::functors::profunctors::uast::ledger::{match_functions, Ledger, MatchKind};
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;
use topos_engine::graphs::mdg::split::{
    fan_out_excluding, ChangedFile, ChildModule, FileChange as SplitChange, Reach, SplitCluster,
};

use super::git::{file_change, DiffEntry};
use super::model::*;
use super::score::Scored;
use super::verdict::raw;

/// Lines that could be an import of a sibling module, for the no-graph
/// split fallback.
const IMPORT_PREFIXES: &[&str] = &["import", "from", "use", "#include", "require("];

pub(super) fn changed_list(entries: &[DiffEntry], deleted: &[String]) -> Vec<ChangedFile> {
    let mut changed: Vec<ChangedFile> = entries
        .iter()
        .map(|entry| ChangedFile {
            path: entry.path.clone(),
            change: match file_change(&entry.status) {
                FileChange::Added => SplitChange::Added,
                FileChange::Renamed => SplitChange::Renamed,
                FileChange::Modified => SplitChange::Modified,
            },
        })
        .collect();
    changed.extend(deleted.iter().map(|path| ChangedFile {
        path: path.clone(),
        change: SplitChange::Deleted,
    }));
    changed
}

/// A parent and its children, before the arithmetic is done.
struct Seed<'a> {
    parent: String,
    children: Vec<String>,
    split: Option<&'a SplitCluster>,
}

/// Group split parents with the children actually carved out of them.
///
/// Both seed passes are candidate finders only — an import line or a graph
/// edge says "the parent now uses this file", not "this file came out of
/// the parent". The ledger decides: a child survives only with moved-code
/// evidence (graph `moved_in`, or a `Moved*` match landing in it), so a
/// brand-new module the parent merely started calling renders as a plain
/// NEW row instead of a bogus `! SPLIT`.
pub(super) fn build_clusters(
    scored: &mut [Scored],
    report: &[SplitCluster],
    measured: bool,
    head_graph: Option<&ModuleDependencyGraph>,
) -> Vec<Cluster> {
    let index: BTreeMap<String, usize> = scored
        .iter()
        .enumerate()
        .map(|(i, file)| (file.recap.path.clone(), i))
        .collect();
    let seeds = if measured {
        graph_seeds(report, &index)
    } else {
        fallback_seeds(scored, &index)
    };

    let mut clusters = Vec::new();
    for seed in seeds {
        if seed.children.is_empty() {
            continue;
        }
        let parent = &scored[index[&seed.parent]];
        let kids: Vec<&Scored> = seed
            .children
            .iter()
            .map(|path| &scored[index[path]])
            .collect();
        let ledger = cluster_ledger(parent, &kids);
        let kept: Vec<String> = kids
            .iter()
            .filter(|kid| {
                let path = &kid.recap.path;
                // Graph evidence or ledger evidence; zero on both means
                // nothing travelled into this file, so it is not a child.
                reported_moved_in(&seed, path).max(moved_into(ledger.as_ref(), path)) > 0
                    // The cluster ledger is all-or-nothing over the
                    // candidate set: one unparseable sibling must not
                    // erase the evidence for the others.
                    || (ledger.is_none() && solo_moved_in(parent, kid) > 0)
            })
            .map(|kid| kid.recap.path.clone())
            .collect();
        if kept.is_empty() {
            // No child survived: the parent keeps no cluster membership.
            continue;
        }
        // One prune pass only. The ledger is re-run over the surviving set
        // only when something was pruned, so no pruned file is counted.
        let seed = Seed {
            children: kept,
            ..seed
        };
        let ledger = if seed.children.len() == kids.len() {
            ledger
        } else {
            let kids: Vec<&Scored> = seed
                .children
                .iter()
                .map(|path| &scored[index[path]])
                .collect();
            cluster_ledger(parent, &kids)
        };
        let cluster = materialize(&seed, scored, &index, head_graph, ledger);
        for (path, role) in std::iter::once((seed.parent.clone(), ClusterRole::Parent)).chain(
            seed.children
                .iter()
                .map(|child| (child.clone(), ClusterRole::Child)),
        ) {
            if let Some(i) = index.get(&path) {
                scored[*i].recap.cluster = Some(ClusterMembership {
                    parent: seed.parent.clone(),
                    role,
                });
            }
        }
        clusters.push(cluster);
    }
    clusters
}

fn graph_seeds<'a>(report: &'a [SplitCluster], index: &BTreeMap<String, usize>) -> Vec<Seed<'a>> {
    report
        .iter()
        .filter(|cluster| index.contains_key(&cluster.parent))
        .map(|cluster| Seed {
            parent: cluster.parent.clone(),
            children: cluster
                .children
                .iter()
                .map(|child| child.path.clone())
                .filter(|path| index.contains_key(path))
                .collect(),
            split: Some(cluster),
        })
        .collect()
}

/// No coupling graphs: attribute each added file to the modified file whose
/// head source imports it most often. Ties go to the smallest path.
fn fallback_seeds<'a>(scored: &[Scored], index: &BTreeMap<String, usize>) -> Vec<Seed<'a>> {
    let parents: Vec<&str> = scored
        .iter()
        .filter(|file| file.recap.change == FileChange::Modified)
        .map(|file| file.recap.path.as_str())
        .collect();
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for child in scored.iter().filter(|file| file.recap.is_new()) {
        let stem = file_stem(&child.recap.path);
        if stem.is_empty() {
            continue;
        }
        let best = parents
            .iter()
            .filter_map(|parent| {
                let source = &scored[index[*parent]].after_src;
                let hits = import_hits(source, &stem);
                (hits > 0).then_some((hits, *parent))
            })
            // Most import lines wins; on a tie the smallest path does.
            .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(a.1)));
        if let Some((_, parent)) = best {
            grouped
                .entry(parent.to_string())
                .or_default()
                .push(child.recap.path.clone());
        }
    }
    grouped
        .into_iter()
        .map(|(parent, children)| Seed {
            parent,
            children,
            split: None,
        })
        .collect()
}

fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn import_hits(source: &str, stem: &str) -> usize {
    source
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            IMPORT_PREFIXES
                .iter()
                .any(|prefix| line.starts_with(prefix))
                || line.contains("require(")
        })
        .filter(|line| line.contains(stem))
        .count()
}

fn materialize(
    seed: &Seed<'_>,
    scored: &[Scored],
    index: &BTreeMap<String, usize>,
    head_graph: Option<&ModuleDependencyGraph>,
    ledger: Option<Ledger>,
) -> Cluster {
    let parent = &scored[index[&seed.parent]];
    let kids: Vec<&Scored> = seed
        .children
        .iter()
        .map(|path| &scored[index[path]])
        .collect();

    let decisions_after = parent.recap.decisions_after.unwrap_or(0)
        + kids
            .iter()
            .map(|kid| kid.recap.decisions_after.unwrap_or(0))
            .sum::<usize>();
    let lines_after =
        parent.recap.lines_after + kids.iter().map(|kid| kid.recap.lines_after).sum::<usize>();
    let worst_function_after = std::iter::once(parent.recap.worst_function_after.clone())
        .chain(
            kids.iter()
                .map(|kid| kid.recap.worst_function_after.clone()),
        )
        .flatten()
        .max_by_key(|entry| entry.complexity);

    let kept: Vec<&str> = seed.children.iter().map(String::as_str).collect();
    let mut cluster = Cluster {
        parent: seed.parent.clone(),
        children: cluster_children(seed, ledger.as_ref()),
        // Marked below, from the finished arithmetic.
        mark: ClusterMark::Ok,
        reasons: Vec::new(),
        lines_before: parent.recap.lines_before,
        lines_after,
        decisions_before: parent.recap.decisions_before.unwrap_or(0),
        decisions_after,
        worst_function_before: parent.recap.worst_function_before.clone(),
        worst_function_after,
        parent_fan_out_before: seed.split.map(|split| split.parent_fan_out_before),
        parent_fan_out_after: seed.split.map(|split| split.parent_fan_out_after),
        // Recomputed on the head graph over the surviving children only;
        // the split report's value still counted the pruned ones.
        parent_fan_out_after_excluding_children: match head_graph {
            Some(graph) => Some(fan_out_excluding(graph, &seed.parent, &kept)),
            None => seed
                .split
                .map(|split| split.parent_fan_out_after_excluding_children),
        },
        symbols_moved: seed
            .split
            .map(|split| {
                split
                    .moved
                    .iter()
                    .filter(|entry| kept.contains(&entry.to.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        symbols_new: seed
            .split
            .map(|split| {
                split
                    .new_symbols
                    .iter()
                    .filter(|entry| kept.contains(&entry.file.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        symbols_lost: seed
            .split
            .map(|split| split.lost.clone())
            .unwrap_or_default(),
        ledger,
    };
    (cluster.mark, cluster.reasons) = cluster_mark(&cluster, parent, &kids);
    cluster
}

/// The parent at base against the parent plus every child at head.
fn cluster_ledger(parent: &Scored, kids: &[&Scored]) -> Option<Ledger> {
    let before = parent.before_snapshots.clone()?;
    let mut after = parent.after_snapshots.clone()?;
    for kid in kids {
        after.extend(kid.after_snapshots.clone()?);
    }
    Some(match_functions(before, after))
}

/// The split report's entry for one child, when the graph saw it.
fn reported<'a>(seed: &Seed<'a>, path: &str) -> Option<&'a ChildModule> {
    seed.split
        .and_then(|split| split.children.iter().find(|child| child.path == path))
}

fn reported_moved_in(seed: &Seed<'_>, path: &str) -> usize {
    reported(seed, path).map_or(0, |child| child.moved_in)
}

fn cluster_children(seed: &Seed<'_>, ledger: Option<&Ledger>) -> Vec<ClusterChild> {
    seed.children
        .iter()
        .map(|path| {
            let reported = reported(seed, path);
            ClusterChild {
                reach: reported.map(|child| child.reach),
                importers: reported
                    .map(|child| child.importers.clone())
                    .unwrap_or_default(),
                // A child whose only evidence is a moved anonymous
                // callback has graph `moved_in == 0`; the ledger sees it.
                moved_in: reported_moved_in(seed, path).max(moved_into(ledger, path)),
                path: path.clone(),
            }
        })
        .collect()
}

/// Moves from the parent into one child, ledgered on its own. Used only
/// when the whole-cluster ledger is `None` because some other candidate
/// child failed to parse.
fn solo_moved_in(parent: &Scored, kid: &Scored) -> usize {
    moved_into(cluster_ledger(parent, &[kid]).as_ref(), &kid.recap.path)
}

/// Ledger moves landing in `child`, counting nested and anonymous
/// callables: JSX extracted into a new component often moves only
/// anonymous callbacks, and that is still moved code.
fn moved_into(ledger: Option<&Ledger>, child: &str) -> usize {
    let Some(ledger) = ledger else { return 0 };
    ledger
        .matches
        .iter()
        .filter(|entry| entry.kind.is_move())
        .filter(|entry| {
            entry
                .after
                .as_ref()
                .is_some_and(|after| after.file == child)
        })
        .count()
}

fn cluster_mark(
    cluster: &Cluster,
    parent: &Scored,
    kids: &[&Scored],
) -> (ClusterMark, Vec<String>) {
    let (decisions_before, decisions_after) = (cluster.decisions_before, cluster.decisions_after);
    // Failure causes come first, so `reasons[0]` of a failed split names
    // why it failed; the headline quotes it.
    let mut reasons = Vec::new();
    let lost: Vec<&str> = parent
        .recap
        .pillars
        .iter()
        .filter(|(_, delta)| delta.lost())
        .map(|(pillar, _)| pillar.as_str())
        .collect();
    if !lost.is_empty() {
        reasons.push(format!("{} lost {}", parent.recap.path, lost.join(", ")));
    }
    // Code moved out of the parent brings its findings with it, so a child
    // failing SECURE is only new risk when the cluster as a whole has more
    // findings than the parent had.
    let findings_before = secure_findings(&parent.before);
    let findings_after = secure_findings(&parent.after)
        + kids
            .iter()
            .map(|kid| secure_findings(&kid.after))
            .sum::<usize>();
    let insecure = findings_after > findings_before;
    if insecure {
        reasons.push(format!(
            "SECURE findings rose {findings_before}→{findings_after} across the split"
        ));
    }
    let grew = cluster.ledger.as_ref().is_some_and(|ledger| {
        ledger.matches.iter().any(|entry| {
            matches!(entry.kind, MatchKind::MovedModified | MatchKind::Renamed)
                && entry.complexity_delta > 0
        })
    });
    let worst_before = cluster.worst_function_before.as_ref();
    let worst_after = cluster.worst_function_after.as_ref();
    let worst_fell = worst_before
        .zip(worst_after)
        .is_some_and(|(before, after)| after.complexity < before.complexity);
    let failed = !lost.is_empty() || insecure || (grew && !worst_fell);
    if failed && grew {
        reasons.push("a moved function gained complexity on the way".to_string());
    }
    if let (Some(before), Some(after)) = (worst_before, worst_after) {
        if before.complexity != after.complexity {
            reasons.push(format!(
                "worst function {}→{}",
                before.complexity, after.complexity
            ));
        }
    }
    if decisions_after != decisions_before {
        reasons.push(decision_reason(decisions_before, decisions_after));
    }
    let private = cluster
        .children
        .iter()
        .filter(|child| child.reach == Some(Reach::Private))
        .count();
    if private > 0 {
        reasons.push(format!(
            "{private} of {} children private",
            cluster.children.len()
        ));
    }

    if failed {
        return (ClusterMark::Fail, reasons);
    }
    let bloated = decisions_after as f64 > decisions_before as f64 * (1.0 + CLUSTER_GROWTH_WARN);
    let sloppy = kids.iter().any(|kid| {
        kid.recap
            .medal_after
            .as_ref()
            .map(|medal| medal.tier == "SLOP")
            .unwrap_or(true)
    });
    if sloppy {
        reasons.push("a child is SLOP or did not parse".to_string());
    }
    if bloated || sloppy {
        (ClusterMark::Warn, reasons)
    } else {
        (ClusterMark::Ok, reasons)
    }
}

/// Dangerous calls plus taint flows: every metric that gates SECURE.
fn secure_findings(result: &ClassificationResult) -> usize {
    GATE_SPECS
        .iter()
        .filter(|spec| spec.pillar == "secure" && spec.gates_achieved)
        .filter_map(|spec| raw(result, spec.metric))
        .sum()
}

fn decision_reason(before: usize, after: usize) -> String {
    if after > before {
        let percent = percent_change(before, after);
        format!("decisions rose {before}→{after} (+{percent}%)")
    } else {
        format!("decisions fell {before}→{after}")
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{commit_all, recap, write_files, write_repo};
    use super::*;

    const ALPHA: &str = "def alpha(x):\n    if x:\n        return 1\n    return 0\n";

    const BETA: &str = "def beta(x):\n    if x:\n        return 2\n    return 0\n";

    const GAMMA: &str = "def gamma(x):\n    if x:\n        return 3\n    return 0\n";

    /// A brand-new module the parent merely starts importing is not an
    /// extraction: RefDiff's rule needs moved code, not just a call edge.
    #[test]
    fn a_new_module_the_parent_merely_uses_is_not_a_split() {
        let (_keep, repo) = write_repo(&[(
            "src/app.py",
            "def run(x):\n    if x:\n        return 1\n    return 0\n",
        )]);
        write_files(
            &repo,
            &[
                (
                    "src/app.py",
                    "from util import helper\n\ndef run(x):\n    if x:\n        return helper(x)\n    return 0\n",
                ),
                (
                    "src/util.py",
                    "def helper(x):\n    if x > 1:\n        return 2\n    return 3\n",
                ),
            ],
        );
        commit_all(&repo, "use a new module");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);

        assert!(
            recap.clusters.is_empty(),
            "an import edge alone is not a split: {:?}",
            recap.clusters
        );
        for file in &recap.files {
            assert!(
                file.cluster.is_none(),
                "{} must not be clustered",
                file.path
            );
        }
        let child = recap
            .files
            .iter()
            .find(|f| f.path == "src/util.py")
            .expect("new file scored");
        assert_eq!(child.change, FileChange::Added);
    }

    /// Only a *nested* callable moved into the child — the top-level
    /// wrapper there is new. Nested moves are still moved code, so the
    /// child stays in the cluster and must not render with `moved_in 0`.
    #[test]
    fn a_child_kept_alive_by_moved_callbacks() {
        let (_keep, repo) = write_repo(&[(
            "src/app.py",
            "def run(x):\n    def check(y):\n        if y > 1:\n            return 2\n        return 3\n    return check(x)\n",
        )]);
        write_files(
            &repo,
            &[
                (
                    "src/app.py",
                    "from worker import work\n\ndef run(x):\n    return work(x)\n",
                ),
                (
                    "src/worker.py",
                    "def work(x):\n    def check(y):\n        if y > 1:\n            return 2\n        return 3\n    return check(x)\n",
                ),
            ],
        );
        commit_all(&repo, "extract");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);

        assert_eq!(recap.clusters.len(), 1, "the nested move is evidence");
        let child = &recap.clusters[0].children[0];
        assert_eq!(child.path, "src/worker.py");
        assert_eq!(child.moved_in, 1, "the moved closure counts");
    }

    #[test]
    fn split_is_clustered_without_stores() {
        let big = format!("{ALPHA}\n\n{BETA}\n\n{GAMMA}");
        let (_keep, repo) = write_repo(&[("src/big.py", big.as_str())]);
        write_files(
            &repo,
            &[
                (
                    "src/big.py",
                    &format!("from helpers import beta, gamma\n\n\n{ALPHA}"),
                ),
                ("src/helpers.py", &format!("{BETA}\n\n{GAMMA}")),
            ],
        );
        commit_all(&repo, "split");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);

        assert_eq!(recap.clusters.len(), 1, "one cluster");
        let cluster = &recap.clusters[0];
        assert_eq!(cluster.parent, "src/big.py");
        assert_eq!(cluster.children.len(), 1);
        assert_eq!(cluster.children[0].path, "src/helpers.py");
        assert!(cluster.children[0].reach.is_none(), "no stores, no reach");
        let ledger = cluster.ledger.as_ref().expect("both sides parse");
        assert_eq!(ledger.totals.moved_identical, 2);
        assert_eq!(cluster.children[0].moved_in, 2);
        assert_eq!(
            cluster.decisions_before, cluster.decisions_after,
            "a pure move adds no decisions"
        );
        assert_eq!(cluster.mark, ClusterMark::Ok);

        let parent = recap
            .files
            .iter()
            .find(|f| f.path == "src/big.py")
            .expect("parent scored");
        assert_eq!(
            parent.cluster.as_ref().map(|c| c.role),
            Some(ClusterRole::Parent)
        );
        let child = recap
            .files
            .iter()
            .find(|f| f.path == "src/helpers.py")
            .expect("child scored");
        assert_eq!(
            child.cluster.as_ref().map(|c| c.role),
            Some(ClusterRole::Child)
        );
        assert_eq!(
            child.cluster.as_ref().map(|c| c.parent.as_str()),
            Some("src/big.py")
        );

        let json = serde_json::to_value(&recap).expect("recap serializes");
        assert_eq!(json["clusters"][0]["parent"], "src/big.py");
        assert_eq!(json["schema"], SCHEMA);
    }

    /// Moving a dangerous call out of the parent is not new risk: the child
    /// fails SECURE, but the split as a whole has no more findings.
    #[test]
    fn a_dangerous_call_moved_into_a_split_child_is_not_a_regression() {
        let gamma = "def gamma(cmd):\n    if cmd:\n        os.system(cmd)\n    return 0\n";
        let big = format!("import os\n\n{ALPHA}\n\n{BETA}\n\n{gamma}");
        let (_keep, repo) = write_repo(&[("src/big.py", big.as_str())]);
        write_files(
            &repo,
            &[
                (
                    "src/big.py",
                    &format!("from helpers import beta, gamma\n\n\n{ALPHA}"),
                ),
                ("src/helpers.py", &format!("import os\n\n{BETA}\n\n{gamma}")),
            ],
        );
        commit_all(&repo, "split");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        assert_eq!(recap.clusters.len(), 1);
        assert_ne!(
            recap.clusters[0].mark,
            ClusterMark::Fail,
            "{:?}",
            recap.clusters[0].reasons
        );
        let child = recap
            .files
            .iter()
            .find(|f| f.path == "src/helpers.py")
            .unwrap();
        assert_ne!(child.status, Headline::Regression);
        assert!(!recap.headline.fails_check(), "{}", recap.reason);
    }

    /// A split that brings in a dangerous call the parent never had fails,
    /// and a failed split fails the headline.
    #[test]
    fn a_split_adding_a_dangerous_call_fails_the_headline() {
        let big = format!("{ALPHA}\n\n{BETA}\n\n{GAMMA}");
        let (_keep, repo) = write_repo(&[("src/big.py", big.as_str())]);
        write_files(
            &repo,
            &[
                (
                    "src/big.py",
                    &format!("from helpers import beta, gamma\n\n\n{ALPHA}"),
                ),
                (
                    "src/helpers.py",
                    &format!(
                        "import os\n\n{BETA}\n\n{GAMMA}\n\ndef shell(cmd):\n    os.system(cmd)\n"
                    ),
                ),
            ],
        );
        commit_all(&repo, "split with a shell");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        assert_eq!(recap.clusters.len(), 1);
        let cluster = &recap.clusters[0];
        assert_eq!(cluster.mark, ClusterMark::Fail);
        assert!(
            cluster.reasons[0].contains("SECURE findings rose"),
            "{:?}",
            cluster.reasons
        );
        assert_eq!(recap.headline, Headline::Regression);
        assert!(
            recap.reason.contains("The split of src/big.py failed"),
            "{}",
            recap.reason
        );
    }

    /// A moved function that came out more complex while the worst
    /// function did not fall is an `X SPLIT`, and it fails the check.
    #[test]
    fn a_moved_function_that_grew_fails_the_headline() {
        let big = format!("{ALPHA}\n\n{BETA}\n\n{GAMMA}");
        let (_keep, repo) = write_repo(&[("src/big.py", big.as_str())]);
        let beta_grown =
            "def beta(x):\n    if x:\n        return 2\n    if x > 5:\n        return 4\n    return 0\n";
        write_files(
            &repo,
            &[
                (
                    "src/big.py",
                    &format!("from helpers import beta, gamma\n\n\n{ALPHA}"),
                ),
                ("src/helpers.py", &format!("{beta_grown}\n\n{GAMMA}")),
            ],
        );
        commit_all(&repo, "split and grow");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        assert_eq!(recap.clusters.len(), 1);
        assert_eq!(
            recap.clusters[0].mark,
            ClusterMark::Fail,
            "{:?}",
            recap.clusters[0].reasons
        );
        assert_eq!(recap.headline, Headline::Regression);
        assert_eq!(recap.check, "fail");
    }
}
