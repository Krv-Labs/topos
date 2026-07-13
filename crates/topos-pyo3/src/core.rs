//! Python bindings for `topos-core`'s categorical primitives and evaluation.

use std::cell::RefCell;
use std::collections::HashMap;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::convert::core_uast_to_py;
use crate::graphs::{
    collect_owned_representations, PyCodePropertyGraph, PyCoreControlFlowGraph,
    PyProgramDependenceGraph,
};
use crate::uast::UASTNode;
use topos_core::core::morphism::ProgramMorphism;
use topos_core::core::object::ProgramObject;
use topos_core::core::omega::EvaluationValue;
use topos_core::evaluation::characteristic_morphism::{
    CharacteristicMorphism, ClassificationResult,
};
use topos_core::evaluation::policies::base::Priority;
use topos_core::graphs::base::Representation;

/// `Ω`'s eight elements — mirrors `topos.core.omega.EvaluationValue`.
#[pyclass(eq, eq_int, hash, frozen, from_py_object, name = "EvaluationValue")]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum PyEvaluationValue {
    SLOP = 0b000,
    SIMPLE = 0b001,
    COMPOSABLE = 0b010,
    SIMPLE_COMPOSABLE = 0b011,
    SECURE = 0b100,
    SIMPLE_SECURE = 0b101,
    COMPOSABLE_SECURE = 0b110,
    IDEAL = 0b111,
}

impl From<EvaluationValue> for PyEvaluationValue {
    fn from(value: EvaluationValue) -> Self {
        match value {
            EvaluationValue::Slop => PyEvaluationValue::SLOP,
            EvaluationValue::Simple => PyEvaluationValue::SIMPLE,
            EvaluationValue::Composable => PyEvaluationValue::COMPOSABLE,
            EvaluationValue::SimpleComposable => PyEvaluationValue::SIMPLE_COMPOSABLE,
            EvaluationValue::Secure => PyEvaluationValue::SECURE,
            EvaluationValue::SimpleSecure => PyEvaluationValue::SIMPLE_SECURE,
            EvaluationValue::ComposableSecure => PyEvaluationValue::COMPOSABLE_SECURE,
            EvaluationValue::Ideal => PyEvaluationValue::IDEAL,
        }
    }
}

impl From<PyEvaluationValue> for EvaluationValue {
    fn from(value: PyEvaluationValue) -> Self {
        match value {
            PyEvaluationValue::SLOP => EvaluationValue::Slop,
            PyEvaluationValue::SIMPLE => EvaluationValue::Simple,
            PyEvaluationValue::COMPOSABLE => EvaluationValue::Composable,
            PyEvaluationValue::SIMPLE_COMPOSABLE => EvaluationValue::SimpleComposable,
            PyEvaluationValue::SECURE => EvaluationValue::Secure,
            PyEvaluationValue::SIMPLE_SECURE => EvaluationValue::SimpleSecure,
            PyEvaluationValue::COMPOSABLE_SECURE => EvaluationValue::ComposableSecure,
            PyEvaluationValue::IDEAL => EvaluationValue::Ideal,
        }
    }
}

#[pymethods]
impl PyEvaluationValue {
    #[getter]
    fn symbol(&self) -> &'static str {
        EvaluationValue::from(*self).symbol()
    }

    #[getter]
    fn description(&self) -> &'static str {
        EvaluationValue::from(*self).description()
    }

    #[getter]
    fn name(&self) -> &'static str {
        EvaluationValue::from(*self).name()
    }

    fn __str__(&self) -> String {
        EvaluationValue::from(*self).to_string()
    }
}

fn priority_from_str(value: &str) -> PyResult<Priority> {
    match value {
        "simple" => Ok(Priority::Simple),
        "composable" => Ok(Priority::Composable),
        "secure" => Ok(Priority::Secure),
        other => Err(PyValueError::new_err(format!(
            "invalid priority {other:?}; expected one of \"simple\", \"composable\", \"secure\""
        ))),
    }
}

fn priority_to_str(priority: Priority) -> &'static str {
    match priority {
        Priority::Simple => "simple",
        Priority::Composable => "composable",
        Priority::Secure => "secure",
    }
}

