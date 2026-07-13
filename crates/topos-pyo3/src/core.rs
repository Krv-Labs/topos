//! Python bindings for `topos-core`'s categorical primitives.
//!
//! This is a **scoped** first cut at issue #148 (expand `topos-pyo3` to
//! the full `topos-core` surface), not the complete binding layer.
//! Exposed here: [`PyEvaluationValue`] (Ω's elements), [`PyProgramMorphism`],
//! [`PyClassificationResult`], and [`PyCharacteristicMorphism`] — enough
//! to prove the classify path works end-to-end across the FFI boundary
//! (`ProgramMorphism::new` → `CharacteristicMorphism::classify_detailed`
//! → a `ClassificationResult` Python can read).
//!
//! # What is deliberately not bound yet
//!
//! `topos/mcp/**` (the ~10 call sites across `evaluate.py`, `compare.py`,
//! `coverage.py`, `inspect.py`, `diagnostics.py`, `assess/*.py`,
//! `metric_locations.py`) also construct `ProgramMorphism` and then call
//! `.build_cfg()` / `.build_cpg()` / `.build_pdg()`, passing the result
//! into `classify_detailed(representations=[...])`, and separately
//! inspect CFG/CPG/MDG/PDG objects directly (e.g. CPG node/edge
//! iteration for security-finding rendering). Rewiring those callers
//! requires the *representation* types (`ControlFlowGraph`,
//! `CodePropertyGraph`, `ModuleDependencyGraph`,
//! `ProgramDependenceGraph`) as pyclasses too, with the same attribute
//! surface their Python counterparts have — that is a submodule-sized
//! task in its own right, not a mechanical follow-on to this file.
//! Swapping any MCP caller over without it would either drop
//! CFG/CPG-derived generators from real MCP tool output or require
//! duck-typing around a missing API, silently changing production
//! behavior. Left as explicit, scoped follow-up (tracked in issue #148)
//! rather than attempted here.
//!
//! This module intentionally does not (yet) expose `Omega`'s lattice
//! operations, `Priority`, or the `representations` parameter of
//! `classify_detailed` — `classify`/`classify_detailed` here always use
//! the default priority and only the automatically-built AST
//! representation, matching the simplest and most common Python call
//! pattern (`CharacteristicMorphism().classify(morphism)`).

use std::collections::HashMap;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use topos_core::core::morphism::ProgramMorphism;
use topos_core::core::omega::EvaluationValue;
use topos_core::evaluation::characteristic_morphism::{
    CharacteristicMorphism, ClassificationResult,
};
use topos_core::evaluation::policies::base::Priority;

/// `Ω`'s eight elements — mirrors `topos.core.omega.EvaluationValue`
/// (a Python `IntEnum`) member-for-member, including the exact bitmask
/// values, so existing Python comparisons/serialization keep working
/// once a caller is rewired to receive this type instead.
#[pyclass(eq, eq_int, from_py_object, name = "EvaluationValue")]
#[derive(Clone, Copy, PartialEq, Eq)]
// Variant names are SCREAMING_CASE, not Rust's usual UpperCamelCase, on
// purpose: they cross the FFI boundary as-is, and Python callers expect
// `EvaluationValue.SIMPLE_COMPOSABLE` to match the pure-Python original.
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

/// A program viewed as a transformation between computational states —
/// see `topos_core::core::morphism` for the categorical framing. Wraps
/// the Rust `ProgramMorphism`; the Python-facing constructor mirrors the
/// pure-Python original's `ProgramMorphism(source=..., language=...)`.
///
/// `unsendable`: `ProgramMorphism` holds a `Cell` (single-threaded
/// caching in `ProgramObject`) and `Box<dyn Representation>` trait
/// objects with no `Send`/`Sync` bounds — neither is needed by
/// `topos-core` itself (a pure, non-pyo3 library has no reason to force
/// thread-safety on every field), so this constraint is absorbed here
/// at the Python-binding layer instead of leaking into the core crate.
/// `unsendable` restricts Python-side access to the thread that created
/// the object, which matches how it's actually used (GIL-serialized,
/// single-threaded MCP tool calls).
#[pyclass(name = "ProgramMorphism", unsendable)]
pub struct PyProgramMorphism {
    pub(crate) inner: ProgramMorphism,
}

#[pymethods]
impl PyProgramMorphism {
    #[new]
    #[pyo3(signature = (source, language=None))]
    fn new(source: String, language: Option<String>) -> Self {
        let language = language.unwrap_or_else(|| "python".to_string());
        PyProgramMorphism {
            inner: ProgramMorphism::new(source, language),
        }
    }

    #[staticmethod]
    #[pyo3(signature = (filepath, language=None))]
    fn from_file(filepath: String, language: Option<String>) -> PyResult<Self> {
        let language = language.unwrap_or_else(|| "python".to_string());
        ProgramMorphism::from_file(filepath, language)
            .map(|inner| PyProgramMorphism { inner })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[getter]
    fn source(&self) -> &str {
        &self.inner.source
    }

    #[getter]
    fn language(&self) -> &str {
        &self.inner.language
    }

    #[getter]
    fn filepath(&self) -> Option<String> {
        self.inner
            .filepath
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
    }

    #[getter]
    fn is_valid(&self) -> bool {
        self.inner.is_valid()
    }

    #[getter]
    fn name(&self) -> String {
        self.inner.name()
    }
}

/// The image of one program morphism under `χ_S : P → Ω` — see
/// `topos_core::evaluation::characteristic_morphism`.
#[pyclass(name = "ClassificationResult", get_all, from_py_object)]
#[derive(Clone)]
pub struct PyClassificationResult {
    pub is_parseable: bool,
    pub dimensions: HashMap<String, PyEvaluationValue>,
    pub scores: HashMap<String, f64>,
    pub lattice_element: PyEvaluationValue,
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
            raw_metrics: result.raw_metrics,
            interpretation: result.interpretation,
            is_entrypoint_module: result.is_entrypoint_module,
        }
    }
}

#[pymethods]
impl PyClassificationResult {
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
        for (dim, val) in &self.dimensions {
            let score_pct = self.scores.get(dim).copied().unwrap_or(0.0) * 100.0;
            lines.push(format!("  {dim}: {}  [{score_pct:.0}%]", val.__str__()));
        }
        for (k, v) in &self.raw_metrics {
            lines.push(format!("    {k}: {v:.3}"));
        }
        lines.join("\n")
    }
}

/// `χ_S : P → Ω` — see `topos_core::evaluation::characteristic_morphism`.
///
/// `classify`/`classify_detailed` here always use the default priority
/// and only the automatically-built AST representation — see this
/// module's doc comment for why `representations`/`priority` aren't
/// exposed yet.
#[pyclass(name = "CharacteristicMorphism")]
pub struct PyCharacteristicMorphism;

#[pymethods]
impl PyCharacteristicMorphism {
    #[new]
    fn new() -> Self {
        PyCharacteristicMorphism
    }

    fn classify(&self, morphism: &PyProgramMorphism) -> PyEvaluationValue {
        CharacteristicMorphism.classify(&morphism.inner).into()
    }

    fn classify_detailed(&self, morphism: &PyProgramMorphism) -> PyClassificationResult {
        CharacteristicMorphism
            .classify_detailed(&morphism.inner, &[], Priority::default())
            .into()
    }
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
        let morphism = PyProgramMorphism::new("x = 1".to_string(), None);
        let classifier = PyCharacteristicMorphism;
        let result = classifier.classify_detailed(&morphism);
        assert!(result.is_parseable);
    }
}
