//! MDG profunctors — coupling / instability / fan-in-out / dep-depth
//! deltas.

pub mod compare;

pub use compare::{
    compare_mdg, coupling_delta, dep_depth_delta, fan_in_delta, fan_out_delta, instability_delta,
    MDGComparison,
};
