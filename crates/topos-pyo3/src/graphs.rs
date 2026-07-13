//! Python bindings for `topos-core` graph representations.

use std::collections::HashMap;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::cfg::{BasicBlock as PyBasicBlock, CFGEdge as PyCFGEdge, EdgeKind as PyEdgeKind};
use crate::convert::{core_uast_to_py, py_uast_to_core};
use crate::uast::UASTNode;
use topos_core::graphs::base::Representation;
use topos_core::graphs::cfg::models::EdgeKind as CoreEdgeKind;
use topos_core::graphs::cfg::object::ControlFlowGraph as CoreCfg;
use topos_core::graphs::cpg::models::CPGEdgeKind as CoreCpgEdgeKind;
use topos_core::graphs::cpg::object::CodePropertyGraph as CoreCpg;
use topos_core::graphs::mdg::models::{GraphNode, GraphRelationship};
use topos_core::graphs::mdg::object::{MdgError, ModuleDependencyGraph as CoreMdg};
use topos_core::graphs::pdg::object::{
    DependenceEdge as CoreDependenceEdge, DependenceKind as CoreDependenceKind,
    ProgramDependenceGraph as CorePdg,
};

// ---------------------------------------------------------------------------
// CPG
// ---------------------------------------------------------------------------

#[pyclass(eq, eq_int, hash, frozen, from_py_object, name = "CPGEdgeKind")]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum PyCPGEdgeKind {
    AST = 0,
    CFG = 1,
    DDG = 2,
    CDG = 3,
}

impl From<CoreCpgEdgeKind> for PyCPGEdgeKind {
    fn from(kind: CoreCpgEdgeKind) -> Self {
        match kind {
            CoreCpgEdgeKind::Ast => PyCPGEdgeKind::AST,
            CoreCpgEdgeKind::Cfg => PyCPGEdgeKind::CFG,
            CoreCpgEdgeKind::Ddg => PyCPGEdgeKind::DDG,
            CoreCpgEdgeKind::Cdg => PyCPGEdgeKind::CDG,
        }
    }
}

impl From<PyCPGEdgeKind> for CoreCpgEdgeKind {
    fn from(kind: PyCPGEdgeKind) -> Self {
        match kind {
            PyCPGEdgeKind::AST => CoreCpgEdgeKind::Ast,
            PyCPGEdgeKind::CFG => CoreCpgEdgeKind::Cfg,
            PyCPGEdgeKind::DDG => CoreCpgEdgeKind::Ddg,
            PyCPGEdgeKind::CDG => CoreCpgEdgeKind::Cdg,
        }
    }
}

#[pymethods]
impl PyCPGEdgeKind {
    fn __str__(&self) -> &'static str {
        CoreCpgEdgeKind::from(*self).label()
    }
}

/// All four `CPGEdgeKind` variants. `#[pyclass]` enums have no
/// Python-level `__iter__` on the class itself (unlike `enum.Enum`), so
/// callers that need `for k in CPGEdgeKind:` use this instead.
#[pyfunction]
pub fn all_cpg_edge_kinds() -> Vec<PyCPGEdgeKind> {
    vec![
        PyCPGEdgeKind::AST,
        PyCPGEdgeKind::CFG,
        PyCPGEdgeKind::DDG,
        PyCPGEdgeKind::CDG,
    ]
}

#[pyclass(name = "CPGEdge", get_all, from_py_object)]
#[derive(Clone)]
pub struct PyCPGEdge {
    pub source: String,
    pub target: String,
    pub kind: PyCPGEdgeKind,
    pub label: String,
}

#[pyclass(name = "CPGNode", get_all, from_py_object)]
#[derive(Clone)]
pub struct PyCPGNode {
    pub uast: UASTNode,
}

#[pymethods]
impl PyCPGNode {
    #[getter]
    fn id(&self) -> &str {
        &self.uast.id
    }

    #[getter]
    fn kind(&self) -> &str {
        &self.uast.kind
    }

    #[getter]
    fn uast(&self) -> UASTNode {
        self.uast.clone()
    }

    #[getter]
    fn attributes(&self) -> HashMap<String, String> {
        self.uast.attributes.clone()
    }
}

#[pyclass(name = "CodePropertyGraph", unsendable)]
pub struct PyCodePropertyGraph {
    pub(crate) inner: CoreCpg,
}

#[pymethods]
impl PyCodePropertyGraph {
    #[getter]
    fn nodes(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (id, node) in &self.inner.nodes {
            dict.set_item(
                id,
                PyCPGNode {
                    uast: core_uast_to_py(&node.uast),
                },
            )?;
        }
        Ok(dict.into())
    }

