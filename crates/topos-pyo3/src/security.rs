//! Security probe bindings for MCP diagnostics.

use std::collections::HashSet;

use pyo3::prelude::*;

use crate::graphs::{PyCPGNode, PyCodePropertyGraph};
use topos_core::functors::probes::cpg::danger::{
    callee_from_text, dangerous_api_reachable, effective_registry, match_registry_key,
};
use topos_core::functors::probes::cpg::taint::taint_flow_paths;
use topos_core::graphs::cpg::models::{CPGEdgeKind, CPGNode};

#[pyfunction]
#[pyo3(signature = (cpg, allow=None))]
pub fn dangerous_api_reachable_py(
    cpg: &PyCodePropertyGraph,
    allow: Option<HashSet<String>>,
) -> usize {
    let allow = allow.unwrap_or_default();
    dangerous_api_reachable(&cpg.inner, &allow)
}

#[pyfunction]
#[pyo3(signature = (language, allow=None))]
pub fn effective_registry_py(language: &str, allow: Option<HashSet<String>>) -> HashSet<String> {
    let allow = allow.unwrap_or_default();
    effective_registry(language, &allow)
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}

#[pyfunction]
pub fn callee_from_text_py(text: &str) -> String {
    callee_from_text(text)
}

#[pyfunction]
#[pyo3(signature = (callee, keys))]
pub fn match_registry_key_py(callee: &str, keys: Vec<String>) -> Option<String> {
    match_registry_key(callee, keys.iter().map(|s| s.as_str())).map(|s| s.to_string())
}

#[pyfunction]
#[pyo3(signature = (callee, registry))]
pub fn matches_registry_py(callee: &str, registry: HashSet<String>) -> bool {
    topos_core::functors::probes::cpg::danger::matches_registry(
        callee,
        registry.iter().map(|s| s.as_str()),
    )
}

#[pyfunction]
#[pyo3(signature = (cpg, allow=None))]
pub fn taint_flow_paths_py(cpg: &PyCodePropertyGraph, allow: Option<HashSet<String>>) -> usize {
    let allow = allow.unwrap_or_default();
    taint_flow_paths(&cpg.inner, &allow)
}

#[pyfunction]
pub fn taint_sources_for_language(language: &str) -> HashSet<String> {
    let sources: &[&str] = match language {
        "python" => &[
            "input",
            "sys.argv",
            "request.args",
            "request.form",
            "request.json",
            "os.environ",
        ],
        "javascript" => &[
            "process.argv",
            "process.env",
            "req.body",
            "req.query",
            "document.location",
            "window.location",
        ],
        "typescript" => &["process.argv", "process.env", "req.body", "req.query"],
        "rust" => &["std::env::args", "std::env::var"],
        "cpp" => &["argv", "getenv", "scanf"],
        "go" => &[
            "os.Getenv",
            "os.Args",
            "r.FormValue",
            "r.URL",
            "flag.String",
        ],
        _ => &[],
    };
    sources.iter().map(|s| (*s).to_string()).collect()
}

#[pyfunction]
pub fn cpg_edge_kind_ddg() -> &'static str {
    CPGEdgeKind::Ddg.label()
}

#[pyfunction]
#[pyo3(signature = (cpg, node))]
pub fn cpg_node_text(cpg: &PyCodePropertyGraph, node: &PyCPGNode) -> String {
    let core_node = CPGNode {
        uast: crate::convert::py_uast_to_core(&node.uast),
    };
    cpg.inner.node_text(&core_node)
}
