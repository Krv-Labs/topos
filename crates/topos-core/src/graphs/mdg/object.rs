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
//! # Deviation from the Python original: the LadybugDB binary format
//!
//! Python supports two `.gitnexus/` store formats: the legacy JSON
//! directory (ported here, [`ModuleDependencyGraph::from_json_dir`]) and
//! the current LadybugDB binary format (`lbug` file, GitNexus ≥ 1.5),
//! read via `import ladybug` — a Python-only binding to a native graph
//! database with no Rust client available. That's a genuine external
//! dependency wall, not a port left undone: reading it from Rust would
//! mean either an FFI binding to `ladybug`'s native library or
//! reimplementing a LadybugDB reader from scratch, neither of which
//! belongs in this representation module. [`MdgError::LadybugBinaryUnsupported`]
//! surfaces this clearly rather than silently returning an empty graph.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::models::{parse_node, parse_relationship, GraphNode, GraphRelationship};
use crate::graphs::base::Representation;

#[derive(Debug)]
pub enum MdgError {
    /// No `.gitnexus/lbug` store found at the expected path.
    NotFound(PathBuf),
    /// The store is the LadybugDB binary format — see the module doc.
    LadybugBinaryUnsupported(PathBuf),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for MdgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MdgError::NotFound(path) => write!(
                f,
                "LadybugDB store not found at {}. Install GitNexus (npm install -g gitnexus) \
                 and run 'gitnexus analyze' in the repository root first.",
                path.display()
            ),
            MdgError::LadybugBinaryUnsupported(path) => write!(
                f,
                "{} is the LadybugDB binary format, which topos-core cannot read \
                 (no Rust client for LadybugDB exists yet — see this module's doc comment)",
                path.display()
            ),
            MdgError::Io(e) => write!(f, "{e}"),
            MdgError::Json(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for MdgError {}

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
    pub nodes: HashMap<String, GraphNode>,
    pub relationships: HashMap<String, GraphRelationship>,
    /// Relationship ids leaving each node — stores ids rather than
    /// cloned `GraphRelationship`s (Python shares object refs; this
    /// avoids the Rust equivalent of duplicating every relationship
    /// into two extra indices).
    outgoing: HashMap<String, Vec<String>>,
    incoming: HashMap<String, Vec<String>>,
}

impl ModuleDependencyGraph {
    pub fn new(target_file: impl Into<String>) -> Self {
        ModuleDependencyGraph {
            target_file: target_file.into(),
            nodes: HashMap::new(),
            relationships: HashMap::new(),
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
        }
    }

    // --- Construction ------------------------------------------------

    /// Build a `ModuleDependencyGraph` from a `.gitnexus/` directory.
    pub fn from_gitnexus_dir(
        gitnexus_dir: impl AsRef<Path>,
        target_file: impl Into<String>,
    ) -> Result<Self, MdgError> {
        let lbug_path = gitnexus_dir.as_ref().join("lbug");
        if lbug_path.is_file() {
            return Err(MdgError::LadybugBinaryUnsupported(lbug_path));
        }
        if lbug_path.is_dir() {
            return Self::from_json_dir(&lbug_path, target_file);
        }
        Err(MdgError::NotFound(lbug_path))
    }

    /// Load from the legacy JSON directory format produced by GitNexus < 1.5.
    pub fn from_json_dir(
        lbug_dir: &Path,
        target_file: impl Into<String>,
    ) -> Result<Self, MdgError> {
        let mut graph = ModuleDependencyGraph::new(target_file);
        for entry in std::fs::read_dir(lbug_dir).map_err(MdgError::Io)? {
            let path = entry.map_err(MdgError::Io)?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(MdgError::Io)?;
            let data: Value = serde_json::from_str(&text).map_err(MdgError::Json)?;
            graph.ingest_json_document(&data);
        }
        Ok(graph)
    }

