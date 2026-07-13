//! CFG profunctors — cyclomatic / edge-kind / longest-path deltas.

pub mod compare;

pub use compare::{
    compare_cfg, cyclomatic_delta, edge_kind_histogram, edge_kind_l1_distance, longest_path_delta,
    CFGComparison,
};
