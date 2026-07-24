//! GitNexus `.gitnexus/` JSON-directory ingestion for
//! [`ModuleDependencyGraph`].
//!
//! Split out of `object.rs`: this file owns *how a graph gets built* from
//! disk (legacy JSON directory + native binary-store dispatch); `object.rs`
//! owns what the graph *is* and how it's queried once built. Two
//! `impl ModuleDependencyGraph` blocks in different files is ordinary
//! Rust -- the type doesn't care which file its methods live in.

use std::path::Path;

use serde_json::Value;

use super::models::{parse_node, parse_relationship};
use super::object::{MdgError, ModuleDependencyGraph};

impl ModuleDependencyGraph {
    /// Build a `ModuleDependencyGraph` from a `.gitnexus/` directory.
    ///
    /// Dispatches to the JSON-dir loader (GitNexus < 1.5) or the native
    /// LadybugDB binary-store loader (GitNexus ≥ 1.5) based on whether
    /// `lbug` is a directory or a file.
    pub fn from_gitnexus_dir(
        gitnexus_dir: impl AsRef<Path>,
        target_file: impl Into<String>,
    ) -> Result<Self, MdgError> {
        let lbug_path = gitnexus_dir.as_ref().join("lbug");
        Self::from_lbug_path(&lbug_path, target_file)
    }

    /// Load from whichever on-disk shape `lbug_path` has:
    ///
    /// - Directory → legacy JSON export (GitNexus < 1.5)
    /// - File → native LadybugDB binary store (GitNexus ≥ 1.5)
    pub fn from_lbug_path(
        lbug_path: &Path,
        target_file: impl Into<String>,
    ) -> Result<Self, MdgError> {
        if lbug_path.is_dir() {
            return Self::from_json_dir(lbug_path, target_file);
        }
        if lbug_path.is_file() {
            return Self::from_ladybug_native(lbug_path, target_file);
        }
        Err(MdgError::NotFound(lbug_path.to_path_buf()))
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
            Value::Array(items) => self.ingest_array_document(items),
            Value::Object(_) => self.ingest_object_document(data),
            _ => {}
        }
    }

    /// Ingest the flat-array document shape: a mixed list of node and
    /// relationship records, distinguished by their own fields.
    fn ingest_array_document(&mut self, items: &[Value]) {
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

    /// Ingest the `{nodes: [...], relationships: [...]}` document shape.
    fn ingest_object_document(&mut self, data: &Value) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_gitnexus_dir_binary_store_does_not_silently_skip() {
        let dir = std::env::temp_dir().join(format!("topos_mdg_json_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lbug"), b"binary-store-placeholder").unwrap();

        let result = ModuleDependencyGraph::from_gitnexus_dir(&dir, "x");
        // Placeholder bytes are not a real Ladybug store, so the native open
        // must fail — but it must not reclaim the old "unsupported / no
        // store" path.
        assert!(
            matches!(result, Err(MdgError::LadybugNativeFailed(_))),
            "unexpected result: {result:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
