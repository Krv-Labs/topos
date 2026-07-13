//! CPG profunctors — per-edge-family Jaccards + danger / taint deltas.

pub mod compare;

pub use compare::{
    compare_cpg, dangerous_delta, family_jaccards, node_jaccard, taint_delta, CPGComparison,
};
