//! Split detection — recognises "one file became several" across a diff.
//!
//! Given the MDG of the repository **before** a change (`base`) and
//! **after** it (`head`), plus the list of changed paths, this module
//! reconstructs the *refactoring intent*: which added files are pieces
//! carved out of which modified/deleted file, which symbols travelled,
//! which were lost, and which are genuinely new.
//!
//! The approach is RefDiff-style: the **edge is the evidence** (the parent
//! still imports the child, or still calls into it), and symbol containment
//! corroborates it (a name defined in the parent at base is defined in the
//! child at head). Either alone attaches a child; neither leaves it
//! unclustered. Nothing here does I/O — it is pure graph arithmetic over
//! two [`ModuleDependencyGraph`]s.
//!
//! Reach tells a reviewer whether a split actually decomposed anything:
//! a [`Reach::Private`] child is only imported by its parent (the split is
//! internal bookkeeping), whereas a [`Reach::Shared`] child earned other
//! consumers (the split published a real seam).
//!
//! Every reported type is `Serialize` so the whole [`SplitReport`] can be
//! handed to an agent as JSON; [`FileChange`] and [`Reach`] render lowercase.
//! [`ChangedFile`] is *input*, not output, so it is deliberately not
//! serializable.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::functors::probes::mdg::coupling::owning_file;
use crate::functors::probes::mdg::fan::calculate_fan_in_out;
use crate::graphs::mdg::object::ModuleDependencyGraph;

/// How a path changed between the two revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileChange {
    Added,
    Modified,
    Deleted,
    /// Never a split parent or child candidate on its own — a rename is a
    /// move of the whole file, not a decomposition. Renamed paths do still
    /// participate in the "defined in *any* changed file" unions that decide
    /// [`SplitCluster::lost`] and [`SplitCluster::new_symbols`], so a symbol
    /// that merely followed a renamed file is not reported as lost.
    Renamed,
}

/// One entry of the diff under analysis.
#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: String,
    pub change: FileChange,
}

/// A symbol defined in one file at base and another at head.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolMove {
    pub name: String,
    /// Node label of the symbol at head (`Function`, `Class`, …).
    pub kind: String,
    pub from: String,
    pub to: String,
}

/// A symbol that exists in a child at head and in no changed file at base.
#[derive(Debug, Clone, Serialize)]
pub struct NewSymbol {
    pub name: String,
    pub kind: String,
    pub file: String,
}

/// Whether a child module is visible beyond its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Reach {
    /// Imported/called by nobody but the parent — the split is internal.
    Private,
    /// Has consumers other than the parent — the split published a seam.
    Shared,
}

/// One added file attributed to a parent.
#[derive(Debug, Clone, Serialize)]
pub struct ChildModule {
    pub path: String,
    /// Distinct repo-relative paths of other files depending on this one
    /// at head, sorted.
    pub importers: Vec<String>,
    pub reach: Reach,
    /// How many symbols travelled from the parent into this child.
    pub moved_in: usize,
}

/// A parent file and the children carved out of it.
#[derive(Debug, Clone, Serialize)]
pub struct SplitCluster {
    pub parent: String,
    /// Sorted by `moved_in` descending, then `path` ascending.
    pub children: Vec<ChildModule>,
    /// Defined in the parent at base, defined in a child at head. Sorted by
    /// `(to, name)`.
    pub moved: Vec<SymbolMove>,
    /// Defined in a child at head, defined in no changed file at base.
    /// Sorted by `(file, name)`.
    pub new_symbols: Vec<NewSymbol>,
    /// Names defined in the parent at base and in no changed file at head.
    /// Sorted.
    pub lost: Vec<String>,
    /// Parent fan-out measured on the base graph.
    pub parent_fan_out_before: usize,
    /// Parent fan-out measured on the head graph, raw.
    pub parent_fan_out_after: usize,
    /// Parent fan-out on the head graph, ignoring callees the children own.
    /// If this is far below [`Self::parent_fan_out_after`], the parent did
    /// not shed dependencies — it merely routed them through the children.
    pub parent_fan_out_after_excluding_children: usize,
}

/// Everything split detection found in one diff.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SplitReport {
    pub clusters: Vec<SplitCluster>,
    /// Added files with neither moved symbols nor an edge from any parent.
    pub unclustered_added: Vec<String>,
    /// Symbols that hopped between two modified files without forming a
    /// cluster. Reported, never clustered. Sorted by `(from, to, name)`.
    pub moved_between_existing: Vec<SymbolMove>,
}

