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

fn collect_scopes<'a>(
    node: &'a UASTNode,
    source: &str,
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

    for child in &node.children {
        collect_scopes(child, source, chain, scopes);
    }

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
    collect_scopes(uast_root, source, &mut chain, &mut scopes);
    scopes
}
