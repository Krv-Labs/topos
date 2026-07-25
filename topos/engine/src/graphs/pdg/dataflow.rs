//! Stack-safe identifier definition/use discovery for PDG data edges.

use std::collections::HashSet;

use crate::graphs::uast::models::{AttributeValue, UASTNode};

/// Return `(defs, uses)` — variable names defined / used by `stmt`.
pub(super) fn defs_and_uses(stmt: &UASTNode, source: &str) -> (HashSet<String>, HashSet<String>) {
    let mut defs = HashSet::new();
    let mut uses = HashSet::new();

    let mut stack = vec![(stmt, false)];
    while let Some((node, in_lhs)) = stack.pop() {
        if node.kind == "AssignExpr" {
            let mut children = node.children.iter();
            if let Some(lhs) = children.next() {
                stack.extend(children.rev().map(|rhs| (rhs, false)));
                stack.push((lhs, true));
            }
            continue;
        }
        if node.kind == "Identifier" {
            record_identifier(node, in_lhs, source, &mut defs, &mut uses);
            continue;
        }
        stack.extend(node.children.iter().rev().map(|child| (child, in_lhs)));
    }
    (defs, uses)
}

fn record_identifier(
    node: &UASTNode,
    in_lhs: bool,
    source: &str,
    defs: &mut HashSet<String>,
    uses: &mut HashSet<String>,
) {
    let name = identifier_name(node, source);
    if name.is_empty() {
        return;
    }
    if in_lhs {
        defs.insert(name);
    } else {
        uses.insert(name);
    }
}

/// Best-effort recovery of an identifier's textual name.
fn identifier_name(node: &UASTNode, source: &str) -> String {
    if let Some(AttributeValue::Str(name)) = node.attributes.get("name") {
        if !name.is_empty() {
            return name.clone();
        }
    }
    if !source.is_empty() {
        let text = node_text(node, source);
        if !text.is_empty() {
            return text;
        }
    }
    node.id.clone()
}

/// Slice `source` by `node`'s byte span (best-effort).
fn node_text(node: &UASTNode, source: &str) -> String {
    let span = &node.span;
    let bytes = source.as_bytes();
    if span.end_byte > bytes.len() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes[span.start_byte..span.end_byte])
        .trim()
        .to_string()
}
