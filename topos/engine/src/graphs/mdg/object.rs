//! Module Dependency Graph (MDG) representation.
//!
//! Consumes the knowledge graph produced by [GitNexus](https://github.com/abhigyanpatwari/GitNexus)
//! and lifts it into a [`Representation`]. This is the **inter-module**
//! view of the program — it captures the import/call/inheritance
//! structure across files, packages, and classes. Compare this with the
//! academic **intra-procedural Program Dependence Graph** at
//! [`crate::graphs::pdg`], which records control- and data-dependence
//! edges *within* a single procedure.
//!
//! GitNexus runs `gitnexus analyze` on a repository and writes a
//! `.gitnexus/` directory containing a LadybugDB graph store. This
//! module parses that output into an in-memory graph of typed nodes and
//! relationships, then computes dependency-level metrics that the AST
//! alone cannot provide.
//!
//! Metrics produced (feed the COMPOSABLE generator of `H(G_qual)`):
//! - `mdg.coupling` — afferent + efferent coupling for a file
//! - `mdg.instability` — `Ce / (Ca + Ce)` (Martin's metric)
//! - `mdg.fan_in` — incoming CALLS edges
//! - `mdg.fan_out` — outgoing CALLS edges
//! - `mdg.dep_depth` — longest IMPORTS chain from the file
//!
//! # LadybugDB binary stores (GitNexus ≥ 1.5)
//!
//! Two on-disk shapes exist under `.gitnexus/lbug`:
//! - **Directory of JSON** (GitNexus < 1.5) — [`ModuleDependencyGraph::from_json_dir`]
//! - **Packed LadybugDB binary file** (GitNexus ≥ 1.5) — loaded by
//!   [`ModuleDependencyGraph::from_ladybug_native`] via the embedded
//!   `lbug` crate (issue #198).
//!
//! [`ModuleDependencyGraph::from_lbug_path`] dispatches on the path shape.

