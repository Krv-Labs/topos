//! PDG profunctors — DDG and CDG edge-set Jaccards, statement / density
//! deltas.

pub mod compare;

pub use compare::{
    compare_pdg, control_dep_jaccard, data_dep_jaccard, density_delta, statement_delta,
    PDGComparison,
};
