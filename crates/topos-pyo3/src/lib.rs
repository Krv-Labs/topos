pub mod cfg;
pub mod convert;
pub mod core;
pub mod graphs;
pub mod probes_ast;
pub mod probes_complexity;
pub mod profunctors;
pub mod security;
pub mod uast;

use pyo3::prelude::*;

#[pymodule]
fn topos_functors(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<uast::SourceSpan>()?;
    m.add_class::<uast::NativeRef>()?;
    m.add_class::<uast::UASTNode>()?;

    m.add_class::<cfg::EdgeKind>()?;
    m.add_class::<cfg::BasicBlock>()?;
    m.add_class::<cfg::CFGEdge>()?;
    m.add_class::<cfg::ControlFlowGraph>()?;

    m.add_class::<probes_ast::EntropyResult>()?;
    m.add_function(wrap_pyfunction!(probes_ast::calculate_kolmogorov_proxy, m)?)?;
    m.add_function(wrap_pyfunction!(probes_ast::calculate_entropy_detailed, m)?)?;
    m.add_function(wrap_pyfunction!(probes_ast::calculate_block_entropy, m)?)?;
    m.add_function(wrap_pyfunction!(probes_ast::calculate_entropy_variance, m)?)?;

    m.add_class::<probes_complexity::PyFunctionComplexity>()?;
    m.add_function(wrap_pyfunction!(
        probes_complexity::calculate_function_complexity_entries,
        m
    )?)?;

    m.add_class::<profunctors::DistanceResult>()?;
    m.add_function(wrap_pyfunction!(profunctors::compute_sequence_distance, m)?)?;
    m.add_function(wrap_pyfunction!(profunctors::calculate_ast_distance, m)?)?;
    m.add_function(wrap_pyfunction!(profunctors::calculate_similarity, m)?)?;
    m.add_function(wrap_pyfunction!(profunctors::structural_distance, m)?)?;
    m.add_class::<profunctors::GHWDistanceResult>()?;
    m.add_function(wrap_pyfunction!(profunctors::calculate_ghw_distance, m)?)?;

    m.add_class::<core::PyEvaluationValue>()?;
    m.add_class::<core::PyProgramObject>()?;
    m.add_class::<core::PyProgramMorphism>()?;
    m.add_class::<core::PyClassificationResult>()?;
    m.add_class::<core::PyCharacteristicMorphism>()?;
    m.add_function(wrap_pyfunction!(core::verdict_from_generators, m)?)?;
    m.add_function(wrap_pyfunction!(core::all_evaluation_values, m)?)?;

    m.add_class::<graphs::PyCPGEdgeKind>()?;
    m.add_function(wrap_pyfunction!(graphs::all_cpg_edge_kinds, m)?)?;
    m.add_class::<graphs::PyCPGEdge>()?;
    m.add_class::<graphs::PyCPGNode>()?;
    m.add_class::<graphs::PyCodePropertyGraph>()?;
    m.add_class::<graphs::PyCoreControlFlowGraph>()?;
    m.add_class::<graphs::PyDependenceKind>()?;
    m.add_class::<graphs::PyDependenceEdge>()?;
    m.add_class::<graphs::PyProgramDependenceGraph>()?;
    m.add_class::<graphs::PyLadybugSchemaMismatchError>()?;
    m.add_class::<graphs::PyGraphNode>()?;
    m.add_class::<graphs::PyGraphRelationship>()?;
    m.add_class::<graphs::PyModuleDependencyGraph>()?;

    m.add_function(wrap_pyfunction!(security::dangerous_api_reachable_py, m)?)?;
    m.add_function(wrap_pyfunction!(security::effective_registry_py, m)?)?;
    m.add_function(wrap_pyfunction!(security::callee_from_text_py, m)?)?;
    m.add_function(wrap_pyfunction!(security::match_registry_key_py, m)?)?;
    m.add_function(wrap_pyfunction!(security::matches_registry_py, m)?)?;
    m.add_function(wrap_pyfunction!(security::taint_sources_for_language, m)?)?;
    m.add_function(wrap_pyfunction!(security::taint_flow_paths_py, m)?)?;
    m.add_function(wrap_pyfunction!(security::cpg_edge_kind_ddg, m)?)?;
    m.add_function(wrap_pyfunction!(security::cpg_node_text, m)?)?;

    Ok(())
}
