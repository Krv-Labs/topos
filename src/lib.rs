pub mod cfg;
pub mod frc;
pub mod ph;
pub mod probes_ast;
pub mod profunctors;
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

    m.add_class::<ph::CycleGenerator>()?;
    m.add_class::<ph::CycleBasisResult>()?;

    m.add_class::<frc::WeightedEdge>()?;
    m.add_class::<frc::EdgeCurvature>()?;
    m.add_function(wrap_pyfunction!(frc::balanced_forman_curvature, m)?)?;
    m.add_function(wrap_pyfunction!(frc::directed_forman_curvature, m)?)?;

    m.add_class::<probes_ast::EntropyResult>()?;
    m.add_function(wrap_pyfunction!(probes_ast::calculate_kolmogorov_proxy, m)?)?;
    m.add_function(wrap_pyfunction!(probes_ast::calculate_entropy_detailed, m)?)?;
    m.add_function(wrap_pyfunction!(probes_ast::calculate_block_entropy, m)?)?;
    m.add_function(wrap_pyfunction!(probes_ast::calculate_entropy_variance, m)?)?;

    m.add_class::<profunctors::DistanceResult>()?;
    m.add_function(wrap_pyfunction!(profunctors::compute_sequence_distance, m)?)?;

    Ok(())
}
