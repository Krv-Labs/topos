//! CFG models — data structures for an intra-procedural control-flow
//! graph built on top of the language-independent UAST.
//!
//! A CFG consists of *basic blocks* (maximal straight-line UAST-statement
//! sequences with single entry and single exit) connected by typed
//! control-flow edges:
//!
//! - `Unconditional` — fall-through into the next block
//! - `True` / `False` — conditional branches out of an `IfStmt` or loop test
//! - `Loopback` — back-edge from end-of-body to loop header
//! - `Break` — exit from a loop / switch
//! - `Continue` — back-edge to the loop test
//! - `Return` — early return to the procedure exit block
//! - `Exception` — try/catch fall-through
//! - `SwitchCase` — case-arm selection
//!
//! The graph always has a unique *entry* block (synthetic) and a unique
//! *exit* block (synthetic). This invariant is required for McCabe
//! cyclomatic complexity to evaluate as `E - N + 2P` with `P = 1`.
//!
//! This module was originally written directly in Rust (predating the
//! v0.4.0 migration) to back a `topos-pyo3` probe; it's relocated here
//! unchanged in shape, since it already matches Python's
//! `graphs/cfg/models.py` field-for-field.

use crate::graphs::uast::models::UASTNode;
use std::collections::HashMap;

/// Typed control-flow edge labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    Unconditional,
    True,
    False,
    Loopback,
    Break,
    Continue,
    Return,
    Exception,
    SwitchCase,
}

/// A maximal straight-line sequence of UAST statements.
#[derive(Debug, Clone)]
pub struct BasicBlock {
    /// Unique integer id within the owning CFG.
    pub id: usize,
    /// The UAST nodes executed in order on entry to this block. Empty
    /// for the synthetic entry/exit blocks.
    pub statements: Vec<UASTNode>,
    /// Human-readable label (`"entry"`, `"exit"`, `"if_then"`, …).
    pub label: String,
}

impl BasicBlock {
    pub fn new(id: usize, label: impl Into<String>) -> Self {
        BasicBlock {
            id,
            statements: Vec::new(),
            label: label.into(),
        }
    }
}

/// A typed edge between two basic blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CFGEdge {
    pub source: usize,
    pub target: usize,
    pub kind: EdgeKind,
}

impl CFGEdge {
    pub fn new(source: usize, target: usize, kind: EdgeKind) -> Self {
        CFGEdge {
            source,
            target,
            kind,
        }
    }
}

pub type Blocks = HashMap<usize, BasicBlock>;
