//! Drops dependency edges GitNexus resolved by bare name and that the
//! source code proves false.
//!
//! GitNexus matches a call or an import to a repository symbol by its name.
//! `std::fs::write` becomes a call to some private `fn write` in another
//! crate, `import json` becomes an import of `pkg/http/json.py`, and
//! `import { x } from "@codemirror/state"` becomes an import of a repo file
//! named `state.ts`. Three rules remove those edges, and only when the
//! evidence rules out every other reading:
//!
//! - [`DropReason::RustPrivateCrossCrate`]: a `CALLS` edge between files of
//!   different crates (nearest `Cargo.toml`) whose target is a non-exported
//!   free function. A private item cannot be called from another crate.
//!   Methods are left alone: a trait impl method carries no `pub` and is
//!   still callable through the trait.
//! - [`DropReason::PythonStdlibImport`]: an `IMPORTS` edge whose target
//!   module is named only by absolute standard-library imports in the source
//!   file (`import json`, `from json import x`), when the repository has no
//!   top-level module of that name.
//! - [`DropReason::JsBarePackageImport`]: an `IMPORTS` edge whose target
//!   file is named only by bare package specifiers (`@codemirror/state`)
//!   that no `paths` alias, `baseUrl` entry, or workspace package covers.
//!
//! Import statements are recovered by parsing each source file's `content`
//! with tree-sitter, because GitNexus stores no specifier on the edge. When
//! a file has no content, its config cannot be read, or any other import in
//! the file could also name the target, the edge stays.

mod js;
mod jsonc;
mod paths;
mod python;
pub mod python_stdlib;
mod rust;
mod syntax;

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

use super::object::ModuleDependencyGraph;
use js::JsRule;
use paths::{extension, normalized};
use python::PythonRule;
use rust::RustRule;

const MARKDOWN_LINK: &str = "markdown-link";

/// Why an edge was judged false.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    RustPrivateCrossCrate,
    PythonStdlibImport,
    JsBarePackageImport,
}

/// Dropped edge counts per reason.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DroppedEdgeCounts {
    pub rust_private_cross_crate: usize,
    pub python_stdlib_import: usize,
    pub js_bare_package_import: usize,
}

impl DroppedEdgeCounts {
    pub fn total(&self) -> usize {
        self.rust_private_cross_crate + self.python_stdlib_import + self.js_bare_package_import
    }

    fn record(&mut self, reason: DropReason) {
        match reason {
            DropReason::RustPrivateCrossCrate => self.rust_private_cross_crate += 1,
            DropReason::PythonStdlibImport => self.python_stdlib_import += 1,
            DropReason::JsBarePackageImport => self.js_bare_package_import += 1,
        }
    }
}

/// One edge removed from the graph, with the files it joined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DroppedEdge {
    pub relationship_id: String,
    pub rel_type: String,
    pub source_file: String,
    pub target_file: String,
    pub reason: DropReason,
}

/// Counts of `edges` per reason.
pub fn count_by_reason(edges: &[DroppedEdge]) -> DroppedEdgeCounts {
    let mut counts = DroppedEdgeCounts::default();
    for edge in edges {
        counts.record(edge.reason);
    }
    counts
}

/// Every edge in `graph` that one of the rules proves false, sorted by
/// relationship id.
pub fn find_false_edges(graph: &ModuleDependencyGraph) -> Vec<DroppedEdge> {
    let mut judge = Judge::new(graph);
    let mut dropped: Vec<DroppedEdge> = graph
        .relationships()
        .values()
        .filter_map(|rel| {
            let (reason, source_file, target_file) = match rel.rel_type.as_str() {
                "CALLS" => judge.call(&rel.source_id, &rel.target_id)?,
                "IMPORTS" if rel.reason != MARKDOWN_LINK => {
                    judge.import(&rel.source_id, &rel.target_id)?
                }
                _ => return None,
            };
            Some(DroppedEdge {
                relationship_id: rel.id.clone(),
                rel_type: rel.rel_type.clone(),
                source_file: source_file.to_string(),
                target_file: target_file.to_string(),
                reason,
            })
        })
        .collect();
    dropped.sort_by(|a, b| a.relationship_id.cmp(&b.relationship_id));
    dropped
}

type Verdict<'g> = (DropReason, &'g str, &'g str);

/// The graph's files, by id and by repo-relative path, with their source.
struct Repo<'g> {
    graph: &'g ModuleDependencyGraph,
    path_of_id: HashMap<&'g str, &'g str>,
    paths: HashSet<&'g str>,
    content: HashMap<&'g str, &'g str>,
}

impl<'g> Repo<'g> {
    fn new(graph: &'g ModuleDependencyGraph) -> Self {
        let mut path_of_id = HashMap::new();
        let mut content = HashMap::new();
        for node in graph.nodes().values().filter(|node| node.label == "File") {
            let Some(path) = node.properties.get("filePath").and_then(Value::as_str) else {
                continue;
            };
            let path = normalized(path);
            path_of_id.insert(node.id.as_str(), path);
            if let Some(text) = node.properties.get("content").and_then(Value::as_str) {
                content.insert(path, text);
            }
        }
        let paths = path_of_id.values().copied().collect();
        Repo {
            graph,
            path_of_id,
            paths,
            content,
        }
    }

    fn has(&self, path: &str) -> bool {
        self.paths.contains(path)
    }
}

struct Judge<'g> {
    repo: Repo<'g>,
    rust: RustRule<'g>,
    python: PythonRule<'g>,
    js: JsRule<'g>,
}

impl<'g> Judge<'g> {
    fn new(graph: &'g ModuleDependencyGraph) -> Self {
        let repo = Repo::new(graph);
        Judge {
            rust: RustRule::default(),
            python: PythonRule::new(&repo),
            js: JsRule::new(&repo),
            repo,
        }
    }

    fn call(&mut self, source_id: &str, target_id: &str) -> Option<Verdict<'g>> {
        let (source, target) = self.rust.judge(&self.repo, source_id, target_id)?;
        Some((DropReason::RustPrivateCrossCrate, source, target))
    }

    fn import(&mut self, source_id: &str, target_id: &str) -> Option<Verdict<'g>> {
        let source = *self.repo.path_of_id.get(source_id)?;
        let target = *self.repo.path_of_id.get(target_id)?;
        if source == target {
            return None;
        }
        let reason = match extension(source) {
            "py" | "pyi" => self
                .python
                .judge(&self.repo, source, target)
                .then_some(DropReason::PythonStdlibImport),
            ext if js::is_js_extension(ext) => self
                .js
                .judge(&self.repo, source, target)
                .then_some(DropReason::JsBarePackageImport),
            _ => None,
        }?;
        Some((reason, source, target))
    }
}

#[cfg(test)]
mod tests;