// --- Primitives --------------------------------------------------------

/// `(name, label)` of every symbol transitively contained in `path`'s File
/// node, sorted. Empty when the graph has no node for `path`.
///
/// Nested `File` nodes (legacy `CONTAINS` Folder→File stores) are skipped so
/// a folder-shaped entry never lends its children's symbols to a file.
pub fn defined_symbols(graph: &ModuleDependencyGraph, path: &str) -> Vec<(String, String)> {
    let Some(file_id) = graph.file_node_id_for(path) else {
        return Vec::new();
    };
    let mut out: BTreeSet<(String, String)> = BTreeSet::new();
    for sid in graph.all_contained_symbols(file_id) {
        let Some(node) = graph.get_node(&sid) else {
            continue;
        };
        if node.label == "File" {
            continue;
        }
        let Some(name) = node.properties.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        out.insert((name.to_string(), node.label.clone()));
    }
    out.into_iter().collect()
}

/// Repo-relative paths of other files depending on `path` at this revision:
/// a File→File `IMPORTS` edge, or a `CALLS` edge from a foreign symbol into
/// one of this file's symbols. Sorted and deduplicated.
pub fn importing_files(graph: &ModuleDependencyGraph, path: &str) -> Vec<String> {
    importing_file_ids(graph, path)
        .into_iter()
        .filter_map(|id| file_path_of(graph, &id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// [`calculate_fan_in_out`]'s `fan_out` for `path`, skipping callees owned by
/// any of `excluded_files`.
///
/// "External" keeps the same meaning as in [`calculate_fan_in_out`]: callees
/// inside the file's own containment tree never counted and still don't.
pub fn fan_out_excluding(
    graph: &ModuleDependencyGraph,
    path: &str,
    excluded_files: &[&str],
) -> usize {
    let Some(file_id) = graph.file_node_id_for(path).map(str::to_string) else {
        return 0;
    };
    let symbol_ids = symbol_id_set(graph, &file_id);
    let excluded_ids: HashSet<String> = excluded_files
        .iter()
        .filter_map(|p| graph.file_node_id_for(p).map(str::to_string))
        .collect();

    let mut callees: HashSet<String> = HashSet::new();
    for sid in &symbol_ids {
        for rel in graph.outgoing(sid, Some("CALLS")) {
            if symbol_ids.contains(&rel.target_id) {
                continue;
            }
            let owner_excluded = owning_file(graph, &rel.target_id)
                .is_some_and(|owner| excluded_ids.contains(&owner));
            if !owner_excluded {
                callees.insert(rel.target_id.clone());
            }
        }
    }
    callees.len()
}

// --- Detection ---------------------------------------------------------

/// Reconstruct split clusters from a base graph, a head graph and a diff.
///
/// See the module docs for the evidence model; the rule numbers below match
/// the specification this implements.
pub fn detect_splits(
    base: &ModuleDependencyGraph,
    head: &ModuleDependencyGraph,
    changed: &[ChangedFile],
) -> SplitReport {
    // Rule 1: candidates.
    let children: Vec<&str> = sorted_paths(changed, |c| c == FileChange::Added);
    let parents: Vec<&str> = sorted_paths(changed, |c| {
        c == FileChange::Modified || c == FileChange::Deleted
    });
    if children.is_empty() {
        return SplitReport {
            moved_between_existing: moved_between_existing(base, head, changed, &HashSet::new()),
            ..Default::default()
        };
    }

    // Symbol tables, computed once per path per revision.
    let base_defs: BTreeMap<&str, Vec<(String, String)>> = changed
        .iter()
        .map(|c| (c.path.as_str(), defined_symbols(base, &c.path)))
        .collect();
    let head_defs: BTreeMap<&str, Vec<(String, String)>> = changed
        .iter()
        .map(|c| (c.path.as_str(), defined_symbols(head, &c.path)))
        .collect();
    let base_names_all = name_union(&base_defs);
    let head_names_all = name_union(&head_defs);

    // Rule 2 (ambiguity): a name defined in more than one changed *parent
    // candidate* at base cannot be attributed by containment alone.
    let mut owners_at_base: HashMap<&str, Vec<&str>> = HashMap::new();
    for parent in &parents {
        for (name, _) in &base_defs[parent] {
            owners_at_base.entry(name).or_default().push(parent);
        }
    }

    // Rule 2 (edge evidence): imports(P, C) on the head graph.
    let mut imports_edge: HashMap<(&str, &str), bool> = HashMap::new();
    for parent in &parents {
        for child in &children {
            imports_edge.insert((parent, child), depends_on(head, parent, child));
        }
    }

    // moved(P, C), after ambiguity resolution.
    let mut moved_pairs: HashMap<(&str, &str), Vec<(String, String)>> = HashMap::new();
    for child in &children {
        for (name, kind) in &head_defs[child] {
            let Some(owners) = owners_at_base.get(name.as_str()) else {
                continue;
            };
            let parent: &str = if owners.len() == 1 {
                owners[0]
            } else {
                // Ambiguous: keep it only if exactly one base owner has an
                // edge into this child at head.
                let mut importing = owners
                    .iter()
                    .copied()
                    .filter(|p| imports_edge[&(*p, *child)]);
                match (importing.next(), importing.next()) {
                    (Some(only), None) => only,
                    _ => continue,
                }
            };
            moved_pairs
                .entry((parent, child))
                .or_default()
                .push((name.clone(), kind.clone()));
        }
    }

    // Rule 3: attach each child to exactly one parent.
    let mut attached: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut unclustered_added: Vec<String> = Vec::new();
    for child in &children {
        let best = parents
            .iter()
            .copied()
            .max_by(|a, b| {
                let count = |p: &str| moved_pairs.get(&(p, *child)).map_or(0, Vec::len);
                count(a)
                    .cmp(&count(b))
                    .then_with(|| imports_edge[&(*a, *child)].cmp(&imports_edge[&(*b, *child)]))
                    // `parents` is sorted ascending and `max_by` keeps the
                    // last maximum, so invert to land on the smallest path.
                    .then_with(|| b.cmp(a))
            })
            .filter(|p| moved_pairs.contains_key(&(*p, *child)) || imports_edge[&(*p, *child)]);
        match best {
            Some(parent) => attached.entry(parent).or_default().push(child),
            None => unclustered_added.push((*child).to_string()),
        }
    }

    // Rule 4: build the clusters.
    let mut clusters: Vec<SplitCluster> = Vec::new();
    let mut clustered_names: HashSet<String> = HashSet::new();
    for (parent, kids) in &attached {
        let parent_id = head.file_node_id_for(parent).map(str::to_string);
        let excluded: Vec<&str> = kids.to_vec();

        let mut children_out: Vec<ChildModule> = kids
            .iter()
            .map(|child| {
                let moved_in = moved_pairs.get(&(*parent, *child)).map_or(0, Vec::len);
                let importer_ids = importing_file_ids(head, child);
                // Reach in node-id space: graph `filePath`s need not equal
                // the diff's path strings (suffix matching), so comparing
                // rendered paths here would be a different question.
                let reach = if importer_ids.iter().all(|id| Some(id) == parent_id.as_ref()) {
                    Reach::Private
                } else {
                    Reach::Shared
                };
                ChildModule {
                    path: (*child).to_string(),
                    importers: importer_ids
                        .iter()
                        .filter_map(|id| file_path_of(head, id))
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    reach,
                    moved_in,
                }
            })
            .collect();
        children_out.sort_by(|a, b| {
            b.moved_in
                .cmp(&a.moved_in)
                .then_with(|| a.path.cmp(&b.path))
        });

        let mut moved: Vec<SymbolMove> = kids
            .iter()
            .flat_map(|child| {
                moved_pairs
                    .get(&(*parent, *child))
                    .into_iter()
                    .flatten()
                    .map(move |(name, kind)| SymbolMove {
                        name: name.clone(),
                        kind: kind.clone(),
                        from: (*parent).to_string(),
                        to: (*child).to_string(),
                    })
            })
            .collect();
        moved.sort_by(|a, b| a.to.cmp(&b.to).then_with(|| a.name.cmp(&b.name)));
        clustered_names.extend(moved.iter().map(|m| m.name.clone()));

        let mut new_symbols: Vec<NewSymbol> = kids
            .iter()
            .flat_map(|child| {
                head_defs[*child]
                    .iter()
                    .filter(|(name, _)| !base_names_all.contains(name.as_str()))
                    .map(move |(name, kind)| NewSymbol {
                        name: name.clone(),
                        kind: kind.clone(),
                        file: (*child).to_string(),
                    })
            })
            .collect();
        new_symbols.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.name.cmp(&b.name)));

        let lost: Vec<String> = base_defs[*parent]
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| !head_names_all.contains(name.as_str()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        clusters.push(SplitCluster {
            parent: (*parent).to_string(),
            children: children_out,
            moved,
            new_symbols,
            lost,
            parent_fan_out_before: raw_fan_out(base, parent),
            parent_fan_out_after: raw_fan_out(head, parent),
            parent_fan_out_after_excluding_children: fan_out_excluding(head, parent, &excluded),
        });
    }
    clusters.sort_by(|a, b| a.parent.cmp(&b.parent));
    unclustered_added.sort();

    SplitReport {
        clusters,
        unclustered_added,
        moved_between_existing: moved_between_existing(base, head, changed, &clustered_names),
    }
}

// --- Helpers -----------------------------------------------------------

fn sorted_paths(changed: &[ChangedFile], keep: impl Fn(FileChange) -> bool) -> Vec<&str> {
    let mut paths: Vec<&str> = changed
        .iter()
        .filter(|c| keep(c.change))
        .map(|c| c.path.as_str())
        .collect();
    paths.sort_unstable();
    paths.dedup();
    paths
}

fn name_union<'a>(defs: &'a BTreeMap<&'a str, Vec<(String, String)>>) -> HashSet<&'a str> {
    defs.values()
        .flatten()
        .map(|(name, _)| name.as_str())
        .collect()
}

fn symbol_id_set(graph: &ModuleDependencyGraph, file_id: &str) -> HashSet<String> {
    let mut ids: HashSet<String> = graph.all_contained_symbols(file_id).into_iter().collect();
    ids.insert(file_id.to_string());
    ids
}

fn file_path_of(graph: &ModuleDependencyGraph, node_id: &str) -> Option<String> {
    graph
        .get_node(node_id)?
        .properties
        .get("filePath")?
        .as_str()
        .map(str::to_string)
}

fn raw_fan_out(graph: &ModuleDependencyGraph, path: &str) -> usize {
    graph
        .file_node_id_for(path)
        .map(|id| calculate_fan_in_out(graph, id, None).fan_out)
        .unwrap_or(0)
}

/// Node ids of *other* File nodes depending on `path` at this revision.
fn importing_file_ids(graph: &ModuleDependencyGraph, path: &str) -> Vec<String> {
    let Some(file_id) = graph.file_node_id_for(path).map(str::to_string) else {
        return Vec::new();
    };
    let symbol_ids = symbol_id_set(graph, &file_id);
    let mut out: BTreeSet<String> = BTreeSet::new();

    for rel in graph.incoming(&file_id, Some("IMPORTS")) {
        if let Some(owner) = owning_file(graph, &rel.source_id) {
            if owner != file_id {
                out.insert(owner);
            }
        }
    }
    for sid in &symbol_ids {
        for rel in graph.incoming(sid, Some("CALLS")) {
            if symbol_ids.contains(&rel.source_id) {
                continue;
            }
            if let Some(owner) = owning_file(graph, &rel.source_id) {
                if owner != file_id {
                    out.insert(owner);
                }
            }
        }
    }
    out.into_iter().collect()
}

/// `imports(P, C)`: a File→File `IMPORTS` edge, or any P symbol `CALLS` any
/// C symbol.
fn depends_on(graph: &ModuleDependencyGraph, from: &str, to: &str) -> bool {
    let (Some(from_id), Some(to_id)) = (
        graph.file_node_id_for(from).map(str::to_string),
        graph.file_node_id_for(to).map(str::to_string),
    ) else {
        return false;
    };
    if graph
        .outgoing(&from_id, Some("IMPORTS"))
        .iter()
        .any(|r| r.target_id == to_id)
    {
        return true;
    }
    let to_symbols = symbol_id_set(graph, &to_id);
    symbol_id_set(graph, &from_id).iter().any(|sid| {
        graph
            .outgoing(sid, Some("CALLS"))
            .iter()
            .any(|r| to_symbols.contains(&r.target_id))
    })
}

/// Rule 5 — names that hopped between two `Modified` files, excluding any
/// already accounted for by a cluster.
fn moved_between_existing(
    base: &ModuleDependencyGraph,
    head: &ModuleDependencyGraph,
    changed: &[ChangedFile],
    clustered_names: &HashSet<String>,
) -> Vec<SymbolMove> {
    let modified: Vec<&str> = sorted_paths(changed, |c| c == FileChange::Modified);
    let mut out: Vec<SymbolMove> = Vec::new();
    for from in &modified {
        let base_here = defined_symbols(base, from);
        let head_here: HashSet<String> = defined_symbols(head, from)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        for to in &modified {
            if from == to {
                continue;
            }
            for (name, kind) in defined_symbols(head, to) {
                let left_source = base_here.iter().any(|(n, _)| *n == name)
                    && !head_here.contains(&name)
                    && !clustered_names.contains(&name);
                if left_source {
                    out.push(SymbolMove {
                        name,
                        kind,
                        from: (*from).to_string(),
                        to: (*to).to_string(),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then_with(|| a.to.cmp(&b.to))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::mdg::models::{GraphNode, GraphRelationship};
    use serde_json::Value;

    fn file_node(path: &str) -> GraphNode {
        GraphNode {
            id: format!("File:{path}"),
            label: "File".to_string(),
            properties: HashMap::from([
                ("filePath".to_string(), Value::String(path.to_string())),
                ("name".to_string(), Value::String(path.to_string())),
            ]),
        }
    }

    fn symbol_node(id: &str, label: &str, name: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            label: label.to_string(),
            properties: HashMap::from([("name".to_string(), Value::String(name.to_string()))]),
        }
    }

    fn rel(source: &str, target: &str, rel_type: &str) -> GraphRelationship {
        GraphRelationship {
            id: format!("{source}-{rel_type}->{target}"),
            source_id: source.to_string(),
            target_id: target.to_string(),
            rel_type: rel_type.to_string(),
            confidence: 1.0,
            reason: String::new(),
            properties: Default::default(),
        }
    }

    /// Adds a File node plus `DEFINES`-owned symbols in one call.
    fn add_file(g: &mut ModuleDependencyGraph, path: &str, symbols: &[(&str, &str)]) {
        g.add_node(file_node(path));
        for (name, kind) in symbols {
            let id = format!("{kind}:{path}:{name}");
            g.add_node(symbol_node(&id, kind, name));
            g.add_relationship(rel(&format!("File:{path}"), &id, "DEFINES"));
        }
    }

    fn changed(entries: &[(&str, FileChange)]) -> Vec<ChangedFile> {
        entries
            .iter()
            .map(|(p, c)| ChangedFile {
                path: (*p).to_string(),
                change: *c,
            })
            .collect()
    }

    #[test]
    fn defined_symbols_sorted_and_empty_for_unknown_file() {
        let mut g = ModuleDependencyGraph::new("x");
        add_file(&mut g, "a.ts", &[("zed", "Function"), ("alpha", "Class")]);
        assert_eq!(
            defined_symbols(&g, "a.ts"),
            vec![
                ("alpha".to_string(), "Class".to_string()),
                ("zed".to_string(), "Function".to_string()),
            ]
        );
        assert!(defined_symbols(&g, "nope.ts").is_empty());
    }

    /// (a) One parent, two children, three moved symbols; one child is
    /// private to the parent, the other is imported by an unrelated file.
    #[test]
    fn split_one_parent_two_children_reach_and_moves() {
        let mut base = ModuleDependencyGraph::new("parent.ts");
        add_file(
            &mut base,
            "src/parent.ts",
            &[
                ("render", "Function"),
                ("parse", "Function"),
                ("Config", "Interface"),
                ("stays", "Function"),
            ],
        );
        add_file(&mut base, "src/other.ts", &[("consume", "Function")]);

        let mut head = ModuleDependencyGraph::new("parent.ts");
        add_file(&mut head, "src/parent.ts", &[("stays", "Function")]);
        // Stored with an absolute prefix while the diff says
        // `src/child_a.ts`: `file_node_id_for` suffix-matches, which is why
        // reach is decided in node-id space rather than on rendered paths.
        add_file(
            &mut head,
            "/repo/src/child_a.ts",
            &[("render", "Function"), ("helper", "Function")],
        );
        add_file(
            &mut head,
            "src/child_b.ts",
            &[("parse", "Function"), ("Config", "Interface")],
        );
        add_file(&mut head, "src/other.ts", &[("consume", "Function")]);
        head.add_relationship(rel(
            "File:src/parent.ts",
            "File:/repo/src/child_a.ts",
            "IMPORTS",
        ));
        head.add_relationship(rel("File:src/parent.ts", "File:src/child_b.ts", "IMPORTS"));
        // child_b also has an outside consumer -> Shared.
        head.add_relationship(rel("File:src/other.ts", "File:src/child_b.ts", "IMPORTS"));

        let report = detect_splits(
            &base,
            &head,
            &changed(&[
                ("src/parent.ts", FileChange::Modified),
                ("src/child_a.ts", FileChange::Added),
                ("src/child_b.ts", FileChange::Added),
            ]),
        );

        assert_eq!(report.clusters.len(), 1);
        let cluster = &report.clusters[0];
        assert_eq!(cluster.parent, "src/parent.ts");
        // child_b moved 2, child_a moved 1 -> descending by moved_in.
        assert_eq!(
            cluster
                .children
                .iter()
                .map(|c| (c.path.as_str(), c.moved_in, c.reach))
                .collect::<Vec<_>>(),
            vec![
                ("src/child_b.ts", 2, Reach::Shared),
                ("src/child_a.ts", 1, Reach::Private),
            ]
        );
        assert_eq!(
            cluster.children[1].importers,
            vec!["src/parent.ts".to_string()]
        );
        assert_eq!(
            cluster.children[0].importers,
            vec!["src/other.ts".to_string(), "src/parent.ts".to_string()]
        );
        assert_eq!(
            cluster
                .moved
                .iter()
                .map(|m| (m.name.as_str(), m.kind.as_str(), m.to.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("render", "Function", "src/child_a.ts"),
                ("Config", "Interface", "src/child_b.ts"),
                ("parse", "Function", "src/child_b.ts"),
            ]
        );
        assert!(cluster.lost.is_empty());
        assert_eq!(
            cluster
                .new_symbols
                .iter()
                .map(|n| (n.name.as_str(), n.file.as_str()))
                .collect::<Vec<_>>(),
            vec![("helper", "src/child_a.ts")]
        );
        assert!(report.unclustered_added.is_empty());
        assert!(report.moved_between_existing.is_empty());
    }

    /// (b) Edge evidence alone attaches a child with zero moved symbols.
    #[test]
    fn added_file_with_only_import_edge_still_attaches() {
        let mut base = ModuleDependencyGraph::new("p.ts");
        add_file(&mut base, "p.ts", &[("keep", "Function")]);

        let mut head = ModuleDependencyGraph::new("p.ts");
        add_file(&mut head, "p.ts", &[("keep", "Function")]);
        add_file(&mut head, "fresh.ts", &[("brandNew", "Function")]);
        head.add_relationship(rel("File:p.ts", "File:fresh.ts", "IMPORTS"));

        let report = detect_splits(
            &base,
            &head,
            &changed(&[
                ("p.ts", FileChange::Modified),
                ("fresh.ts", FileChange::Added),
            ]),
        );
        assert_eq!(report.clusters.len(), 1);
        let cluster = &report.clusters[0];
        assert_eq!(cluster.children.len(), 1);
        assert_eq!(cluster.children[0].moved_in, 0);
        assert_eq!(cluster.children[0].reach, Reach::Private);
        assert!(cluster.moved.is_empty());
        assert_eq!(cluster.new_symbols.len(), 1);
    }

    /// A deleted parent whose symbols reappear in added files is a pure
    /// split; symbols that reappear nowhere are `lost`.
    #[test]
    fn deleted_parent_is_a_pure_split_and_reports_lost() {
        let mut base = ModuleDependencyGraph::new("gone.ts");
        add_file(
            &mut base,
            "gone.ts",
            &[("kept", "Function"), ("dropped", "Function")],
        );

        let mut head = ModuleDependencyGraph::new("gone.ts");
        add_file(&mut head, "new.ts", &[("kept", "Function")]);

        let report = detect_splits(
            &base,
            &head,
            &changed(&[
                ("gone.ts", FileChange::Deleted),
                ("new.ts", FileChange::Added),
            ]),
        );
        assert_eq!(report.clusters.len(), 1);
        assert_eq!(report.clusters[0].lost, vec!["dropped".to_string()]);
        assert_eq!(report.clusters[0].parent_fan_out_after, 0);
    }

    /// (c) Neither moved symbols nor an edge -> unclustered.
    #[test]
    fn added_file_with_no_evidence_is_unclustered() {
        let mut base = ModuleDependencyGraph::new("p.ts");
        add_file(&mut base, "p.ts", &[("keep", "Function")]);

        let mut head = ModuleDependencyGraph::new("p.ts");
        add_file(&mut head, "p.ts", &[("keep", "Function")]);
        add_file(&mut head, "unrelated.ts", &[("solo", "Function")]);

        let report = detect_splits(
            &base,
            &head,
            &changed(&[
                ("p.ts", FileChange::Modified),
                ("unrelated.ts", FileChange::Added),
            ]),
        );
        assert!(report.clusters.is_empty());
        assert_eq!(report.unclustered_added, vec!["unrelated.ts".to_string()]);
    }

    /// (d) `fan_out_excluding` drops callees owned by excluded files, and
    /// still ignores calls that never left the file.
    #[test]
    fn fan_out_excluding_drops_excluded_callees() {
        let mut g = ModuleDependencyGraph::new("p.ts");
        add_file(&mut g, "p.ts", &[("a", "Function"), ("b", "Function")]);
        add_file(&mut g, "child.ts", &[("c", "Function")]);
        add_file(&mut g, "far.ts", &[("d", "Function")]);
        // intra-file call: never counted by either function.
        g.add_relationship(rel("Function:p.ts:a", "Function:p.ts:b", "CALLS"));
        g.add_relationship(rel("Function:p.ts:a", "Function:child.ts:c", "CALLS"));
        g.add_relationship(rel("Function:p.ts:a", "Function:far.ts:d", "CALLS"));

        assert_eq!(fan_out_excluding(&g, "p.ts", &[]), 2);
        assert_eq!(fan_out_excluding(&g, "p.ts", &["child.ts"]), 1);
        assert_eq!(fan_out_excluding(&g, "p.ts", &["child.ts", "far.ts"]), 0);
        assert_eq!(fan_out_excluding(&g, "missing.ts", &[]), 0);
    }

    /// `importing_files` sees CALLS edges, not just IMPORTS.
    #[test]
    fn importing_files_counts_call_edges() {
        let mut g = ModuleDependencyGraph::new("lib.ts");
        add_file(&mut g, "lib.ts", &[("util", "Function")]);
        add_file(&mut g, "caller.ts", &[("run", "Function")]);
        add_file(&mut g, "importer.ts", &[("x", "Function")]);
        g.add_relationship(rel(
            "Function:caller.ts:run",
            "Function:lib.ts:util",
            "CALLS",
        ));
        g.add_relationship(rel("File:importer.ts", "File:lib.ts", "IMPORTS"));

        assert_eq!(
            importing_files(&g, "lib.ts"),
            vec!["caller.ts".to_string(), "importer.ts".to_string()]
        );
        assert!(importing_files(&g, "lib.ts").iter().all(|p| p != "lib.ts"));
    }

    /// (e) A name defined in two changed base files is attributed to the
    /// parent that imports the child at head.
    #[test]
    fn ambiguous_name_resolves_via_import_edge() {
        let mut base = ModuleDependencyGraph::new("p1.ts");
        add_file(&mut base, "p1.ts", &[("shared", "Function")]);
        add_file(&mut base, "p2.ts", &[("shared", "Function")]);

        let mut head = ModuleDependencyGraph::new("p1.ts");
        add_file(&mut head, "p1.ts", &[]);
        add_file(&mut head, "p2.ts", &[("shared", "Function")]);
        add_file(&mut head, "kid.ts", &[("shared", "Function")]);
        // Only p1 depends on the child at head -> it wins the name.
        head.add_relationship(rel("File:p1.ts", "File:kid.ts", "IMPORTS"));

        let report = detect_splits(
            &base,
            &head,
            &changed(&[
                ("p1.ts", FileChange::Modified),
                ("p2.ts", FileChange::Modified),
                ("kid.ts", FileChange::Added),
            ]),
        );
        assert_eq!(report.clusters.len(), 1);
        assert_eq!(report.clusters[0].parent, "p1.ts");
        assert_eq!(
            report.clusters[0]
                .moved
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            vec!["shared"]
        );

        // With no edge at all the name is dropped and the child unclusters.
        let mut head2 = head.clone();
        head2.relationships.clear();
        let mut rebuilt = ModuleDependencyGraph::new("p1.ts");
        for node in head2.nodes.values() {
            rebuilt.add_node(node.clone());
        }
        let report2 = detect_splits(
            &base,
            &rebuilt,
            &changed(&[
                ("p1.ts", FileChange::Modified),
                ("p2.ts", FileChange::Modified),
                ("kid.ts", FileChange::Added),
            ]),
        );
        assert!(report2.clusters.is_empty());
        assert_eq!(report2.unclustered_added, vec!["kid.ts".to_string()]);
    }

    /// (5) A symbol hopping between two modified files is reported, not
    /// clustered.
    #[test]
    fn moved_between_existing_is_reported_only() {
        let mut base = ModuleDependencyGraph::new("a.ts");
        add_file(&mut base, "a.ts", &[("hop", "Function")]);
        add_file(&mut base, "b.ts", &[("stay", "Function")]);

        let mut head = ModuleDependencyGraph::new("a.ts");
        add_file(&mut head, "a.ts", &[]);
        add_file(
            &mut head,
            "b.ts",
            &[("stay", "Function"), ("hop", "Function")],
        );

        let report = detect_splits(
            &base,
            &head,
            &changed(&[
                ("a.ts", FileChange::Modified),
                ("b.ts", FileChange::Modified),
            ]),
        );
        assert!(report.clusters.is_empty());
        assert_eq!(report.moved_between_existing.len(), 1);
        let m = &report.moved_between_existing[0];
        assert_eq!(
            (m.name.as_str(), m.from.as_str(), m.to.as_str()),
            ("hop", "a.ts", "b.ts")
        );
    }

    /// The report serializes with lowercase enum tags — this is the shape
    /// agents consume, so a rename here is a breaking wire change.
    #[test]
    fn report_serializes_with_lowercase_enums() {
        let report = SplitReport {
            clusters: vec![SplitCluster {
                parent: "p.ts".to_string(),
                children: vec![ChildModule {
                    path: "kid.ts".to_string(),
                    importers: vec!["p.ts".to_string()],
                    reach: Reach::Private,
                    moved_in: 1,
                }],
                moved: Vec::new(),
                new_symbols: Vec::new(),
                lost: Vec::new(),
                parent_fan_out_before: 0,
                parent_fan_out_after: 0,
                parent_fan_out_after_excluding_children: 0,
            }],
            ..Default::default()
        };
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["clusters"][0]["children"][0]["reach"], "private");
        assert_eq!(
            serde_json::to_value(FileChange::Renamed).unwrap(),
            "renamed"
        );
    }

    /// Fan-out triple: raw head fan-out counts the children's symbols; the
    /// excluding variant shows the parent shed nothing.
    #[test]
    fn cluster_reports_fan_out_before_after_and_excluding_children() {
        let mut base = ModuleDependencyGraph::new("p.ts");
        add_file(&mut base, "p.ts", &[("work", "Function")]);
        add_file(&mut base, "dep.ts", &[("d", "Function")]);
        base.add_relationship(rel("Function:p.ts:work", "Function:dep.ts:d", "CALLS"));

        let mut head = ModuleDependencyGraph::new("p.ts");
        add_file(&mut head, "p.ts", &[("work", "Function")]);
        add_file(&mut head, "dep.ts", &[("d", "Function")]);
        add_file(&mut head, "kid.ts", &[("moved", "Function")]);
        head.add_relationship(rel("Function:p.ts:work", "Function:dep.ts:d", "CALLS"));
        head.add_relationship(rel("Function:p.ts:work", "Function:kid.ts:moved", "CALLS"));

        let report = detect_splits(
            &base,
            &head,
            &changed(&[
                ("p.ts", FileChange::Modified),
                ("kid.ts", FileChange::Added),
            ]),
        );
        let cluster = &report.clusters[0];
        assert_eq!(cluster.parent_fan_out_before, 1);
        assert_eq!(cluster.parent_fan_out_after, 2);
        assert_eq!(cluster.parent_fan_out_after_excluding_children, 1);
    }
}