    #[getter]
    fn edges(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        let list = PyList::empty(py);
        for edge in &self.inner.edges {
            list.append(PyCPGEdge {
                source: edge.source.clone(),
                target: edge.target.clone(),
                kind: edge.kind.into(),
                label: edge.label.clone(),
            })?;
        }
        Ok(list.into())
    }

    #[getter]
    fn language(&self) -> &str {
        &self.inner.language
    }

    #[getter]
    fn source(&self) -> &str {
        &self.inner.source
    }

    #[getter]
    fn name(&self) -> &str {
        self.inner.name()
    }

    #[getter]
    fn dimension(&self) -> &str {
        self.inner.dimension()
    }

    fn node_text(&self, node: &PyCPGNode) -> String {
        let core_node = topos_core::graphs::cpg::models::CPGNode {
            uast: py_uast_to_core(&node.uast),
        };
        self.inner.node_text(&core_node)
    }

    fn metrics(&self) -> HashMap<String, f64> {
        self.inner.metrics()
    }

    #[staticmethod]
    #[pyo3(signature = (uast_root, source = ""))]
    fn from_uast(uast_root: &UASTNode, source: &str) -> Self {
        let core_root = py_uast_to_core(uast_root);
        PyCodePropertyGraph {
            inner: CoreCpg::from_uast(&core_root, source),
        }
    }
}

// ---------------------------------------------------------------------------
// CFG (topos-core) — exposed as CoreControlFlowGraph (see cfg.rs's plain
// ControlFlowGraph for the separate, constructible-from-parts type used by
// the regression-diff / render.py subsystem — same Python module, distinct
// names to avoid a silent add_class() shadowing collision)
// ---------------------------------------------------------------------------

#[pyclass(name = "CoreControlFlowGraph", unsendable)]
pub struct PyCoreControlFlowGraph {
    pub(crate) inner: CoreCfg,
}

#[pymethods]
impl PyCoreControlFlowGraph {
    #[getter]
    fn name(&self) -> &str {
        self.inner.name()
    }

    #[getter]
    fn dimension(&self) -> &str {
        self.inner.dimension()
    }

    fn metrics(&self) -> HashMap<String, f64> {
        self.inner.metrics()
    }

    fn cyclomatic_complexity(&self) -> usize {
        self.inner.cyclomatic_complexity()
    }

    fn essential_complexity(&self) -> usize {
        self.inner.essential_complexity()
    }

    fn max_nesting_depth(&self) -> usize {
        self.inner.max_nesting_depth()
    }

    fn longest_acyclic_path(&self) -> usize {
        self.inner.longest_acyclic_path()
    }

    #[getter]
    fn blocks(&self) -> HashMap<usize, PyBasicBlock> {
        self.inner
            .blocks
            .iter()
            .map(|(&id, block)| {
                (
                    id,
                    PyBasicBlock {
                        id: block.id,
                        statements: block.statements.iter().map(core_uast_to_py).collect(),
                        label: block.label.clone(),
                    },
                )
            })
            .collect()
    }

    #[getter]
    fn edges(&self) -> Vec<PyCFGEdge> {
        self.inner
            .edges
            .iter()
            .map(|edge| PyCFGEdge {
                source: edge.source,
                target: edge.target,
                kind: core_edge_kind_to_py(edge.kind),
            })
            .collect()
    }

    #[getter]
    fn entry_id(&self) -> usize {
        self.inner.entry_id
    }

    #[getter]
    fn exit_id(&self) -> usize {
        self.inner.exit_id
    }
}

fn core_edge_kind_to_py(kind: CoreEdgeKind) -> PyEdgeKind {
    match kind {
        CoreEdgeKind::Unconditional => PyEdgeKind::UNCONDITIONAL,
        CoreEdgeKind::True => PyEdgeKind::TRUE,
        CoreEdgeKind::False => PyEdgeKind::FALSE,
        CoreEdgeKind::Loopback => PyEdgeKind::LOOPBACK,
        CoreEdgeKind::Break => PyEdgeKind::BREAK,
        CoreEdgeKind::Continue => PyEdgeKind::CONTINUE,
        CoreEdgeKind::Return => PyEdgeKind::RETURN,
        CoreEdgeKind::Exception => PyEdgeKind::EXCEPTION,
        CoreEdgeKind::SwitchCase => PyEdgeKind::SWITCHCASE,
    }
}

// ---------------------------------------------------------------------------
// PDG
// ---------------------------------------------------------------------------

#[pyclass(eq, eq_int, hash, frozen, from_py_object, name = "DependenceKind")]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum PyDependenceKind {
    DATA = 0,
    CONTROL = 1,
}