#[pyclass(name = "ProgramObject", unsendable)]
pub struct PyProgramObject {
    pub(crate) inner: ProgramObject,
}

#[pymethods]
impl PyProgramObject {
    #[getter]
    fn source(&self) -> &str {
        &self.inner.source
    }

    #[getter]
    fn language(&self) -> &str {
        &self.inner.language
    }

    #[getter]
    fn is_valid(&self) -> bool {
        self.inner.is_valid()
    }

    #[getter]
    fn uast_root(&self) -> UASTNode {
        core_uast_to_py(&self.inner.uast_root)
    }

    #[getter]
    fn parser_name(&self) -> &str {
        &self.inner.parser_name
    }

    #[getter]
    fn parser_version(&self) -> &str {
        &self.inner.parser_version
    }
}

#[pyclass(name = "ProgramMorphism", unsendable)]
pub struct PyProgramMorphism {
    pub(crate) inner: RefCell<ProgramMorphism>,
}

#[pymethods]
impl PyProgramMorphism {
    #[new]
    #[pyo3(signature = (source, language=None))]
    fn new(source: String, language: Option<String>) -> Self {
        let language = language.unwrap_or_else(|| "python".to_string());
        PyProgramMorphism {
            inner: RefCell::new(ProgramMorphism::new(source, language)),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (filepath, language=None))]
    fn from_file(filepath: String, language: Option<String>) -> PyResult<Self> {
        let language = language.unwrap_or_else(|| "python".to_string());
        ProgramMorphism::from_file(filepath, language)
            .map(|inner| PyProgramMorphism {
                inner: RefCell::new(inner),
            })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[getter]
    fn source(&self) -> String {
        self.inner.borrow().source.clone()
    }

    #[getter]
    fn language(&self) -> String {
        self.inner.borrow().language.clone()
    }

    #[getter]
    fn filepath(&self) -> Option<String> {
        self.inner
            .borrow()
            .filepath
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
    }

    #[getter]
    fn is_valid(&self) -> bool {
        self.inner.borrow().is_valid()
    }

    #[getter]
    fn name(&self) -> String {
        self.inner.borrow().name()
    }

    #[getter]
    fn ast(&self) -> Option<PyProgramObject> {
        self.inner
            .borrow()
            .ast
            .as_ref()
            .cloned()
            .map(|inner| PyProgramObject { inner })
    }

    fn build_cfg(&self) -> Option<PyCoreControlFlowGraph> {
        self.inner
            .borrow_mut()
            .build_cfg()
            .map(|cfg| PyCoreControlFlowGraph { inner: cfg.clone() })
    }

    fn build_pdg(&self) -> Option<PyProgramDependenceGraph> {
        self.inner
            .borrow_mut()
            .build_pdg()
            .map(|pdg| PyProgramDependenceGraph { inner: pdg.clone() })
    }

    fn build_cpg(&self) -> Option<PyCodePropertyGraph> {
        self.inner
            .borrow_mut()
            .build_cpg()
            .map(|cpg| PyCodePropertyGraph { inner: cpg.clone() })
    }

    fn __eq__(&self, other: &Self) -> bool {
        *self.inner.borrow() == *other.inner.borrow()
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.inner.borrow().hash(&mut hasher);
        hasher.finish()
    }
}

#[pyclass(name = "ClassificationResult", get_all, set_all, from_py_object)]
#[derive(Clone)]
pub struct PyClassificationResult {
    pub is_parseable: bool,
    pub dimensions: HashMap<String, PyEvaluationValue>,
    pub scores: HashMap<String, f64>,
    pub lattice_element: PyEvaluationValue,
    pub priority: String,
    pub raw_metrics: HashMap<String, f64>,
    pub interpretation: HashMap<String, String>,
    pub is_entrypoint_module: bool,
}

impl From<ClassificationResult> for PyClassificationResult {
    fn from(result: ClassificationResult) -> Self {
        PyClassificationResult {
            is_parseable: result.is_parseable,
            dimensions: result
                .dimensions
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            scores: result.scores,
            lattice_element: result.lattice_element.into(),
            priority: priority_to_str(result.priority).to_string(),
            raw_metrics: result.raw_metrics,
            interpretation: result.interpretation,
            is_entrypoint_module: result.is_entrypoint_module,
        }
    }
}

#[pymethods]
impl PyClassificationResult {
    #[new]
    #[pyo3(signature = (
        is_parseable,
        dimensions=HashMap::new(),
        scores=HashMap::new(),
        lattice_element=PyEvaluationValue::SLOP,
        priority="secure".to_string(),
        raw_metrics=HashMap::new(),
        interpretation=HashMap::new(),
        is_entrypoint_module=false,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        is_parseable: bool,
        dimensions: HashMap<String, PyEvaluationValue>,
        scores: HashMap<String, f64>,
        lattice_element: PyEvaluationValue,
        priority: String,
        raw_metrics: HashMap<String, f64>,
        interpretation: HashMap<String, String>,
        is_entrypoint_module: bool,
    ) -> Self {
        PyClassificationResult {
            is_parseable,
            dimensions,
            scores,
            lattice_element,
            priority,
            raw_metrics,
            interpretation,
            is_entrypoint_module,
        }
    }