    fn ingest_json_document(&mut self, data: &Value) {
        match data {
            Value::Array(items) => {
                for item in items {
                    if item.get("label").is_some() && item.get("id").is_some() {
                        if let Some(node) = parse_node(item) {
                            self.add_node(node);
                        }
                    } else if item.get("type").is_some() && item.get("sourceId").is_some() {
                        if let Some(rel) = parse_relationship(item) {
                            self.add_relationship(rel);
                        }
                    }
                }
            }
            Value::Object(_) => {
                for node in data
                    .get("nodes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(node) = parse_node(node) {
                        self.add_node(node);
                    }
                }
                for rel in data
                    .get("relationships")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(rel) = parse_relationship(rel) {
                        self.add_relationship(rel);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn add_node(&mut self, node: GraphNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn add_relationship(&mut self, rel: GraphRelationship) {
        self.outgoing
            .entry(rel.source_id.clone())
            .or_default()
            .push(rel.id.clone());
        self.incoming
            .entry(rel.target_id.clone())
            .or_default()
            .push(rel.id.clone());
        self.relationships.insert(rel.id.clone(), rel);
    }

    // --- Lookups -------------------------------------------------------

    pub fn get_node(&self, node_id: &str) -> Option<&GraphNode> {
        self.nodes.get(node_id)
    }

    pub fn nodes_of_label(&self, label: &str) -> Vec<&GraphNode> {
        self.nodes.values().filter(|n| n.label == label).collect()
    }

    pub fn relationships_of_type(&self, rel_type: &str) -> Vec<&GraphRelationship> {
        self.relationships
            .values()
            .filter(|r| r.rel_type == rel_type)
            .collect()
    }

    pub fn outgoing(&self, node_id: &str, rel_type: Option<&str>) -> Vec<&GraphRelationship> {
        self.outgoing
            .get(node_id)
            .into_iter()
            .flatten()
            .filter_map(|id| self.relationships.get(id))
            .filter(|r| rel_type.is_none_or(|t| r.rel_type == t))
            .collect()
    }

    pub fn incoming(&self, node_id: &str, rel_type: Option<&str>) -> Vec<&GraphRelationship> {
        self.incoming
            .get(node_id)
            .into_iter()
            .flatten()
            .filter_map(|id| self.relationships.get(id))
            .filter(|r| rel_type.is_none_or(|t| r.rel_type == t))
            .collect()
    }

    /// Find the File node ID matching `target_file`.
    pub fn file_node_id(&self) -> Option<&str> {
        self.nodes.values().find_map(|node| {
            if node.label != "File" {
                return None;
            }
            let file_path = node.properties.get("filePath")?.as_str()?;
            let matches = file_path == self.target_file
                || file_path.ends_with(&format!("/{}", self.target_file))
                || self.target_file.ends_with(&format!("/{file_path}"));
            matches.then_some(node.id.as_str())
        })
    }

    /// IDs of all symbols directly contained in a file node.
    pub fn contained_symbols(&self, file_node_id: &str) -> Vec<String> {
        self.outgoing(file_node_id, Some("CONTAINS"))
            .into_iter()
            .map(|r| r.target_id.clone())
            .collect()
    }

    /// IDs of all symbols transitively reachable via CONTAINS edges.
    ///
    /// Performs a BFS down the CONTAINS tree starting from `node_id`.
    /// Cycles are handled safely via a visited set.
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
        unimplemented!(
            "ModuleDependencyGraph::metrics depends on functors::probes::mdg::{{coupling, fan}} (issue #145)"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn from_gitnexus_dir_reports_ladybug_binary_as_unsupported() {
        let dir = std::env::temp_dir().join(format!("topos_mdg_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lbug"), b"binary-store-placeholder").unwrap();

        let result = ModuleDependencyGraph::from_gitnexus_dir(&dir, "x");
        assert!(matches!(result, Err(MdgError::LadybugBinaryUnsupported(_))));

        std::fs::remove_dir_all(&dir).ok();
    }
}