impl From<CoreDependenceKind> for PyDependenceKind {
    fn from(kind: CoreDependenceKind) -> Self {
        match kind {
            CoreDependenceKind::Data => PyDependenceKind::DATA,
            CoreDependenceKind::Control => PyDependenceKind::CONTROL,
        }
    }
}

#[pyclass(name = "DependenceEdge", get_all, from_py_object)]
#[derive(Clone)]
pub struct PyDependenceEdge {
    pub source: String,
    pub target: String,
    pub kind: PyDependenceKind,
    pub var: String,
}

impl From<&CoreDependenceEdge> for PyDependenceEdge {
    fn from(edge: &CoreDependenceEdge) -> Self {
        PyDependenceEdge {
            source: edge.source.clone(),
            target: edge.target.clone(),
            kind: edge.kind.into(),
            var: edge.var.clone(),
        }
    }
}

#[pyclass(name = "ProgramDependenceGraph", unsendable)]
pub struct PyProgramDependenceGraph {
    pub(crate) inner: CorePdg,
}

#[pymethods]
impl PyProgramDependenceGraph {
    #[getter]
    fn name(&self) -> &str {
        self.inner.name()
    }

    #[getter]
    fn dimension(&self) -> &str {
        self.inner.dimension()
    }

    fn metrics(&self) -> HashMap<String, f64> {
        self.inner.metrics()
    }

    #[getter]
    fn edges(&self) -> Vec<PyDependenceEdge> {
        self.inner
            .edges
            .iter()
            .map(PyDependenceEdge::from)
            .collect()
    }

    #[getter]
    fn statements(&self) -> Vec<UASTNode> {
        self.inner.statements.iter().map(core_uast_to_py).collect()
    }
}

// ---------------------------------------------------------------------------
// MDG
// ---------------------------------------------------------------------------

#[pyclass(name = "LadybugSchemaMismatchError", extends=PyValueError)]
pub struct PyLadybugSchemaMismatchError;

#[pymethods]
impl PyLadybugSchemaMismatchError {
    #[new]
    #[pyo3(signature = (*_args))]
    fn new(_args: &Bound<'_, pyo3::types::PyTuple>) -> Self {
        PyLadybugSchemaMismatchError
    }
}

#[pyclass(name = "GraphNode", get_all, from_py_object)]
#[derive(Clone)]
pub struct PyGraphNode {
    pub id: String,
    pub label: String,
    pub properties: HashMap<String, String>,
}

#[pymethods]
impl PyGraphNode {
    #[new]
    #[pyo3(signature = (id, label, properties=HashMap::new()))]
    fn new(id: String, label: String, properties: HashMap<String, String>) -> Self {
        PyGraphNode {
            id,
            label,
            properties,
        }
    }
}

#[pyclass(name = "GraphRelationship", get_all, from_py_object)]
#[derive(Clone)]
pub struct PyGraphRelationship {
    pub id: String,
    #[pyo3(name = "source_id")]
    pub source_id: String,
    #[pyo3(name = "target_id")]
    pub target_id: String,
    #[pyo3(name = "type")]
    pub rel_type: String,
    pub confidence: f64,
    pub reason: String,
}

#[pymethods]
impl PyGraphRelationship {
    #[new]
    #[pyo3(signature = (id, source_id, target_id, r#type, confidence=1.0, reason=String::new()))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: String,
        source_id: String,
        target_id: String,
        r#type: String,
        confidence: f64,
        reason: String,
    ) -> Self {
        PyGraphRelationship {
            id,
            source_id,
            target_id,
            rel_type: r#type,
            confidence,
            reason,
        }
    }
}

#[pyclass(name = "ModuleDependencyGraph", unsendable)]
pub struct PyModuleDependencyGraph {
    pub(crate) inner: CoreMdg,
}

#[pymethods]
impl PyModuleDependencyGraph {
    #[getter]
    fn target_file(&self) -> &str {
        &self.inner.target_file
    }

    fn file_node_id(&self) -> Option<&str> {
        self.inner.file_node_id()
    }

    #[getter]
    fn nodes(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (id, node) in &self.inner.nodes {
            dict.set_item(
                id,
                PyGraphNode {
                    id: node.id.clone(),
                    label: node.label.clone(),
                    properties: node
                        .properties
                        .iter()
                        .map(|(k, v)| (k.clone(), v.to_string()))
                        .collect(),
                },
            )?;
        }
        Ok(dict.into())
    }

