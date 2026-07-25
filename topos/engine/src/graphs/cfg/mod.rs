//! The control-flow graph representation — [`models`] (data), [`builder`]
//! (UAST → CFG construction), [`object`] (the `Representation` and its
//! metrics).

pub mod builder;
pub mod models;
pub mod object;

#[cfg(test)]
mod edge_contracts;
