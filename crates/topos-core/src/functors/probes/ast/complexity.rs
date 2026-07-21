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

const DECISION_UAST_KINDS: &[&str] = &["IfStmt", "ForStmt", "WhileStmt", "TryStmt"];

/// Cyclomatic complexity of one callable's subtree: each decision node
/// (`IfStmt`/`ForStmt`/`WhileStmt`/`TryStmt`) adds one, a `MatchStmt` adds
/// one per case arm beyond the first (a k-way switch is k-1 decisions), so
/// this agrees with `cfg.cyclomatic`; plus a short-circuit `BinaryExpr`
/// (`and`/`or`/`&&`/`||`) adds one.
///
/// Note: counting per-arm here intentionally diverges from the last Python
/// release (`topos-mcp==0.3.11`), which counted a whole match/switch as a
/// single decision. The divergence is documented in the `[0.4.0]` CHANGELOG
/// entry (the parity/benchmark harness that originally allowlisted it was a
/// migration-verification artifact and has since been removed).
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
        match node.kind.as_str() {
            // A k-way switch/match contributes k branches (k - 1 decisions),
            // counted from its arms so this agrees with the CFG builder.
            "MatchStmt" => {
                *count += crate::graphs::cfg::builder::match_arm_count(node).saturating_sub(1);
            }
            k if DECISION_UAST_KINDS.contains(&k) => *count += 1,
            "BinaryExpr" => {
                if let Some(AttributeValue::Str(op)) = node.attributes.get("operator") {
                    if matches!(op.as_str(), "and" | "or" | "&&" | "||") {
                        *count += 1;
                    }
                }
            }
            _ => {}
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

    #[test]
    fn python_match_arms_count_toward_complexity() {
        // Each case arm is a decision: a 3-arm match => 2 decisions + base 1
        // = complexity 3, consistent with cfg.cyclomatic (#153 follow-up).
        // Intentionally diverges from 0.3.11 (allowlisted in parity_check.py).
        let source = "def f(x):\n    match x:\n        case 1:\n            y = 1\n        case 2:\n            y = 2\n        case _:\n            y = 3\n    return y\n";
        let result = parse_source(source, "python", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 3);
    }

    #[test]
    fn go_switch_arms_count_toward_complexity() {
        let source = "package p\nfunc f(x int) int {\n\tvar y int\n\tswitch {\n\tcase x > 2:\n\t\ty = 1\n\tcase x > 1:\n\t\ty = 2\n\tdefault:\n\t\ty = 3\n\t}\n\treturn y\n}\n";
        let result = parse_source(source, "go", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 3);
    }
}

// ---------------------------------------------------------------------------
// Per-function complexity *entries* — name/span-aware, still UAST-only
// ---------------------------------------------------------------------------
//
// `calculate_function_complexities` above answers "what's the worst
// complexity" (keyed by opaque node id, fine for a gate check).
// `calculate_function_complexity_entries` answers "which function, at
// which lines" for agent-facing reporting — it needs a real name and a
// span. Both requirements are satisfiable straight from the UAST: UAST
// spans already carry real line numbers, and a `FunctionDecl`'s name is
// its first `Identifier`-kind child (the mappers preserve that child;
// they just don't duplicate its text into an attribute) — so this reuses
// `node_complexity` above rather than re-deriving complexity via a
// second, tree-sitter-native pass. Genuinely multi-language, same as
// `calculate_function_complexities`.

const SCOPE_UAST_KINDS: &[&str] = &["FunctionDecl", "MethodDecl", "TypeDecl"];

/// One function/method/closure's complexity, name, span, and scope kind.
pub struct FunctionComplexityEntry {
    pub name: String,
    pub qualified_name: String,
    pub kind: &'static str,
    pub start_line: usize,
    pub end_line: usize,
    pub complexity: usize,
}

/// A UAST node's own name, if its first child is an `Identifier` (the
/// mappers place the declared name there for `FunctionDecl` /
/// `MethodDecl` / `TypeDecl`). Sliced from `source` by the child's span,
/// since UAST nodes don't carry token text themselves.
fn uast_node_name(node: &UASTNode, source: &str) -> Option<String> {
    let ident = node.children.first()?;
    if ident.kind != "Identifier" {
        return None;
    }
    source
        .get(ident.span.start_byte..ident.span.end_byte)
        .map(|s| s.to_string())
}

fn classify_kind(node: &UASTNode, source: &str, chain: &[(String, String)]) -> &'static str {
    if let Some((enclosing_kind, _)) = chain.last() {
        if enclosing_kind == "TypeDecl" {
            return "method";
        }
        if enclosing_kind == "FunctionDecl" || enclosing_kind == "MethodDecl" {
            return "closure";
        }
    }
    if node.kind == "MethodDecl" {
        return "method";
    }
    if is_async(node, source) {
        "async_function"
    } else {
        "function"
    }
}