    #[getter]
    fn relationships(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (id, rel) in &self.inner.relationships {
            dict.set_item(
                id,
                PyGraphRelationship {
                    id: rel.id.clone(),
                    source_id: rel.source_id.clone(),
                    target_id: rel.target_id.clone(),
                    rel_type: rel.rel_type.clone(),
                    confidence: rel.confidence,
                    reason: rel.reason.clone(),
                },
            )?;
        }
        Ok(dict.into())
    }

    #[getter]
    fn name(&self) -> &str {
        self.inner.name()
    }

    #[getter]
    fn dimension(&self) -> &str {
        self.inner.dimension()
    }

    fn metrics(&self) -> HashMap<String, f64> {
        self.inner.metrics()
    }

    #[staticmethod]
    fn from_gitnexus_dir(gitnexus_dir: &str, target_file: &str) -> PyResult<Self> {
        CoreMdg::from_gitnexus_dir(gitnexus_dir, target_file)
            .map(|inner| PyModuleDependencyGraph { inner })
            .map_err(mdg_error_to_py)
    }

    #[staticmethod]
    #[pyo3(signature = (target_file, nodes, relationships))]
    fn from_parts(
        target_file: &str,
        nodes: Vec<PyGraphNode>,
        relationships: Vec<PyGraphRelationship>,
    ) -> Self {
        let mut inner = CoreMdg::new(target_file);
        for node in nodes {
            inner.add_node(GraphNode {
                id: node.id,
                label: node.label,
                properties: node
                    .properties
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            });
        }
        for rel in relationships {
            inner.add_relationship(GraphRelationship {
                id: rel.id,
                source_id: rel.source_id,
                target_id: rel.target_id,
                rel_type: rel.rel_type,
                confidence: rel.confidence,
                reason: rel.reason,
            });
        }
        PyModuleDependencyGraph { inner }
    }
}

fn mdg_error_to_py(err: MdgError) -> PyErr {
    let msg = err.to_string();
    if msg.to_lowercase().contains("storage version")
        || msg.to_lowercase().contains("different version")
        || msg.to_lowercase().contains("ladybug")
    {
        return PyErr::new::<PyLadybugSchemaMismatchError, _>(msg);
    }
    PyValueError::new_err(msg)
}

// ---------------------------------------------------------------------------
// Representation extraction for classify_detailed
// ---------------------------------------------------------------------------

pub enum OwnedRep {
    Cfg(CoreCfg),
    Cpg(CoreCpg),
    Pdg(CorePdg),
    Mdg(CoreMdg),
}

impl Representation for OwnedRep {
    fn name(&self) -> &str {
        match self {
            OwnedRep::Cfg(r) => r.name(),
            OwnedRep::Cpg(r) => r.name(),
            OwnedRep::Pdg(r) => r.name(),
            OwnedRep::Mdg(r) => r.name(),
        }
    }

    fn dimension(&self) -> &str {
        match self {
            OwnedRep::Cfg(r) => r.dimension(),
            OwnedRep::Cpg(r) => r.dimension(),
            OwnedRep::Pdg(r) => r.dimension(),
            OwnedRep::Mdg(r) => r.dimension(),
        }
    }

    fn metrics(&self) -> HashMap<String, f64> {
        match self {
            OwnedRep::Cfg(r) => r.metrics(),
            OwnedRep::Cpg(r) => r.metrics(),
            OwnedRep::Pdg(r) => r.metrics(),
            OwnedRep::Mdg(r) => r.metrics(),
        }
    }
}

pub fn collect_owned_representations(reps: Option<&Bound<'_, PyList>>) -> PyResult<Vec<OwnedRep>> {
    let mut out = Vec::new();
    let Some(list) = reps else {
        return Ok(out);
    };
    for item in list.iter() {
        if let Ok(cfg) = item.extract::<PyRef<'_, PyCoreControlFlowGraph>>() {
            out.push(OwnedRep::Cfg(cfg.inner.clone()));
        } else if let Ok(cpg) = item.extract::<PyRef<'_, PyCodePropertyGraph>>() {
            out.push(OwnedRep::Cpg(cpg.inner.clone()));
        } else if let Ok(pdg) = item.extract::<PyRef<'_, PyProgramDependenceGraph>>() {
            out.push(OwnedRep::Pdg(pdg.inner.clone()));
        } else if let Ok(mdg) = item.extract::<PyRef<'_, PyModuleDependencyGraph>>() {
            out.push(OwnedRep::Mdg(mdg.inner.clone()));
        } else if item.hasattr("metrics")? && item.hasattr("dimension")? {
            return Err(PyTypeError::new_err(
                "Python-native Representation objects are no longer supported; use topos.topos_functors graph classes",
            ));
        }
    }
    Ok(out)
}
