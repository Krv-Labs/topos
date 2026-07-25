//! AST profunctors — tree edit distance and Gromov-Wasserstein over
//! raw ASTs.

pub mod compare;

pub use compare::{
    calculate_ast_distance, calculate_ghw_distance, calculate_similarity, structural_distance,
    DistanceResult, GHWDistanceResult, GhwOptions,
};
