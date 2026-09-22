//! Callable enumeration over the UAST — names, kinds, and spans.
//!
//! Every per-function probe needs the same three things before it can say
//! anything useful: which nodes are callables, what to call them, and
//! where they live in the source. That walk is non-trivial (see
//! [`uast_name_node`] on why `children.first()` is wrong, and [`is_async`]
//! on why the keyword has to be recovered from source text), and it is
//! shared by two pillars now — `ast.max_function_complexity` for SIMPLE
//! and `nav.max_function_divergence` for NAVIGABLE.
//!
//! Sharing it is not just DRY. A gate metric and its location path
//! disagreeing produces a gate that can fail with nothing to point an
//! agent at — an un-fixable gate. One walk, one set of callables, so that
//! divergence is unrepresentable per pillar.

use crate::graphs::uast::models::UASTNode;

/// UAST kinds that open a *naming* scope for qualified names.
pub(crate) const SCOPE_UAST_KINDS: &[&str] = &["FunctionDecl", "MethodDecl", "TypeDecl"];

/// Label stem for callables the grammar gives no name at all; suffixed
/// with `@<line>` so the entry still points at editable source.
pub(crate) const ANONYMOUS_NAME: &str = "<anonymous>";

/// One callable's identity and location, plus the UAST node itself so a
/// probe can compute whatever metric it cares about over the subtree.
pub(crate) struct FunctionScope<'a> {
    pub name: String,
    pub qualified_name: String,
    pub kind: &'static str,
    pub start_line: usize,
    pub end_line: usize,
    pub node: &'a UASTNode,
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
/// Arrow functions are the second exception, in the other direction: the
/// grammar gives `arrow_function` no name field at all, so its direct
/// `Identifier` children are the *parameter* (`x => …`) or the expression
/// *body* (`() => e`). Accepting those would name the callable `x` or `e`
/// and — worse — mask the enclosing-node inference below, which only runs
/// when a callable has no own name. So arrows are declared nameless here.
///
/// TypeScript's `class_declaration` names itself with a `type_identifier`
/// (no UAST kind of its own), which left `C` unnamed and every class-field
/// arrow un-qualified. That native kind is accepted for these three
/// declaration forms only — never for a callable, so it can't be mistaken
/// for a function's name or shift [`is_async`]'s header window. Rust's
/// `impl_item` / `trait_item` also map to `TypeDecl` and name themselves
/// with a `type_identifier`, but widening to every `TypeDecl` would
/// re-qualify every Rust method (`m` / `function` -> `T.m` / `method`)
/// across the whole workspace — a real improvement, but not this change's
/// job, so it stays out.
const TYPE_NAME_DECL_KINDS: &[&str] = &[
    "class_declaration",
    "abstract_class_declaration",
    "interface_declaration",
];

fn uast_name_node(node: &UASTNode) -> Option<&UASTNode> {
    if node.native.node_kind == "arrow_function" {
        return None;
    }
    let accept_type_identifier =
        node.kind == "TypeDecl" && TYPE_NAME_DECL_KINDS.contains(&node.native.node_kind.as_str());
    node.children.iter().find(|child| {
        child.kind == "Identifier"
            || child.native.node_kind == "property_identifier"
            || (accept_type_identifier && child.native.node_kind == "type_identifier")
    })
}

/// Enclosing nodes that bind a callable to a name without being the
/// binding itself — `React.memo(() => …)`, `forwardRef(…)`,
/// `useCallback(() => …, [])`. Walking through at most
/// [`MAX_WRAPPER_DEPTH`] of them lets the wrapped arrow inherit the name of
/// the `variable_declarator` (or `pair`, …) that the *call* is bound to.
const NAME_WRAPPER_KINDS: &[&str] = &["arguments", "call_expression", "parenthesized_expression"];

/// Enough for `const F = React.memo(forwardRef(() => …))`; bounded so a
/// deeply curried expression can't walk the whole file looking for a name.
const MAX_WRAPPER_DEPTH: usize = 3;

/// Text of a node, sliced from `source` by its span.
fn node_text<'s>(node: &UASTNode, source: &'s str) -> Option<&'s str> {
    source.get(node.span.start_byte..node.span.end_byte)
}

/// The first child of `parent` matching `accept` that is positioned
/// *before* `node` — the binding-name side of `name = callable`, never
/// something from inside the callable itself.
fn child_before<'a>(
    parent: &'a UASTNode,
    node: &UASTNode,
    accept: impl Fn(&UASTNode) -> bool,
) -> Option<&'a UASTNode> {
    parent
        .children
        .iter()
        .find(|child| child.span.end_byte <= node.span.start_byte && accept(child))
}

fn is_identifier_like(child: &UASTNode) -> bool {
    child.kind == "Identifier" || child.native.node_kind == "property_identifier"
}