/// Best-effort `async` detection: the mappers only keep *named* tree-sitter
/// children (see `mapper_common::filtered_named_children`), and `async` is
/// an anonymous keyword token in tree-sitter-python's grammar — the node
/// kind stays `function_definition` either way, but its *span* still
/// starts at `async` (tree-sitter includes leading anonymous tokens in the
/// parent's span), so it never survives as a UAST child. Checking whether
/// the node's own span starts with the `async` keyword recovers it without
/// touching the shared mapper.
fn is_async(node: &UASTNode, source: &str) -> bool {
    source.get(node.span.start_byte..).is_some_and(|text| {
        text.strip_prefix("async")
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
    })
}

fn collect_entries(
    node: &UASTNode,
    source: &str,
    chain: &mut Vec<(String, String)>,
    entries: &mut Vec<FunctionComplexityEntry>,
) {
    let is_function = matches!(node.kind.as_str(), "FunctionDecl" | "MethodDecl");
    let is_scope = SCOPE_UAST_KINDS.contains(&node.kind.as_str());

    if is_function {
        if let Some(name) = uast_node_name(node, source) {
            let mut qualified_parts: Vec<&str> = chain.iter().map(|(_, n)| n.as_str()).collect();
            qualified_parts.push(&name);
            entries.push(FunctionComplexityEntry {
                name: name.clone(),
                qualified_name: qualified_parts.join("."),
                kind: classify_kind(node, source, chain),
                start_line: node.span.start_line,
                end_line: node.span.end_line,
                complexity: node_complexity(node),
            });
        }
    }

    let pushed = if is_scope {
        uast_node_name(node, source).map(|name| {
            chain.push((node.kind.clone(), name));
        })
    } else {
        None
    };

    for child in &node.children {
        collect_entries(child, source, chain, entries);
    }

    if pushed.is_some() {
        chain.pop();
    }
}

/// Per-function complexity with locations, parallel to the gate metric.
///
/// Same decision-node counting as [`calculate_function_complexities`],
/// but keyed by real (dotted, qualified) names with spans. `source` is
/// needed to slice out identifier text (see [`uast_node_name`]).
pub fn calculate_function_complexity_entries(
    uast_root: &UASTNode,
    source: &str,
) -> Vec<FunctionComplexityEntry> {
    let mut entries = Vec::new();
    let mut chain = Vec::new();
    collect_entries(uast_root, source, &mut chain, &mut entries);
    entries
}

#[cfg(test)]
mod entries_tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    fn entries(source: &str, language: &str) -> Vec<FunctionComplexityEntry> {
        let result = parse_source(source, language, None).expect("parse should not fail");
        calculate_function_complexity_entries(&result.uast_root, source)
    }

    #[test]
    fn top_level_function_kind_and_span() {
        let source = "def foo(x):\n    if x:\n        return 1\n    return 0\n";
        let es = entries(source, "python");
        assert_eq!(es.len(), 1);
        let foo = &es[0];
        assert_eq!(foo.name, "foo");
        assert_eq!(foo.qualified_name, "foo");
        assert_eq!(foo.kind, "function");
        assert_eq!(foo.start_line, 1);
        assert_eq!(foo.complexity, 2);
    }

    #[test]
    fn method_inside_class_is_qualified_and_kind_method() {
        let source = "class C:\n    def m(self):\n        return 1\n";
        let es = entries(source, "python");
        let m = es.iter().find(|e| e.name == "m").unwrap();
        assert_eq!(m.qualified_name, "C.m");
        assert_eq!(m.kind, "method");
    }

    #[test]
    fn nested_closure_is_dotted_and_outer_includes_nested_count() {
        let source = "def outer():\n    def inner():\n        if True:\n            return 1\n    return inner\n";
        let es = entries(source, "python");
        let inner = es.iter().find(|e| e.name == "inner").unwrap();
        let outer = es.iter().find(|e| e.name == "outer").unwrap();
        assert_eq!(inner.qualified_name, "outer.inner");
        assert_eq!(inner.kind, "closure");
        assert!(outer.complexity >= inner.complexity);
    }

    #[test]
    fn module_level_only_has_no_entries() {
        assert!(entries("x = 1\n", "python").is_empty());
    }

    #[test]
    fn async_function_kind_is_detected() {
        let es = entries("async def bar():\n    return 1\n", "python");
        assert_eq!(es[0].kind, "async_function");
    }

    #[test]
    fn works_across_languages_unlike_the_python_original() {
        let source = "fn f(x: i32) -> i32 {\n    if x > 0 {\n        return x;\n    }\n    0\n}\n";
        let es = entries(source, "rust");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].name, "f");
        assert_eq!(es[0].complexity, 2);
    }
}
