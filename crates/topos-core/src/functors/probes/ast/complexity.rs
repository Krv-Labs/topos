//! Per-function complexity analysis over the UAST.
//!
//! # Deviation from the Python original
//!
//! Python's `calculate_function_complexities` builds a per-function
//! sub-`ProgramObject` without wiring `uast_root`, so its intended
//! language-neutral path (walking `DECISION_UAST_KINDS`) never actually
//! runs — it silently falls back to Python-specific native tree-sitter
//! node-type strings (`"function_definition"`, `"elif_clause"`, ...),
//! so every other language gets `max_function_complexity = 0` always (a
//! vacuous pass of the `<= 10.0` gate). Filed as issue #153.
//!
//! This is a from-scratch implementation rather than a faithful port of
//! that bug: it walks the already-built UAST directly for
//! `FunctionDecl`/`MethodDecl` nodes, genuinely multi-language, and
//! simpler than Python's per-function AST reconstruction.

use std::collections::HashMap;

use crate::graphs::uast::models::{AttributeValue, UASTNode};

const DECISION_UAST_KINDS: &[&str] = &["IfStmt", "ForStmt", "WhileStmt", "MatchStmt", "TryStmt"];

/// Cyclomatic complexity of one callable's subtree: each decision node
/// (`IfStmt`/`ForStmt`/`WhileStmt`/`MatchStmt`/`TryStmt`) adds one, plus
/// a short-circuit `BinaryExpr` (`and`/`or`/`&&`/`||`) adds one.
///
/// The boolean-operator check is currently dormant — no UAST mapper
/// (issue #142) populates a `BinaryExpr`'s `"operator"` attribute with
/// token text yet, the same "mappers don't carry token text" limitation
/// noted in `graphs::pdg::object::identifier_name`. Kept because it's
/// what the Python original's own (also dormant) UAST-kind path
/// intended, and it costs nothing to leave wired for when a mapper
/// starts recording operator text.
fn node_complexity(node: &UASTNode) -> usize {
    fn walk(node: &UASTNode, count: &mut usize) {
        if DECISION_UAST_KINDS.contains(&node.kind.as_str()) {
            *count += 1;
        }
        if node.kind == "BinaryExpr" {
            if let Some(AttributeValue::Str(op)) = node.attributes.get("operator") {
                if matches!(op.as_str(), "and" | "or" | "&&" | "||") {
                    *count += 1;
                }
            }
        }
        for child in &node.children {
            walk(child, count);
        }
    }
    let mut count = 0;
    walk(node, &mut count);
    count + 1
}

/// Cyclomatic complexity for each function/method in a UAST, keyed by
/// UAST node id.
///
/// Python keys by extracted function *name*; this crate's mappers don't
/// carry token text yet (same limitation as `pdg::object`), so node id
/// is the only stable key available — and it has the incidental benefit
/// of not colliding on same-named nested/overloaded functions the way a
/// name-keyed map would.
pub fn calculate_function_complexities(uast_root: &UASTNode) -> HashMap<String, usize> {
    let mut complexities = HashMap::new();
    let mut stack: Vec<&UASTNode> = vec![uast_root];
    while let Some(node) = stack.pop() {
        if matches!(node.kind.as_str(), "FunctionDecl" | "MethodDecl") {
            complexities.insert(node.id.clone(), node_complexity(node));
        }
        stack.extend(node.children.iter());
    }
    complexities
}

/// Maximum cyclomatic complexity found in any function/method; `0` if
/// there are none.
pub fn calculate_max_function_complexity(uast_root: &UASTNode) -> usize {
    calculate_function_complexities(uast_root)
        .values()
        .copied()
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    #[test]
    fn flat_function_has_complexity_one() {
        let result = parse_source("def f(x):\n    return x\n", "python", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 1);
    }

    #[test]
    fn nested_if_and_for_increase_complexity() {
        let source = "def f(items):\n    for item in items:\n        if item:\n            return item\n    return None\n";
        let result = parse_source(source, "python", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 3);
    }

    #[test]
    fn works_across_languages_unlike_the_python_original() {
        // The point of issue #153: Rust functions must be counted too.
        let source = "fn f(x: i32) -> i32 {\n    if x > 0 {\n        return x;\n    }\n    0\n}\n";
        let result = parse_source(source, "rust", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 2);
    }

    #[test]
    fn no_functions_is_zero() {
        let result = parse_source("x = 1\n", "python", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 0);
    }
}
