//! The AST layer: language dispatch, parse artifacts, and the
//! `Representation` adapter over [`crate::core::object::ProgramObject`].

pub mod dispatch;
pub mod languages;
pub mod object;
pub mod types;
