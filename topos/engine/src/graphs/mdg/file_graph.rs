//! File-level dependency graph — the MDG collapsed to one node per file.
//!
//! GitNexus records `IMPORTS` and `CALLS` between arbitrary symbols. A PR
//! reviewer thinks in files, so [`FileGraph::build`] folds every endpoint
//! onto its owning `File` node (memoized, one pass over the relationships)
//! and keeps two adjacency sets: forward imports and reverse "depends on"
//! (imports ∪ calls). Self-edges vanish in the fold.
//!
//! On top of that sit the questions `pr-recap` asks of a range:
//!
//! - [`FileGraph::import_sccs`] — which files import each other in a cycle;
//! - [`FileGraph::dependents`] / [`FileGraph::transitive_dependents`] — who
//!   is exposed when a file changes;
//! - [`new_import_cycles`] — which head cycles the base did not already have,
//!   with the edges the change introduced and the one to cut.
//!
//! Everything is iterative (petgraph's Kosaraju and hand-rolled BFS), so a
//! long import chain cannot overflow the stack.

use std::collections::{BTreeSet, HashMap, VecDeque};

use petgraph::algo::kosaraju_scc;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::functors::probes::mdg::coupling::owning_file;
use crate::graphs::mdg::object::ModuleDependencyGraph;

/// `IMPORTS` edges GitNexus draws for links between Markdown documents.
/// They are not code dependencies, so they never form a cycle or a
/// dependent.
const MARKDOWN_LINK: &str = "markdown-link";

/// One file per node, with import and dependency adjacency by path index.
///
/// Paths are sorted before they are indexed, so every derived list comes
/// out in a stable order.
#[derive(Debug, Clone, Default)]
pub struct FileGraph {
    paths: Vec<String>,
    index: HashMap<String, usize>,
    imports: Vec<BTreeSet<usize>>,
    /// Reverse `IMPORTS ∪ CALLS`: `depended_on_by[f]` are the files that
    /// import `f` or call into it.
    depended_on_by: Vec<BTreeSet<usize>>,
}

/// A head import cycle the base did not have in this shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleDelta {
    /// Head paths of every file in the cycle, sorted.
    pub members: Vec<String>,
    /// Head `IMPORTS` edges inside the cycle that the base lacked, sorted.
    pub introduced_edges: Vec<(String, String)>,
    /// The introduced edge to break: the first one whose source is a changed
    /// file, else the first introduced edge.
    pub cut: Option<(String, String)>,
    /// `cut.source → cut.target → … → cut.source`, a shortest way back
    /// through the cycle. Empty when there is no cut.
    pub witness: Vec<String>,
}

fn normalized(path: &str) -> &str {
    path.strip_prefix("./").unwrap_or(path)
}

impl FileGraph {
    /// Folds `graph`'s `IMPORTS` and `CALLS` onto its `File` nodes.
    ///
    /// Endpoints that belong to no file are dropped, as are self-edges and
    /// Markdown-link imports.
    pub fn build(graph: &ModuleDependencyGraph) -> FileGraph {
        let mut path_of_id: HashMap<&str, &str> = HashMap::new();
        for node in graph.nodes.values().filter(|node| node.label == "File") {
            if let Some(path) = node.properties.get("filePath").and_then(|v| v.as_str()) {
                path_of_id.insert(node.id.as_str(), normalized(path));
            }
        }
        let mut paths: Vec<String> = path_of_id.values().map(|p| p.to_string()).collect();
        paths.sort();
        paths.dedup();
        let index: HashMap<String, usize> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| (path.clone(), i))
            .collect();