/// A callable's name taken from the node that *binds* it.
///
/// `const PollShell = () => …`, `{ onClick: () => … }`, `handle = () => …`
/// in a class body and `module.exports.handler = async () => …` are all
/// named in the source — just not on the `FunctionDecl` node, because the
/// grammar hangs the name off the enclosing declarator/pair/assignment.
/// Falling straight through to `<anonymous>@<line>` made most of a modern
/// React or Node file unnameable, and therefore un-targetable by any gate
/// that reports a location. This only ever runs when the callable has no
/// name of its own, so a real identifier is never overridden.
///
/// `ancestors` is the enclosing chain, root-first.
fn inferred_name(node: &UASTNode, ancestors: &[&UASTNode], source: &str) -> Option<String> {
    let mut index = ancestors.len();
    for _ in 0..=MAX_WRAPPER_DEPTH {
        let parent = *ancestors.get(index.checked_sub(1)?)?;
        if NAME_WRAPPER_KINDS.contains(&parent.native.node_kind.as_str()) {
            index -= 1;
            continue;
        }
        return name_from_binder(parent, node, source);
    }
    None
}

fn name_from_binder(parent: &UASTNode, node: &UASTNode, source: &str) -> Option<String> {
    match parent.native.node_kind.as_str() {
        // `const f = () => …`, `let f = function () {}`; `lexical_declaration`
        // covers the shape where the declarator layer is absent.
        "variable_declarator" | "lexical_declaration" | "variable_declaration" => {
            let ident = child_before(parent, node, |child| child.kind == "Identifier")?;
            node_text(ident, source).map(str::to_string)
        }
        // Object literal `{ onClick: () => … }` — the key is the name.
        "pair" => {
            let key = child_before(parent, node, |child| {
                is_identifier_like(child) || child.native.node_kind == "string"
            })?;
            let text = node_text(key, source)?;
            Some(
                text.trim_matches(|c| c == '\'' || c == '"' || c == '`')
                    .to_string(),
            )
        }
        // Class property arrow functions. `classify_kind` still reads the
        // chain, so these stay `method` inside a `TypeDecl`.
        "public_field_definition" | "property_definition" | "field_definition" => {
            let ident = child_before(parent, node, is_identifier_like)?;
            node_text(ident, source).map(str::to_string)
        }
        // `x.handler = () => …` — the whole left-hand side, so a member
        // expression keeps the qualifier that makes it findable.
        "assignment_expression" | "assignment" => assigned_name(parent, node, source),
        _ => (parent.kind == "AssignExpr")
            .then(|| assigned_name(parent, node, source))
            .flatten(),
    }
}

/// Longest name kept for an assignment target; past this the text is more
/// likely a destructuring pattern than a useful label.
const MAX_ASSIGNED_NAME_LEN: usize = 60;

