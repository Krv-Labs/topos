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

const DECISION_UAST_KINDS: &[&str] = &[
    "IfStmt",
    "ForStmt",
    "WhileStmt",
    "ConditionalExpr",
    "WithStmt",
    "AssertStmt",
];

/// Count implicit `for` and filter `if` clauses inside a Python comprehension.
fn comprehension_decision_points(node: &UASTNode) -> usize {
    let mut points = 0;
    fn walk(node: &UASTNode, count: &mut usize) {
        match node.native.node_kind.as_str() {
            "for_in_clause" | "for_clause" | "if_clause" => *count += 1,
            _ => {}
        }
        for child in &node.children {
            walk(child, count);
        }
    }
    walk(node, &mut points);
    points
}

/// Cyclomatic complexity of one callable's subtree: each decision node
/// (`IfStmt`/`ForStmt`/`WhileStmt`/`ConditionalExpr`/…) adds one, a
/// `MatchStmt` adds one per case arm beyond the first (a k-way switch is
/// k-1 decisions), `TryStmt` adds one plus one per handler beyond the
/// first (mirroring old Python per-`except_clause` counting), a
/// `Comprehension` adds one per implicit `for` and filter `if`, and a
/// short-circuit `BinaryExpr` (`and`/`or`/`&&`/`||`) adds one.
///
/// Note: counting per-arm here intentionally diverges from the last Python
/// release (`topos-mcp==0.3.11`), which counted a whole match/switch as a
/// single decision. The divergence is documented in the `[0.4.0]` CHANGELOG
/// entry (the parity/benchmark harness that originally allowlisted it was a
/// migration-verification artifact and has since been removed).
fn node_complexity(node: &UASTNode) -> usize {
    fn walk(node: &UASTNode, count: &mut usize) {
        match node.kind.as_str() {
            // A k-way switch/match contributes k branches (k - 1 decisions),
            // counted from its arms so this agrees with the CFG builder.
            "MatchStmt" => {
                *count += crate::graphs::cfg::builder::match_arm_count(node).saturating_sub(1);
            }
            // Try body + one decision per handler; first handler is covered by
            // the base +1, extras match old Python per-`except_clause` tally.
            "TryStmt" => {
                let handlers = node
                    .children
                    .iter()
                    .filter(|c| c.kind == "CatchClause")
                    .count();
                *count += 1 + handlers.saturating_sub(1);
            }
            "Comprehension" => {
                *count += comprehension_decision_points(node);
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

    #[test]
    fn boolean_chain_breaks_simple_gate() {
        let source = "def f(a, b, c, d, e, f, g, h, i, j, k):\n    return a and b and c and d and e and f and g and h and i and j and k\n";
        let result = parse_source(source, "python", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 11);
    }

    #[test]
    fn boolean_chain_three_term_rust() {
        let source = "fn f(a: bool, b: bool, c: bool) -> bool {\n    a && b && c\n}\n";
        let result = parse_source(source, "rust", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 3);
    }

    #[test]
    fn boolean_chain_three_term_go() {
        let source = "package p\nfunc f(a, b, c bool) bool {\n\treturn a && b && c\n}\n";
        let result = parse_source(source, "go", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 3);
    }

    #[test]
    fn javascript_ternary_increases_complexity() {
        let source = "function f(x) { return x ? 1 : 0; }\n";
        let result = parse_source(source, "javascript", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 2);
    }

    #[test]
    fn filtered_comprehension_increases_complexity() {
        let source = "def f(items):\n    return [x for x in items if x > 0]\n";
        let result = parse_source(source, "python", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 3);
    }

    #[test]
    fn with_statement_increases_complexity() {
        let source = "def f():\n    with open('x') as fh:\n        return fh.read()\n";
        let result = parse_source(source, "python", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 2);
    }

    #[test]
    fn assert_statement_increases_complexity() {
        let source = "def f(x):\n    assert x\n    return x\n";
        let result = parse_source(source, "python", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 2);
    }

    #[test]
    fn python_multi_except_counts_per_handler() {
        let source = "def f():\n    try:\n        pass\n    except ValueError:\n        pass\n    except TypeError:\n        pass\n    except KeyError:\n        pass\n";
        let result = parse_source(source, "python", None).unwrap();
        assert_eq!(calculate_max_function_complexity(&result.uast_root), 4);
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

/// Label stem for callables the grammar gives no name at all; suffixed
/// with `@<line>` so the entry still points at editable source.
const ANONYMOUS_NAME: &str = "<anonymous>";

/// One function/method/closure's complexity, name, span, and scope kind.
pub struct FunctionComplexityEntry {
    pub name: String,
    pub qualified_name: String,
    pub kind: &'static str,
    pub start_line: usize,
    pub end_line: usize,
    pub complexity: usize,
}

/// The direct child holding a declaration's name, if there is one.
///
/// The mappers put the declared name in an `Identifier` child of
/// `FunctionDecl` / `MethodDecl` / `TypeDecl`, but *not* necessarily the
/// first one: Rust leads with `visibility_modifier` and
/// `function_modifiers` (`pub`, `async`, `unsafe`, …) and Go's
/// `MethodDecl` leads with the receiver `parameter_list`. Insisting on
/// `children.first()` therefore made every `pub fn` look anonymous —
/// which silently dropped it from the location path while it still
/// counted toward the `ast.max_function_complexity` gate. Scanning for
/// the first `Identifier` child instead is language-neutral and correct
/// for every grammar dumped so far, because modifiers and receivers are
/// never mapped to `Identifier`.
///
/// JavaScript is the one exception: a `method_definition`'s name is a
/// `property_identifier`, which has no UAST kind of its own, so that
/// native kind is accepted too rather than leaving `C.m` anonymous.
fn uast_name_node(node: &UASTNode) -> Option<&UASTNode> {
    node.children
        .iter()
        .find(|child| child.kind == "Identifier" || child.native.node_kind == "property_identifier")
}

/// A UAST node's own declared name. Sliced from `source` by the name
/// child's span (see [`uast_name_node`]), since UAST nodes don't carry
/// token text themselves.
fn uast_node_name(node: &UASTNode, source: &str) -> Option<String> {
    let ident = uast_name_node(node)?;
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
/// parent's span), so it never survives as a UAST child. Recovering it from
/// the source text avoids touching the shared mapper.
///
/// The keyword is not always first, though: `pub async fn` starts the span
/// at `pub`. So the check is over the whole *header* — everything from the
/// declaration's start up to its name (or, for an anonymous callable, up to
/// its first mapped child), which is exactly the run of modifier keywords.
/// Matching whole tokens keeps identifiers like `async_run` from counting.
fn is_async(node: &UASTNode, source: &str) -> bool {
    let start = node.span.start_byte;
    let end = uast_name_node(node)
        .or_else(|| node.children.first())
        .map_or(node.span.end_byte, |child| child.span.start_byte);
    source.get(start..end.max(start)).is_some_and(|header| {
        header
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|token| token == "async")
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

    // Every callable that `calculate_function_complexities` counts must get
    // an entry, or the `ast.max_function_complexity` gate can fail with no
    // location to point an agent at — an un-targetable, and therefore
    // un-fixable, gate. Genuinely anonymous callables (JS function
    // expressions, arrow functions taking a parameter list) get a synthetic
    // label instead of being skipped; the line number keeps it stable and
    // editable, and mirrors the `<module>` marker convention used on the
    // MCP side for gates that are not attributable to a function at all.
    let name = is_function.then(|| {
        uast_node_name(node, source)
            .unwrap_or_else(|| format!("{ANONYMOUS_NAME}@{}", node.span.start_line))
    });

    if let Some(name) = &name {
        let mut qualified_parts: Vec<&str> = chain.iter().map(|(_, n)| n.as_str()).collect();
        qualified_parts.push(name);
        entries.push(FunctionComplexityEntry {
            name: name.clone(),
            qualified_name: qualified_parts.join("."),
            kind: classify_kind(node, source, chain),
            start_line: node.span.start_line,
            end_line: node.span.end_line,
            complexity: node_complexity(node),
        });
    }

    let pushed = if is_scope {
        // Reuse the callable's label (synthetic included) so a named function
        // nested inside an anonymous one still gets a qualified name.
        name.or_else(|| uast_node_name(node, source)).map(|label| {
            chain.push((node.kind.clone(), label));
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

    #[test]
    fn visibility_modifier_does_not_hide_the_name() {
        // `pub` maps to a leading `visibility_modifier` child, so a
        // `children.first()`-only name lookup made every `pub fn` in the
        // workspace anonymous — and therefore invisible to the location
        // path, even while it counted toward the gate.
        let source =
            "pub fn wide(x: i32) -> i32 {\n    if x > 0 {\n        return x;\n    }\n    0\n}\n";
        let es = entries(source, "rust");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].name, "wide");
        assert_eq!(es[0].kind, "function");
        assert_eq!(es[0].start_line, 1);
        assert_eq!(es[0].end_line, 6);
    }

    #[test]
    fn async_is_detected_behind_a_visibility_modifier() {
        // The span starts at `pub`, not `async`, so the keyword has to be
        // found across the whole header rather than as a prefix.
        let source = "pub async fn fetch(x: i32) -> i32 {\n    if x > 0 {\n        return x;\n    }\n    0\n}\n";
        let es = entries(source, "rust");
        assert_eq!(es[0].name, "fetch");
        assert_eq!(es[0].kind, "async_function");
    }

    #[test]
    fn async_lookalike_identifier_is_not_async() {
        let es = entries("fn async_run(x: i32) -> i32 {\n    x\n}\n", "rust");
        assert_eq!(es[0].name, "async_run");
        assert_eq!(es[0].kind, "function");
    }

    #[test]
    fn go_method_receiver_does_not_hide_the_name() {
        // Go's `MethodDecl` leads with the receiver `parameter_list`.
        let source =
            "package p\nfunc (r *T) M(x int) int {\n\tif x > 0 {\n\t\treturn x\n\t}\n\treturn 0\n}\n";
        let es = entries(source, "go");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].name, "M");
        assert_eq!(es[0].kind, "method");
    }

    #[test]
    fn javascript_method_name_comes_from_property_identifier() {
        let source = "class C {\n  m(x) {\n    return x ? 1 : 0;\n  }\n}\n";
        let es = entries(source, "javascript");
        let m = es.iter().find(|e| e.name == "m").expect("method entry");
        assert_eq!(m.qualified_name, "C.m");
        assert_eq!(m.kind, "method");
    }

    #[test]
    fn anonymous_callable_gets_a_located_synthetic_name() {
        let source = "[1, 2].map(function (x) {\n  return x ? 1 : 0;\n});\n";
        let es = entries(source, "javascript");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].name, "<anonymous>@1");
        assert_eq!(es[0].qualified_name, "<anonymous>@1");
        assert_eq!(es[0].start_line, 1);
        assert_eq!(es[0].end_line, 3);
        assert_eq!(es[0].complexity, 2);
    }

    #[test]
    fn named_function_inside_an_anonymous_one_is_still_qualified() {
        let source = "run(function () {\n  function inner(x) {\n    return x ? 1 : 0;\n  }\n});\n";
        let es = entries(source, "javascript");
        let inner = es.iter().find(|e| e.name == "inner").expect("inner entry");
        assert_eq!(inner.qualified_name, "<anonymous>@1.inner");
        assert_eq!(inner.kind, "closure");
    }

    /// The regression that matters: `calculate_max_function_complexity`
    /// feeds the SIMPLE `ast.max_function_complexity` gate while these
    /// entries feed `metric_locations` -> `refactor_targets` ->
    /// `binding_constraint`. If the two paths disagree, a real gate failure
    /// has no location, never becomes a refactor target, and can never be
    /// picked as the binding constraint — an un-fixable gate. So every node
    /// the gate counts must produce exactly one entry.
    #[test]
    fn every_gate_counted_function_has_exactly_one_entry() {
        let cases: &[(&str, &str)] = &[
            (
                "rust",
                "pub fn wide(x: i32) -> i32 {\n    if x > 0 && x < 9 {\n        return x;\n    }\n    if x == 0 {\n        return 1;\n    }\n    0\n}\n",
            ),
            (
                "rust",
                "struct S;\nimpl S {\n    pub async fn m(&self, x: i32) -> i32 {\n        if x > 0 {\n            x\n        } else {\n            0\n        }\n    }\n}\n",
            ),
            (
                "javascript",
                "[1, 2].map(function (x) {\n  return x ? 1 : 0;\n});\nconst g = (a, b) => (a ? b : 0);\nclass C {\n  m(x) {\n    return x ? 1 : 0;\n  }\n}\n",
            ),
            (
                "go",
                "package p\nfunc (r *T) M(x int) int {\n\tif x > 0 {\n\t\treturn x\n\t}\n\treturn 0\n}\n",
            ),
            (
                "python",
                "class C:\n    async def m(self, x):\n        def inner(y):\n            return 1 if y else 0\n\n        return inner(x)\n",
            ),
            ("python", "x = 1\n"),
        ];

        for (language, source) in cases {
            let result = parse_source(source, language, None).expect("parse should not fail");
            let es = calculate_function_complexity_entries(&result.uast_root, source);
            assert_eq!(
                es.len(),
                calculate_function_complexities(&result.uast_root).len(),
                "entry count must match the gate's function count for {language}: {source:?}"
            );
            assert_eq!(
                es.iter().map(|e| e.complexity).max().unwrap_or(0),
                calculate_max_function_complexity(&result.uast_root),
                "worst entry must match the gate metric for {language}: {source:?}"
            );
        }
    }
}