use super::models::{GraphNode, GraphRelationship};
use crate::functors::probes::mdg::coupling::{
    calculate_coupling, calculate_dependency_depth, calculate_instability_from_result,
};
use crate::functors::probes::mdg::fan::calculate_fan_in_out;
use crate::graphs::base::Representation;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug)]
pub enum MdgError {
    /// No `.gitnexus/lbug` store found at the expected path.
    NotFound(PathBuf),
    /// Embedded `lbug` client failed to open/query the binary store.
    LadybugNativeFailed(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for MdgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MdgError::NotFound(path) => write!(
                f,
                "LadybugDB store not found at {}. Install GitNexus (npm install -g gitnexus@1.6.8) \
                 and run 'gitnexus analyze' in the repository root first.",
                path.display()
            ),
            MdgError::LadybugNativeFailed(msg) => {
                write!(f, "failed to load LadybugDB store via native lbug: {msg}")
            }
            MdgError::Io(e) => write!(f, "{e}"),
            MdgError::Json(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for MdgError {}

/// Repository-wide topology behind a [`ModuleDependencyGraph`].
///
/// One loaded store owns this once. Target-specific views share it through
/// an `Arc`; only the target path differs. Fields stay private so mutation
/// always goes through [`ModuleDependencyGraph`]'s copy-on-write methods.
#[derive(Debug, Clone, Default)]
pub struct MdgTopology {
    nodes: HashMap<String, GraphNode>,
    relationships: HashMap<String, GraphRelationship>,
    /// Relationship ids leaving each node — stores ids rather than
    /// cloned `GraphRelationship`s (Python shares object refs; this
    /// avoids the Rust equivalent of duplicating every relationship
    /// into two extra indices).
    outgoing: HashMap<String, Vec<String>>,
    incoming: HashMap<String, Vec<String>>,
}

/// Inter-module dependency-graph representation parsed from GitNexus
/// output.
///
/// Provides graph lookup methods and computes dependency-level metrics
/// for a target file path within the graph. This is the **module-level**
/// dependency view (imports, calls, inheritance across files) — distinct
/// from the academic intra-procedural [`crate::graphs::pdg::object::ProgramDependenceGraph`].
#[derive(Debug, Clone)]
pub struct ModuleDependencyGraph {
    pub target_file: String,
    topology: Arc<MdgTopology>,
}

/// Parent → child edges that mean "this symbol lives inside that one".
///
/// GitNexus does not express file membership with a single edge type:
/// `DEFINES` carries File → symbol, `HAS_METHOD` / `HAS_PROPERTY` carry
/// class-member nesting, and `CONTAINS` is Folder → File plus File →
/// Section. Walking `CONTAINS` alone — as this did — reached no symbol from
/// any File node, so every file measured `fan_in = fan_out = 0` and the
/// COMPOSABLE fan gates could never fire. `CONTAINS` stays in the set for
/// the legacy JSON-directory stores (GitNexus < 1.5), which used it for
/// file membership.
///
/// `MEMBER_OF` is deliberately absent: it points a symbol at its *community*
/// cluster, not its parent, so following it would pull in unrelated files'
/// symbols.
pub const CONTAINMENT_RELS: [&str; 4] = ["CONTAINS", "DEFINES", "HAS_METHOD", "HAS_PROPERTY"];

impl ModuleDependencyGraph {
    pub fn new(target_file: impl Into<String>) -> Self {
        ModuleDependencyGraph {
            target_file: target_file.into(),
            topology: Arc::new(MdgTopology::default()),
        }
    }

    /// Same loaded store, scored against a different file.
    ///
    /// File identity is only the lookup key in [`Self::file_node_id`]. The
    /// nodes and edges do not change, so a process that already holds this
    /// store can retarget it without opening Ladybug again. The view shares
    /// the loaded topology; it does not copy it. Mutation goes through
    /// copy-on-write, so editing a view cannot affect the store or another
    /// view.
    pub fn for_target(&self, target_file: impl Into<String>) -> Self {
        ModuleDependencyGraph {
            target_file: target_file.into(),
            topology: Arc::clone(&self.topology),
        }
    }

    pub fn add_node(&mut self, node: GraphNode) {
        Arc::make_mut(&mut self.topology)
            .nodes
            .insert(node.id.clone(), node);
    }

    pub fn add_relationship(&mut self, rel: GraphRelationship) {
        let topology = Arc::make_mut(&mut self.topology);
        topology
            .outgoing
            .entry(rel.source_id.clone())
            .or_default()
            .push(rel.id.clone());
        topology
            .incoming
            .entry(rel.target_id.clone())
            .or_default()
            .push(rel.id.clone());
        topology.relationships.insert(rel.id.clone(), rel);
    }

    // --- Lookups -------------------------------------------------------

    /// The repository topology shared by this view.
    pub fn topology(&self) -> &Arc<MdgTopology> {
        &self.topology
    }

    pub fn nodes(&self) -> &HashMap<String, GraphNode> {
        &self.topology.nodes
    }

    pub fn relationships(&self) -> &HashMap<String, GraphRelationship> {
        &self.topology.relationships
    }

    pub fn get_node(&self, node_id: &str) -> Option<&GraphNode> {
        self.topology.nodes.get(node_id)
    }

    pub fn nodes_of_label(&self, label: &str) -> Vec<&GraphNode> {
        self.topology
            .nodes
            .values()
            .filter(|n| n.label == label)
            .collect()
    }

    pub fn relationships_of_type(&self, rel_type: &str) -> Vec<&GraphRelationship> {
        self.topology
            .relationships
            .values()
            .filter(|r| r.rel_type == rel_type)
            .collect()
    }

    pub fn outgoing(&self, node_id: &str, rel_type: Option<&str>) -> Vec<&GraphRelationship> {
        self.topology
            .outgoing
            .get(node_id)
            .into_iter()
            .flatten()
            .filter_map(|id| self.topology.relationships.get(id))
            .filter(|r| rel_type.is_none_or(|t| r.rel_type == t))
            .collect()
    }

    pub fn incoming(&self, node_id: &str, rel_type: Option<&str>) -> Vec<&GraphRelationship> {
        self.topology
            .incoming
            .get(node_id)
            .into_iter()
            .flatten()
            .filter_map(|id| self.topology.relationships.get(id))
            .filter(|r| rel_type.is_none_or(|t| r.rel_type == t))
            .collect()
    }

    /// Find the File node ID matching `target_file`.
    pub fn file_node_id(&self) -> Option<&str> {
        self.file_node_id_for(&self.target_file)
    }

    /// Find the File node ID matching an arbitrary repo-relative `path`.
    ///
    /// Same matching rules as [`Self::file_node_id`] (exact, or either side
    /// a path-segment suffix of the other), but for a caller-supplied path
    /// rather than `self.target_file`. One graph holds the whole repo, so
    /// consumers that reason about several files at once (e.g.
    /// [`crate::graphs::mdg::split`]) need the lookup without rebuilding a
    /// graph per target.
    pub fn file_node_id_for(&self, path: &str) -> Option<&str> {
        self.topology.nodes.values().find_map(|node| {
            if node.label != "File" {
                return None;
            }
            let file_path = node.properties.get("filePath")?.as_str()?;
            let matches = file_path == path
                || file_path.ends_with(&format!("/{path}"))
                || path.ends_with(&format!("/{file_path}"));
            matches.then_some(node.id.as_str())
        })
    }

    /// IDs of all symbols directly contained in a file node.
    pub fn contained_symbols(&self, file_node_id: &str) -> Vec<String> {
        CONTAINMENT_RELS
            .iter()
            .flat_map(|rel_type| self.outgoing(file_node_id, Some(rel_type)))
            .map(|r| r.target_id.clone())
            .collect()
    }

    /// IDs of all symbols transitively reachable via containment edges.
    ///
    /// Performs a BFS down the [`CONTAINMENT_RELS`] tree starting from
    /// `node_id`. Cycles are handled safely via a visited set.
    pub fn all_contained_symbols(&self, node_id: &str) -> Vec<String> {
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut result = Vec::new();
        let mut frontier: VecDeque<String> = self.contained_symbols(node_id).into();
        while let Some(child) = frontier.pop_front() {
            if !visited.insert(child.clone()) {
                continue;
            }
            frontier.extend(self.contained_symbols(&child));
            result.push(child);
        }
        result
    }
}

impl Representation for ModuleDependencyGraph {
    fn name(&self) -> &str {
        "mdg"
    }

    /// Feeds the COMPOSABLE generator of `H(G_qual)`.
    fn dimension(&self) -> &str {
        "composable"
    }

    fn metrics(&self) -> HashMap<String, f64> {
        let Some(file_node_id) = self.file_node_id().map(str::to_string) else {
            // No matching File node in the graph — matches the Python
            // original's default (zero coupling, neutral 0.5
            // instability) rather than omitting the keys entirely.
            return HashMap::from([
                ("mdg.coupling".to_string(), 0.0),
                ("mdg.instability".to_string(), 0.5),
                ("mdg.fan_in".to_string(), 0.0),
                ("mdg.fan_out".to_string(), 0.0),
                ("mdg.dep_depth".to_string(), 0.0),
            ]);
        };

        let mut symbol_ids: std::collections::HashSet<String> = self
            .all_contained_symbols(&file_node_id)
            .into_iter()
            .collect();
        symbol_ids.insert(file_node_id.clone());

        let coupling = calculate_coupling(self, &file_node_id, Some(&symbol_ids));
        let instability = calculate_instability_from_result(&coupling);
        let fan = calculate_fan_in_out(self, &file_node_id, Some(&symbol_ids));
        let dep_depth = calculate_dependency_depth(self, &file_node_id);

        HashMap::from([
            ("mdg.coupling".to_string(), coupling.total() as f64),
            ("mdg.instability".to_string(), instability),
            ("mdg.fan_in".to_string(), fan.fan_in as f64),
            ("mdg.fan_out".to_string(), fan.fan_out as f64),
            ("mdg.dep_depth".to_string(), dep_depth as f64),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn node(id: &str, label: &str, file_path: Option<&str>) -> GraphNode {
        let mut properties = HashMap::new();
        if let Some(path) = file_path {
            properties.insert("filePath".to_string(), Value::String(path.to_string()));
        }
        GraphNode {
            id: id.to_string(),
            label: label.to_string(),
            properties,
        }
    }

    fn rel(id: &str, source: &str, target: &str, rel_type: &str) -> GraphRelationship {
        GraphRelationship {
            id: id.to_string(),
            source_id: source.to_string(),
            target_id: target.to_string(),
            rel_type: rel_type.to_string(),
            confidence: 1.0,
            reason: String::new(),
            properties: Default::default(),
        }
    }

    #[test]
    fn file_node_id_matches_by_suffix() {
        let mut graph = ModuleDependencyGraph::new("src/lib.rs");
        graph.add_node(node("f1", "File", Some("/repo/src/lib.rs")));
        assert_eq!(graph.file_node_id(), Some("f1"));
    }

    #[test]
    fn all_contained_symbols_bfs_handles_cycles() {
        let mut graph = ModuleDependencyGraph::new("x");
        graph.add_relationship(rel("r1", "a", "b", "CONTAINS"));
        graph.add_relationship(rel("r2", "b", "c", "CONTAINS"));
        graph.add_relationship(rel("r3", "c", "a", "CONTAINS")); // cycle back to a
        let mut symbols = graph.all_contained_symbols("a");
        symbols.sort();
        // The starting node itself is never marked `visited` up front (it's
        // never in its own initial frontier), so a cycle back to it — as
        // here — legitimately makes it appear once in the result. This
        // matches the Python original's `visited` set exactly, which has
        // the same property; it's not a bug in either port, just a quirk
        // of "visited tracks *popped* nodes, not the start."
        assert_eq!(
            symbols,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn metrics_reports_coupling_instability_fan_and_depth() {
        let mut g = ModuleDependencyGraph::new("a.py");
        g.add_node(node("File:a.py", "File", Some("a.py")));
        g.add_node(node("File:b.py", "File", Some("b.py")));
        g.add_node(node("File:c.py", "File", Some("c.py")));
        // Two edges, not one: a single edge leaves `I` unresolvable (see
        // `probes::mdg::coupling::MIN_RESOLVABLE_COUPLING`), so "pure
        // efferent" only becomes a readable verdict from two imports up.
        g.add_relationship(rel("i1", "File:a.py", "File:b.py", "IMPORTS"));
        g.add_relationship(rel("i2", "File:a.py", "File:c.py", "IMPORTS"));

        let metrics = g.metrics();
        assert_eq!(metrics["mdg.coupling"], 2.0);
        assert_eq!(metrics["mdg.instability"], 1.0); // pure efferent
        assert_eq!(metrics["mdg.fan_in"], 0.0);
        assert_eq!(metrics["mdg.fan_out"], 0.0);
        assert_eq!(metrics["mdg.dep_depth"], 1.0);
    }

    /// The shape GitNexus actually emits: a file owns its symbols over
    /// `DEFINES`, class members hang off `HAS_METHOD`, and the cross-file
    /// `CALLS` edges that fan-in/fan-out count leave the *symbols*, not the
    /// File node. Walking `CONTAINS` alone reached none of them, so every
    /// file measured `fan_in = fan_out = 0`.
    #[test]
    fn fan_counts_symbols_reached_over_defines_not_only_contains() {
        let mut g = ModuleDependencyGraph::new("a.rs");
        g.add_node(node("File:a.rs", "File", Some("a.rs")));
        g.add_node(node("Struct:a.rs:S", "Struct", None));
        g.add_node(node("Method:a.rs:S.run", "Method", None));
        g.add_relationship(rel("d1", "File:a.rs", "Struct:a.rs:S", "DEFINES"));
        g.add_relationship(rel(
            "m1",
            "Struct:a.rs:S",
            "Method:a.rs:S.run",
            "HAS_METHOD",
        ));
        // One external caller of the nested method, one external callee.
        g.add_relationship(rel(
            "c1",
            "Function:b.rs:caller",
            "Method:a.rs:S.run",
            "CALLS",
        ));
        g.add_relationship(rel(
            "c2",
            "Method:a.rs:S.run",
            "Function:c.rs:callee",
            "CALLS",
        ));

        let metrics = g.metrics();
        assert_eq!(metrics["mdg.fan_in"], 1.0);
        assert_eq!(metrics["mdg.fan_out"], 1.0);
    }

    #[test]
    fn metrics_defaults_when_target_file_has_no_matching_node() {
        let g = ModuleDependencyGraph::new("missing.py");
        let metrics = g.metrics();
        assert_eq!(metrics["mdg.coupling"], 0.0);
        assert_eq!(metrics["mdg.instability"], 0.5);
        assert_eq!(metrics["mdg.fan_in"], 0.0);
        assert_eq!(metrics["mdg.fan_out"], 0.0);
        assert_eq!(metrics["mdg.dep_depth"], 0.0);
    }

    #[test]
    fn target_views_share_one_topology_allocation() {
        let mut base = ModuleDependencyGraph::new("a.py");
        base.add_node(node("File:a.py", "File", Some("a.py")));
        base.add_relationship(rel("r1", "File:a.py", "File:b.py", "IMPORTS"));

        let first = base.for_target("b.py");
        let second = base.for_target("c.py");
        let cloned = base.clone();

        assert!(Arc::ptr_eq(base.topology(), first.topology()));
        assert!(Arc::ptr_eq(first.topology(), second.topology()));
        assert!(Arc::ptr_eq(base.topology(), cloned.topology()));
        assert_eq!(first.target_file, "b.py");
        assert_eq!(second.target_file, "c.py");
        assert_eq!(first.nodes().len(), 1);
        assert_eq!(second.relationships().len(), 1);
    }

    #[test]
    fn unique_graph_mutates_its_topology_in_place() {
        let mut graph = ModuleDependencyGraph::new("a.py");
        let allocation = Arc::as_ptr(graph.topology());

        graph.add_node(node("File:a.py", "File", Some("a.py")));
        graph.add_relationship(rel("r1", "File:a.py", "File:b.py", "IMPORTS"));

        assert_eq!(allocation, Arc::as_ptr(graph.topology()));
    }

    #[test]
    fn mutating_a_view_copies_topology_and_leaves_the_base_alone() {
        let mut base = ModuleDependencyGraph::new("a.py");
        base.add_node(node("File:a.py", "File", Some("a.py")));
        base.add_relationship(rel("r1", "File:a.py", "File:b.py", "IMPORTS"));

        let mut view = base.for_target("b.py");
        view.add_node(node("File:b.py", "File", Some("b.py")));
        view.add_relationship(rel("r2", "File:b.py", "File:a.py", "IMPORTS"));

        assert!(!Arc::ptr_eq(base.topology(), view.topology()));
        assert!(base.get_node("File:b.py").is_none());
        assert_eq!(base.relationships().len(), 1);
        assert!(view.get_node("File:b.py").is_some());
        assert_eq!(view.relationships().len(), 2);
    }

    #[test]
    fn mutating_the_base_after_a_view_exists_leaves_the_view_alone() {
        let mut base = ModuleDependencyGraph::new("a.py");
        base.add_node(node("File:a.py", "File", Some("a.py")));

        let view = base.for_target("b.py");
        base.add_node(node("File:b.py", "File", Some("b.py")));
        base.add_relationship(rel("r1", "File:a.py", "File:b.py", "IMPORTS"));

        assert!(!Arc::ptr_eq(base.topology(), view.topology()));
        assert!(view.get_node("File:b.py").is_none());
        assert!(view.relationships().is_empty());
        assert!(base.get_node("File:b.py").is_some());
        assert_eq!(base.relationships().len(), 1);
    }

    #[test]
    fn shared_target_views_score_safely_from_multiple_threads() {
        let mut base = ModuleDependencyGraph::new("");
        for path in ["a.py", "b.py", "c.py"] {
            base.add_node(node(&format!("File:{path}"), "File", Some(path)));
        }
        base.add_relationship(rel("i1", "File:a.py", "File:b.py", "IMPORTS"));
        base.add_relationship(rel("i2", "File:a.py", "File:c.py", "IMPORTS"));

        let views = Arc::new(
            ["a.py", "b.py", "c.py"]
                .into_iter()
                .map(|path| base.for_target(path))
                .collect::<Vec<_>>(),
        );

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let views = Arc::clone(&views);
                std::thread::spawn(move || {
                    (0..20)
                        .map(|_| {
                            views
                                .iter()
                                .map(|view| view.metrics()["mdg.coupling"])
                                .sum::<f64>()
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let results: Vec<Vec<f64>> = handles
            .into_iter()
            .map(|handle| handle.join().expect("worker panicked"))
            .collect();

        // a.py has two efferent edges; b.py and c.py each have one afferent.
        assert_eq!(results[0][0], 4.0);
        assert!(results.iter().all(|result| result == &results[0]));
    }
}

/// Retargeting + metrics benchmark, ignored by default.
///
/// Run with:
/// `cargo test --release -p topos-engine bench_mdg_retarget -- --ignored --nocapture`
///
/// `MDG_BENCH_FILES` (comma-separated file counts) and `MDG_BENCH_VIEWS`
/// (comma-separated target counts) narrow a run. Peak RSS is measured from
/// outside, e.g. `/usr/bin/time -l env MDG_BENCH_FILES=500 ... cargo test ...`.
#[cfg(test)]
mod bench {
    use super::*;
    use serde_json::Value;
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    const ORDINARY_FILES: usize = 500;
    const LARGE_FILES: usize = 5_000;

    fn target_path(index: usize) -> String {
        format!("src/f{index:05}.rs")
    }

    /// Four nodes and sixteen relationships per file: a File node, three
    /// symbols, three `DEFINES`, one `HAS_METHOD`, and twelve `IMPORTS`.
    fn bench_graph(files: usize) -> ModuleDependencyGraph {
        let mut graph = ModuleDependencyGraph::new("");
        for index in 0..files {
            let path = target_path(index);
            let file_id = format!("File:{path}");
            graph.add_node(GraphNode {
                id: file_id.clone(),
                label: "File".to_string(),
                properties: HashMap::from([("filePath".to_string(), Value::String(path))]),
            });
            for symbol in 0..3 {
                let symbol_id = format!("Function:{index}:{symbol}");
                graph.add_node(GraphNode {
                    id: symbol_id.clone(),
                    label: "Function".to_string(),
                    properties: HashMap::new(),
                });
                graph.add_relationship(GraphRelationship {
                    id: format!("{file_id}-DEFINES->{symbol_id}"),
                    source_id: file_id.clone(),
                    target_id: symbol_id,
                    rel_type: "DEFINES".to_string(),
                    confidence: 1.0,
                    reason: String::new(),
                    properties: HashMap::new(),
                });
            }
            graph.add_relationship(GraphRelationship {
                id: format!("{file_id}-HAS_METHOD->{index}:1"),
                source_id: format!("Function:{index}:0"),
                target_id: format!("Function:{index}:1"),
                rel_type: "HAS_METHOD".to_string(),
                confidence: 1.0,
                reason: String::new(),
                properties: HashMap::new(),
            });
            for step in 1..=12 {
                let target = (index + step * 7) % files;
                let target_id = format!("File:{}", target_path(target));
                graph.add_relationship(GraphRelationship {
                    id: format!("{file_id}-IMPORTS-{step}->{target_id}"),
                    source_id: file_id.clone(),
                    target_id,
                    rel_type: "IMPORTS".to_string(),
                    confidence: 1.0,
                    reason: String::new(),
                    properties: HashMap::new(),
                });
            }
        }
        graph
    }

    fn env_list(name: &str, default: &[usize]) -> Vec<usize> {
        match std::env::var(name) {
            Ok(raw) => raw
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect(),
            Err(_) => default.to_vec(),
        }
    }

    fn millis(duration: Duration) -> f64 {
        duration.as_secs_f64() * 1_000.0
    }

    fn retarget_and_score(files: usize, targets: usize) {
        let graph = bench_graph(files);
        let selected: Vec<usize> = (0..targets)
            .map(|view| view * (files / targets.max(1)))
            .collect();

        // Targeting only: build every view before timing the scoring pass.
        let started = Instant::now();
        let views: Vec<ModuleDependencyGraph> = selected
            .iter()
            .map(|&index| graph.for_target(target_path(index)))
            .collect();
        let targeting = started.elapsed();

        // Total scoring time with every view retained, so peak memory
        // reflects the targeting strategy rather than one transient view.
        let started = Instant::now();
        let mut checksum = 0.0;
        for view in &views {
            checksum += view.metrics().values().sum::<f64>();
        }
        let scoring = started.elapsed();
        black_box(checksum);
        black_box(&views);

        eprintln!(
            "mdg-bench files={files} nodes={} relationships={} views={targets} \
             targeting_ms={:.1} scoring_ms={:.1} total_ms={:.1}",
            graph.nodes().len(),
            graph.relationships().len(),
            millis(targeting),
            millis(scoring),
            millis(targeting + scoring),
        );
    }

    #[test]
    #[ignore = "performance measurement; run explicitly in release"]
    fn bench_mdg_retarget() {
        for files in env_list("MDG_BENCH_FILES", &[ORDINARY_FILES, LARGE_FILES]) {
            for targets in env_list("MDG_BENCH_VIEWS", &[1, 40, 100]) {
                retarget_and_score(files, targets);
            }
        }
    }
}