        let resolve = |node_id: &str| -> Option<usize> {
            let file = owning_file(graph, node_id)?;
            let path = path_of_id.get(file.as_str())?;
            index.get(*path).copied()
        };
        let mut owner: HashMap<&str, Option<usize>> = HashMap::new();
        let mut imports = vec![BTreeSet::new(); paths.len()];
        let mut depended_on_by = vec![BTreeSet::new(); paths.len()];
        for rel in graph.relationships.values() {
            let is_import = match rel.rel_type.as_str() {
                "IMPORTS" if rel.reason == MARKDOWN_LINK => continue,
                "IMPORTS" => true,
                "CALLS" => false,
                _ => continue,
            };
            let from = *owner
                .entry(rel.source_id.as_str())
                .or_insert_with(|| resolve(&rel.source_id));
            let to = *owner
                .entry(rel.target_id.as_str())
                .or_insert_with(|| resolve(&rel.target_id));
            let (Some(from), Some(to)) = (from, to) else {
                continue;
            };
            if from == to {
                continue;
            }
            if is_import {
                imports[from].insert(to);
            }
            depended_on_by[to].insert(from);
        }

        FileGraph {
            paths,
            index,
            imports,
            depended_on_by,
        }
    }

    /// Whether the graph has a file at `path`.
    pub fn contains(&self, path: &str) -> bool {
        self.index.contains_key(normalized(path))
    }

    /// Whether `from` imports `to` directly.
    pub fn has_import(&self, from: &str, to: &str) -> bool {
        match (self.index_of(from), self.index_of(to)) {
            (Some(from), Some(to)) => self.imports[from].contains(&to),
            _ => false,
        }
    }

    /// Import cycles: strongly connected components over `IMPORTS` with two
    /// or more files. Members are sorted, and so are the components.
    pub fn import_sccs(&self) -> Vec<Vec<String>> {
        let mut components: Vec<Vec<String>> = self
            .import_components()
            .into_iter()
            .map(|component| {
                component
                    .into_iter()
                    .map(|i| self.paths[i].clone())
                    .collect()
            })
            .collect();
        components.sort();
        components
    }

    /// Files that import `path` or call into it, sorted.
    pub fn dependents(&self, path: &str) -> Vec<String> {
        self.index_of(path)
            .map(|i| {
                self.depended_on_by[i]
                    .iter()
                    .map(|&d| self.paths[d].clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every file that reaches any of `paths` over `IMPORTS ∪ CALLS`, the
    /// sources themselves excluded. One breadth-first walk from all sources.
    pub fn transitive_dependents<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> BTreeSet<String> {
        let sources: BTreeSet<usize> = paths.into_iter().filter_map(|p| self.index_of(p)).collect();
        let mut seen = sources.clone();
        let mut queue: VecDeque<usize> = sources.iter().copied().collect();
        let mut reached = BTreeSet::new();
        while let Some(file) = queue.pop_front() {
            for &dependent in &self.depended_on_by[file] {
                if seen.insert(dependent) {
                    reached.insert(self.paths[dependent].clone());
                    queue.push_back(dependent);
                }
            }
        }
        reached
    }

    fn index_of(&self, path: &str) -> Option<usize> {
        self.index.get(normalized(path)).copied()
    }

    /// Import SCCs of two or more files, as sorted path indexes.
    fn import_components(&self) -> Vec<Vec<usize>> {
        let mut graph: DiGraph<(), ()> = DiGraph::with_capacity(self.paths.len(), 0);
        for _ in &self.paths {
            graph.add_node(());
        }
        for (from, targets) in self.imports.iter().enumerate() {
            for &to in targets {
                graph.add_edge(NodeIndex::new(from), NodeIndex::new(to), ());
            }
        }
        kosaraju_scc(&graph)
            .into_iter()
            .filter(|component| component.len() >= 2)
            .map(|component| {
                let mut members: Vec<usize> = component.iter().map(|n| n.index()).collect();
                members.sort_unstable();
                members
            })
            .collect()
    }

    /// Shortest import path `from → … → to` that stays inside `within`.
    fn import_path(&self, from: usize, to: usize, within: &BTreeSet<usize>) -> Option<Vec<usize>> {
        let mut parent: HashMap<usize, usize> = HashMap::new();
        let mut queue = VecDeque::from([from]);
        parent.insert(from, from);
        while let Some(file) = queue.pop_front() {
            if file == to {
                let mut path = vec![to];
                let mut step = to;
                while step != from {
                    step = parent[&step];
                    path.push(step);
                }
                path.reverse();
                return Some(path);
            }
            for &next in &self.imports[file] {
                if within.contains(&next) && !parent.contains_key(&next) {
                    parent.insert(next, file);
                    queue.push_back(next);
                }
            }
        }
        None
    }
}

/// Head import cycles the base did not already have.
///
/// A head SCC is reported when its members, translated to base paths
/// (`head_to_base` maps a renamed file to its old path; any other path is
/// its own), do not all sit inside one base SCC. That covers a brand-new
/// cycle and a cycle that grew; a rename inside an existing cycle is not
/// news. `changed` holds head paths and steers which introduced edge is
/// named as the cut.
pub fn new_import_cycles(
    base: &FileGraph,
    head: &FileGraph,
    head_to_base: &HashMap<String, String>,
    changed: &BTreeSet<String>,
) -> Vec<CycleDelta> {
    let mut base_component: HashMap<&str, usize> = HashMap::new();
    for (id, component) in base.import_components().iter().enumerate() {
        for &file in component {
            base_component.insert(base.paths[file].as_str(), id);
        }
    }
    let to_base = |path: &str| -> String {
        head_to_base
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_string())
    };

    let mut deltas = Vec::new();
    for component in head.import_components() {
        let translated: Vec<String> = component.iter().map(|&f| to_base(&head.paths[f])).collect();
        let first = base_component.get(translated[0].as_str());
        let pre_existing = first.is_some()
            && translated
                .iter()
                .all(|path| base_component.get(path.as_str()) == first);
        if pre_existing {
            continue;
        }

        let within: BTreeSet<usize> = component.iter().copied().collect();
        let mut introduced: Vec<(usize, usize)> = Vec::new();
        for &from in &component {
            for &to in head.imports[from].intersection(&within) {
                if !base.has_import(&to_base(&head.paths[from]), &to_base(&head.paths[to])) {
                    introduced.push((from, to));
                }
            }
        }
        let name = |(from, to): (usize, usize)| (head.paths[from].clone(), head.paths[to].clone());
        let mut introduced_edges: Vec<(String, String)> =
            introduced.into_iter().map(name).collect();
        introduced_edges.sort();

        let cut = introduced_edges
            .iter()
            .find(|(from, _)| changed.contains(from))
            .or_else(|| introduced_edges.first())
            .cloned();
        let witness = cut
            .as_ref()
            .and_then(|(from, to)| {
                let (from, to) = (head.index_of(from)?, head.index_of(to)?);
                let back = head.import_path(to, from, &within)?;
                let mut walk = vec![head.paths[from].clone()];
                walk.extend(back.into_iter().map(|f| head.paths[f].clone()));
                Some(walk)
            })
            .unwrap_or_default();

        deltas.push(CycleDelta {
            members: component.iter().map(|&f| head.paths[f].clone()).collect(),
            introduced_edges,
            cut,
            witness,
        });
    }
    deltas.sort_by(|a, b| a.members.cmp(&b.members));
    deltas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::mdg::models::{GraphNode, GraphRelationship};
    use serde_json::Value;

    /// A tiny MDG builder: files, symbols inside them, and edges between
    /// either.
    struct Mdg(ModuleDependencyGraph);

    impl Mdg {
        fn new() -> Mdg {
            Mdg(ModuleDependencyGraph::new("x"))
        }

        /// A file node; its id drops a leading `./` so edges can name it
        /// either way, while `filePath` keeps the raw spelling.
        fn file(mut self, path: &str) -> Mdg {
            self.0.add_node(GraphNode {
                id: format!("File:{}", path.trim_start_matches("./")),
                label: "File".to_string(),
                properties: HashMap::from([(
                    "filePath".to_string(),
                    Value::String(path.to_string()),
                )]),
            });
            self
        }

        /// A function `name` defined in `path`.
        fn function(mut self, path: &str, name: &str) -> Mdg {
            let id = format!("Function:{path}:{name}");
            self.0.add_node(GraphNode {
                id: id.clone(),
                label: "Function".to_string(),
                properties: HashMap::new(),
            });
            self.edge(&format!("File:{path}"), &id, "DEFINES", "")
        }

        fn edge(mut self, source: &str, target: &str, rel_type: &str, reason: &str) -> Mdg {
            self.0.add_relationship(GraphRelationship {
                id: format!("{source}-{rel_type}->{target}"),
                source_id: source.to_string(),
                target_id: target.to_string(),
                rel_type: rel_type.to_string(),
                confidence: 1.0,
                reason: reason.to_string(),
                properties: HashMap::new(),
            });
            self
        }

        /// `from` imports `to`, both files.
        fn imports(self, from: &str, to: &str) -> Mdg {
            self.edge(
                &format!("File:{from}"),
                &format!("File:{to}"),
                "IMPORTS",
                "",
            )
        }

        fn graph(&self) -> FileGraph {
            FileGraph::build(&self.0)
        }
    }

    fn set(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn pair(from: &str, to: &str) -> (String, String) {
        (from.to_string(), to.to_string())
    }

    #[test]
    fn a_new_two_file_cycle_is_reported_with_its_cut_and_witness() {
        let base = Mdg::new()
            .file("a.rs")
            .file("b.rs")
            .imports("a.rs", "b.rs")
            .graph();
        let head = Mdg::new()
            .file("a.rs")
            .file("b.rs")
            .imports("a.rs", "b.rs")
            .imports("b.rs", "a.rs")
            .graph();
        let deltas = new_import_cycles(&base, &head, &HashMap::new(), &set(&["b.rs"]));
        assert_eq!(
            deltas,
            vec![CycleDelta {
                members: vec!["a.rs".to_string(), "b.rs".to_string()],
                introduced_edges: vec![pair("b.rs", "a.rs")],
                cut: Some(pair("b.rs", "a.rs")),
                witness: vec!["b.rs".to_string(), "a.rs".to_string(), "b.rs".to_string()],
            }]
        );
    }

    #[test]
    fn a_cycle_the_base_already_had_is_not_reported() {
        let both = || {
            Mdg::new()
                .file("a.rs")
                .file("b.rs")
                .imports("a.rs", "b.rs")
                .imports("b.rs", "a.rs")
        };
        let deltas = new_import_cycles(
            &both().graph(),
            &both().graph(),
            &HashMap::new(),
            &set(&["a.rs"]),
        );
        assert!(deltas.is_empty());
    }

    #[test]
    fn a_rename_inside_an_existing_cycle_is_not_reported() {
        let base = Mdg::new()
            .file("a.rs")
            .file("b.rs")
            .imports("a.rs", "b.rs")
            .imports("b.rs", "a.rs")
            .graph();
        let head = Mdg::new()
            .file("a.rs")
            .file("c.rs")
            .imports("a.rs", "c.rs")
            .imports("c.rs", "a.rs")
            .graph();
        let renames = HashMap::from([("c.rs".to_string(), "b.rs".to_string())]);
        assert!(new_import_cycles(&base, &head, &renames, &set(&["a.rs", "c.rs"])).is_empty());
        // Without the rename the same head is a new cycle.
        assert_eq!(
            new_import_cycles(&base, &head, &HashMap::new(), &set(&[])).len(),
            1
        );
    }

    #[test]
    fn a_cycle_that_grew_is_reported() {
        let base = Mdg::new()
            .file("a.rs")
            .file("b.rs")
            .file("c.rs")
            .imports("a.rs", "b.rs")
            .imports("b.rs", "a.rs")
            .imports("b.rs", "c.rs")
            .graph();
        let head = Mdg::new()
            .file("a.rs")
            .file("b.rs")
            .file("c.rs")
            .imports("a.rs", "b.rs")
            .imports("b.rs", "a.rs")
            .imports("b.rs", "c.rs")
            .imports("c.rs", "a.rs")
            .graph();
        let deltas = new_import_cycles(&base, &head, &HashMap::new(), &set(&["c.rs"]));
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].members, vec!["a.rs", "b.rs", "c.rs"]);
        assert_eq!(deltas[0].introduced_edges, vec![pair("c.rs", "a.rs")]);
        assert_eq!(deltas[0].witness, vec!["c.rs", "a.rs", "b.rs", "c.rs"]);
    }

    #[test]
    fn markdown_link_imports_are_ignored() {
        let head = Mdg::new()
            .file("README.md")
            .file("docs/guide.md")
            .edge(
                "File:README.md",
                "File:docs/guide.md",
                "IMPORTS",
                "markdown-link",
            )
            .edge(
                "File:docs/guide.md",
                "File:README.md",
                "IMPORTS",
                "markdown-link",
            )
            .graph();
        assert!(head.import_sccs().is_empty());
        assert!(head.dependents("README.md").is_empty());
    }

    #[test]
    fn the_cut_prefers_an_edge_out_of_a_changed_file() {
        let base = Mdg::new().file("a.py").file("b.py").file("c.py").graph();
        let head = Mdg::new()
            .file("a.py")
            .file("b.py")
            .file("c.py")
            .imports("a.py", "b.py")
            .imports("b.py", "c.py")
            .imports("c.py", "a.py")
            .graph();
        let cut = |changed: &[&str]| {
            new_import_cycles(&base, &head, &HashMap::new(), &set(changed))[0]
                .cut
                .clone()
        };
        assert_eq!(cut(&["c.py"]), Some(pair("c.py", "a.py")));
        // Two changed sources: the lexicographically first edge wins.
        assert_eq!(cut(&["c.py", "b.py"]), Some(pair("b.py", "c.py")));
        // No changed source: the first introduced edge.
        assert_eq!(cut(&[]), Some(pair("a.py", "b.py")));
    }

    #[test]
    fn dependents_follow_imports_and_calls_folded_to_files() {
        let head = Mdg::new()
            .file("./core.rs")
            .file("api.rs")
            .file("cli.rs")
            .file("main.rs")
            .function("core.rs", "run")
            .function("cli.rs", "start")
            .imports("api.rs", "core.rs")
            .edge("Function:cli.rs:start", "Function:core.rs:run", "CALLS", "")
            .imports("main.rs", "cli.rs")
            // A call inside one file is not a dependency.
            .edge("Function:core.rs:run", "File:core.rs", "CALLS", "")
            .graph();
        assert!(head.contains("core.rs"));
        assert_eq!(head.dependents("core.rs"), vec!["api.rs", "cli.rs"]);
        assert_eq!(
            head.transitive_dependents(["core.rs"]),
            set(&["api.rs", "cli.rs", "main.rs"])
        );
        // Sources are excluded even when they reach one another.
        assert_eq!(
            head.transitive_dependents(["core.rs", "cli.rs"]),
            set(&["api.rs", "main.rs"])
        );
        // Calls are dependencies, not imports: no cycle through them.
        assert!(head.import_sccs().is_empty());
    }

    #[test]
    fn a_long_chain_with_one_back_edge_is_one_cycle() {
        const FILES: usize = 20_000;
        let name = |i: usize| format!("m{i:05}.py");
        let mut mdg = Mdg::new();
        for i in 0..FILES {
            mdg = mdg.file(&name(i));
        }
        for i in 1..FILES {
            mdg = mdg.imports(&name(i - 1), &name(i));
        }
        let base = mdg.graph();
        let head = mdg.imports(&name(FILES - 1), &name(0)).graph();

        assert!(base.import_sccs().is_empty());
        let sccs = head.import_sccs();
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].len(), FILES);
        let deltas = new_import_cycles(&base, &head, &HashMap::new(), &set(&[]));
        assert_eq!(deltas[0].cut, Some(pair(&name(FILES - 1), &name(0))));
        assert_eq!(deltas[0].witness.len(), FILES + 1);
        assert_eq!(
            head.transitive_dependents([name(5).as_str()]).len(),
            FILES - 1
        );
    }
}