    fn summary(&self) -> PyEvaluationValue {
        self.lattice_element
    }

    fn __str__(&self) -> String {
        if !self.is_parseable {
            return "Classification: \u{22a5} SLOP (parse failure)".to_string();
        }
        let mut lines = vec![format!(
            "Classification: {}",
            self.lattice_element.__str__()
        )];
        let mut dims: Vec<_> = self.dimensions.iter().collect();
        dims.sort_by_key(|(dim, _)| dim.as_str());
        for (dim, val) in dims {
            let score_pct = self.scores.get(dim).copied().unwrap_or(0.0) * 100.0;
            lines.push(format!("  {dim}: {}  [{score_pct:.0}%]", val.__str__()));
        }
        let mut metrics: Vec<_> = self.raw_metrics.iter().collect();
        metrics.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in metrics {
            lines.push(format!("    {k}: {v:.3}"));
        }
        lines.join("\n")
    }
}

#[pyclass(name = "CharacteristicMorphism")]
pub struct PyCharacteristicMorphism;

#[pymethods]
impl PyCharacteristicMorphism {
    #[new]
    fn new() -> Self {
        PyCharacteristicMorphism
    }

    #[pyo3(signature = (morphism, representations=None, priority="secure"))]
    fn classify_detailed<'py>(
        &self,
        morphism: &PyProgramMorphism,
        representations: Option<&Bound<'py, PyList>>,
        priority: &str,
    ) -> PyResult<PyClassificationResult> {
        let owned = collect_owned_representations(representations)?;
        let refs: Vec<&dyn Representation> =
            owned.iter().map(|r| r as &dyn Representation).collect();
        let result = CharacteristicMorphism.classify_detailed(
            &morphism.inner.borrow(),
            &refs,
            priority_from_str(priority)?,
        );
        Ok(result.into())
    }

    fn classify(&self, morphism: &PyProgramMorphism) -> PyEvaluationValue {
        CharacteristicMorphism
            .classify(&morphism.inner.borrow())
            .into()
    }
}

#[pyfunction]
#[pyo3(signature = (simple, composable, secure))]
pub fn verdict_from_generators(simple: bool, composable: bool, secure: bool) -> PyEvaluationValue {
    topos_core::core::omega::verdict_from_generators(simple, composable, secure).into()
}

/// All eight `EvaluationValue` elements, in `EvaluationValue::ALL` order.
///
/// `#[pyclass]` enums have no Python-level `__iter__` on the class itself
/// (unlike `enum.Enum`), so callers that need `for v in EvaluationValue:`
/// use this instead.
#[pyfunction]
pub fn all_evaluation_values() -> Vec<PyEvaluationValue> {
    EvaluationValue::ALL.iter().map(|&v| v.into()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluation_value_round_trips_through_python_wrapper() {
        for value in EvaluationValue::ALL {
            let py_value: PyEvaluationValue = value.into();
            let back: EvaluationValue = py_value.into();
            assert_eq!(value.bits(), back.bits());
        }
    }

    #[test]
    fn classify_matches_core_crate_directly() {
        let morphism = ProgramMorphism::new("x = 1".to_string(), "python");
        let classifier = CharacteristicMorphism;
        let result = classifier.classify_detailed(&morphism, &[], Priority::default());
        assert!(result.is_parseable);
    }
}