fn assigned_name(parent: &UASTNode, node: &UASTNode, source: &str) -> Option<String> {
    let target = child_before(parent, node, |_| true)?;
    let text = node_text(target, source)?.trim();
    (!text.is_empty() && text.len() <= MAX_ASSIGNED_NAME_LEN).then(|| text.to_string())
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

fn collect_scopes<'a>(
    node: &'a UASTNode,
    source: &str,
    ancestors: &mut Vec<&'a UASTNode>,
    chain: &mut Vec<(String, String)>,
    scopes: &mut Vec<FunctionScope<'a>>,
) {
    let is_function = matches!(node.kind.as_str(), "FunctionDecl" | "MethodDecl");
    let is_scope = SCOPE_UAST_KINDS.contains(&node.kind.as_str());

    // Every callable a per-function gate counts must get an entry, or that
    // gate can fail with no location to point an agent at — an
    // un-targetable, and therefore un-fixable, gate. Genuinely anonymous
    // callables (JS function expressions, arrow functions taking a
    // parameter list) get a synthetic label instead of being skipped; the
    // line number keeps it stable and editable, and mirrors the `<module>`
    // marker convention used on the MCP side for gates that are not
    // attributable to a function at all.
    let name = is_function.then(|| {
        uast_node_name(node, source)
            .or_else(|| inferred_name(node, ancestors, source))
            .unwrap_or_else(|| format!("{ANONYMOUS_NAME}@{}", node.span.start_line))
    });

    if let Some(name) = &name {
        let mut qualified_parts: Vec<&str> = chain.iter().map(|(_, n)| n.as_str()).collect();
        qualified_parts.push(name);
        scopes.push(FunctionScope {
            name: name.clone(),
            qualified_name: qualified_parts.join("."),
            kind: classify_kind(node, source, chain),
            start_line: node.span.start_line,
            end_line: node.span.end_line,
            node,
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

    ancestors.push(node);
    for child in &node.children {
        collect_scopes(child, source, ancestors, chain, scopes);
    }
    ancestors.pop();

    if pushed.is_some() {
        chain.pop();
    }
}

/// Every callable in the tree, with a dotted qualified name and a span.
///
/// `source` is needed to slice out identifier text (see
/// [`uast_node_name`]) — UAST nodes don't carry token text themselves.
pub(crate) fn function_scopes<'a>(uast_root: &'a UASTNode, source: &str) -> Vec<FunctionScope<'a>> {
    let mut scopes = Vec::new();
    let mut chain = Vec::new();
    let mut ancestors = Vec::new();
    collect_scopes(uast_root, source, &mut ancestors, &mut chain, &mut scopes);
    scopes
}

#[cfg(test)]
mod name_inference_tests {
    use super::{function_scopes, FunctionScope};
    use crate::graphs::ast::dispatch::parse_source;
    use crate::graphs::uast::models::UASTNode;

    struct Parsed {
        root: UASTNode,
        source: String,
    }

    fn parse(source: &str, language: &str, file: &str) -> Parsed {
        let result = parse_source(source, language, Some(file)).expect("parse should not fail");
        Parsed {
            root: result.uast_root,
            source: source.to_string(),
        }
    }

    fn scopes<'a>(parsed: &'a Parsed) -> Vec<FunctionScope<'a>> {
        function_scopes(&parsed.root, &parsed.source)
    }

    fn named<'a, 's>(scopes: &'a [FunctionScope<'s>], name: &str) -> &'a FunctionScope<'s> {
        scopes
            .iter()
            .find(|scope| scope.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "no scope named {name:?}; got {:?}",
                    scopes.iter().map(|s| &s.name).collect::<Vec<_>>()
                )
            })
    }

    #[test]
    fn exported_arrow_component_takes_the_declarator_name() {
        let parsed = parse(
            "export const PollShell = ({a}: {a: number}) => { return a ? 1 : 0 }\n",
            "typescript",
            "PollShell.tsx",
        );
        let scopes = scopes(&parsed);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].name, "PollShell");
        assert_eq!(scopes[0].qualified_name, "PollShell");
        assert_eq!(scopes[0].kind, "function");
    }

    #[test]
    fn object_literal_values_take_their_key() {
        let parsed = parse(
            "const obj = { onClick: () => 1, run: function () { return 2 } }\n",
            "typescript",
            "obj.ts",
        );
        let scopes = scopes(&parsed);
        assert_eq!(scopes.len(), 2);
        assert_eq!(named(&scopes, "onClick").kind, "function");
        assert_eq!(named(&scopes, "run").kind, "function");
    }

    #[test]
    fn class_property_arrow_is_a_qualified_method() {
        let parsed = parse("class C { handle = () => 1 }\n", "typescript", "c.ts");
        let scopes = scopes(&parsed);
        let handle = named(&scopes, "handle");
        assert_eq!(handle.qualified_name, "C.handle");
        assert_eq!(handle.kind, "method");
    }

    #[test]
    fn wrapped_arrow_takes_the_name_the_call_is_bound_to() {
        let parsed = parse("const Memo = React.memo(() => 1)\n", "typescript", "m.tsx");
        let scopes = scopes(&parsed);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].name, "Memo");
    }

    #[test]
    fn inferred_names_still_chain_into_qualified_names() {
        let parsed = parse(
            "function named() {}\nconst x = () => { const inner = () => 2; return inner() }\n",
            "typescript",
            "chain.ts",
        );
        let scopes = scopes(&parsed);
        assert_eq!(named(&scopes, "named").qualified_name, "named");
        assert_eq!(named(&scopes, "x").qualified_name, "x");
        let inner = named(&scopes, "inner");
        assert_eq!(inner.qualified_name, "x.inner");
        assert_eq!(inner.kind, "closure");
    }

    #[test]
    fn assignment_target_names_an_async_arrow() {
        let parsed = parse(
            "module.exports.handler = async (e) => e\n",
            "javascript",
            "handler.js",
        );
        let scopes = scopes(&parsed);
        assert_eq!(scopes.len(), 1);
        assert!(
            scopes[0].name.contains("handler"),
            "expected handler in {:?}",
            scopes[0].name
        );
        assert_eq!(scopes[0].kind, "async_function");
    }

    #[test]
    fn an_unbound_callback_argument_stays_anonymous() {
        // Nothing binds this arrow to a name, so the synthetic label is
        // still the only thing that can point a gate at the source.
        let parsed = parse("items.map((x) => x * 2)\n", "typescript", "map.ts");
        let scopes = scopes(&parsed);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].name, "<anonymous>@1");
        assert_eq!(scopes[0].qualified_name, "<anonymous>@1");
    }
}
