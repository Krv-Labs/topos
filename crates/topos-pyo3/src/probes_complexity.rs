//! Python bindings for `functors::probes::ast::complexity`.

use pyo3::prelude::*;

use crate::core::PyProgramObject;
use topos_core::functors::probes::ast::complexity::calculate_function_complexity_entries as core_entries;

#[pyclass(name = "FunctionComplexity", get_all)]
pub struct PyFunctionComplexity {
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub complexity: usize,
    pub includes_nested: bool,
}

#[pyfunction]
pub fn calculate_function_complexity_entries(ast: &PyProgramObject) -> Vec<PyFunctionComplexity> {
    core_entries(&ast.inner.uast_root, &ast.inner.source)
        .into_iter()
        .map(|e| PyFunctionComplexity {
            name: e.name,
            qualified_name: e.qualified_name,
            kind: e.kind.to_string(),
            start_line: e.start_line,
            end_line: e.end_line,
            complexity: e.complexity,
            includes_nested: true,
        })
        .collect()
}
