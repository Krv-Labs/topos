//! Calls into a private free function of another crate.

use std::collections::HashMap;

use serde_json::Value;

use super::paths::{ancestors, join, parent};
use super::Repo;
use crate::functors::probes::mdg::coupling::owning_file;

#[derive(Default)]
pub(super) struct RustRule<'g> {
    owners: HashMap<String, Option<&'g str>>,
}

impl<'g> RustRule<'g> {
    /// The source and target files when this call cannot be real.
    pub(super) fn judge(
        &mut self,
        repo: &Repo<'g>,
        source_id: &str,
        target_id: &str,
    ) -> Option<(&'g str, &'g str)> {
        if !is_private_free_function(repo, target_id) {
            return None;
        }
        let source = self.owner(repo, source_id)?;
        let target = self.owner(repo, target_id)?;
        if !source.ends_with(".rs") || !target.ends_with(".rs") {
            return None;
        }
        (crate_root(repo, source)? != crate_root(repo, target)?).then_some((source, target))
    }

    fn owner(&mut self, repo: &Repo<'g>, node_id: &str) -> Option<&'g str> {
        if let Some(cached) = self.owners.get(node_id) {
            return *cached;
        }
        let path = owning_file(repo.graph, node_id)
            .and_then(|file_id| repo.path_of_id.get(file_id.as_str()).copied());
        self.owners.insert(node_id.to_string(), path);
        path
    }
}

fn is_private_free_function(repo: &Repo<'_>, node_id: &str) -> bool {
    let Some(node) = repo.graph.get_node(node_id) else {
        return false;
    };
    node.properties.get("isExported") == Some(&Value::Bool(false))
        && node
            .properties
            .get("filePath")
            .and_then(Value::as_str)
            .is_none_or(|path| path.ends_with(".rs"))
        && node.label != "Method"
        && repo.graph.incoming(node_id, Some("HAS_METHOD")).is_empty()
}

/// The directory of the nearest `Cargo.toml` above `path`.
fn crate_root<'p>(repo: &Repo<'_>, path: &'p str) -> Option<&'p str> {
    ancestors(parent(path)).find(|dir| repo.has(&join(dir, "Cargo.toml")))
}
