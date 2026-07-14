//! C++ -> UAST mapper.

use std::collections::HashMap;

use tree_sitter::Node;

use super::mapper_common::map_tree_sitter_to_uast;
use super::models::{AttributeValue, UASTNode};

/// Real tree-sitter-cpp grammar node names (verified against the
/// vendored grammar directly -- the previous table here was copy-pasted
/// from the Python/Rust mappers and used node names tree-sitter-cpp
/// doesn't emit at all, e.g. `class_definition`/`struct_item`, so every
/// C++ type declaration silently fell to `Unknown`; see issue #158).
fn declaration_kind(kind: &str) -> Option<&'static str> {
    match kind {
        // Free functions AND in-class method *definitions* (with a
        // body) -- tree-sitter-cpp doesn't distinguish the two at this
        // node.
        "function_definition" => Some("FunctionDecl"),
        "class_specifier" => Some("TypeDecl"),
        "struct_specifier" => Some("TypeDecl"),
        "enum_specifier" => Some("TypeDecl"),
        "union_specifier" => Some("TypeDecl"),
        _ => None,
    }
}

/// `declaration` (free-standing) and `field_declaration` (inside a
/// class body) are dual-purpose in the grammar: a var/member
/// declaration (`int x;`) and a declaration-only fn/method signature
/// with no body (`void f(int);`, or a pure-virtual
/// `virtual double area() const = 0;`) both use the same wrapper node,
/// distinguished only by whether a `function_declarator` child is
/// present.
fn is_declaration_only_kind(kind: &str) -> bool {
    matches!(kind, "declaration" | "field_declaration")
}

/// True for real fn/method declarations, false for fn pointers.
///
/// tree-sitter-cpp represents both `void f(int);` and
/// `void (*handler)(int);` with a `function_declarator` wrapper. A real
/// prototype has the fn name directly under that wrapper; a fn pointer
/// nests the identifier under `pointer_declarator` /
/// `parenthesized_declarator` and should remain a `VarDecl`.
fn is_named_function_declarator(node: &Node) -> bool {
    if node.kind() != "function_declarator" {
        return false;
    }
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|child| matches!(child.kind(), "identifier" | "field_identifier"));
    found
}

fn has_function_declarator(node: &Node) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|child| is_named_function_declarator(&child));
    found
}

const STATEMENT_TYPES: &[(&str, &str)] = &[
    ("if_statement", "IfStmt"),
    ("for_statement", "ForStmt"),
    ("while_statement", "WhileStmt"),
    ("return_statement", "ReturnStmt"),
    ("break_statement", "BreakStmt"),
    ("continue_statement", "ContinueStmt"),
    ("throw_statement", "ThrowStmt"),
    ("try_statement", "TryStmt"),
    ("expression_statement", "ExprStmt"),
];

const EXPRESSION_TYPES: &[(&str, &str)] = &[
    ("assignment_expression", "AssignExpr"),
    ("binary_expression", "BinaryExpr"),
    ("unary_expression", "UnaryExpr"),
    ("call_expression", "CallExpr"),
    ("field_expression", "MemberExpr"),
    ("subscript_expression", "MemberExpr"),
];

pub fn map_node_kind(kind: &str) -> &'static str {
    if let Some(mapped) = declaration_kind(kind) {
        return mapped;
    }
    if is_declaration_only_kind(kind) {
        // ponytail: `map_tree_sitter_to_uast`'s `map_node_kind` callback
        // only sees the kind string, not the `Node` itself, so this
        // can't inspect children to distinguish a declaration-only
        // function prototype (`void f(int);`) from a plain var/member
        // declaration the way Python's `_has_function_declarator` check
        // does -- always mapping to VarDecl undercounts FunctionDecl for
        // header-only prototypes and pure-virtual method signatures.
        // Upgrade path: widen `map_node_kind`'s signature to take
        // `&Node` across every mapper if this undercounting turns out to
        // matter for a real corpus (abstractness detection itself does
        // not depend on this -- see `has_pure_virtual_method` below,
        // which inspects the raw `Node` directly).
        return "VarDecl";
    }
    if let Some((_, mapped)) = STATEMENT_TYPES.iter().find(|(k, _)| *k == kind) {
        return mapped;
    }
    if let Some((_, mapped)) = EXPRESSION_TYPES.iter().find(|(k, _)| *k == kind) {
        return mapped;
    }
    match kind {
        "identifier" => "Identifier",
        "translation_unit" => "File",
        s if s.ends_with("literal") || matches!(s, "string" | "integer" | "float") => "Literal",
        _ => "Unknown",
    }
}

/// Martin Abstractness classification (issue #124/#158): a class/struct
/// is abstract iff it declares at least one pure-virtual method
/// (`virtual ... = 0;`) -- the C++ idiom for an interface/abstract base
/// class. `enum`/`union` are always concrete.
fn type_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "class_specifier" => Some("class"),
        "struct_specifier" => Some("struct"),
        "enum_specifier" => Some("enum"),
        "union_specifier" => Some("union"),
        _ => None,
    }
}

/// True for a declaration-only method signature with a `= 0`
/// pure-specifier, e.g. `virtual double area() const = 0;`.
fn is_pure_virtual(field_decl: &Node, source: &[u8]) -> bool {
    if !has_function_declarator(field_decl) {
        return false;
    }
    let mut cursor = field_decl.walk();
    let found = field_decl
        .named_children(&mut cursor)
        .any(|child| child.kind() == "number_literal" && child.utf8_text(source) == Ok("0"));
    found
}

fn has_pure_virtual_method(type_node: &Node, source: &[u8]) -> bool {
    let mut cursor = type_node.walk();
    for child in type_node.children(&mut cursor) {
        if child.kind() != "field_declaration_list" {
            continue;
        }
        let mut member_cursor = child.walk();
        let found = child
            .named_children(&mut member_cursor)
            .any(|member| member.kind() == "field_declaration" && is_pure_virtual(&member, source));
        if found {
            return true;
        }
    }
    false
}

fn extract_type_attributes(node: &Node, source: &[u8]) -> HashMap<String, AttributeValue> {
    let Some(kind) = type_kind(node.kind()) else {
        return HashMap::new();
    };
    let is_class_like = matches!(node.kind(), "class_specifier" | "struct_specifier");
    let resolved = if is_class_like && has_pure_virtual_method(node, source) {
        "abstractClass"
    } else {
        kind
    };
    HashMap::from([(
        "typeKind".to_string(),
        AttributeValue::Str(resolved.to_string()),
    )])
}

pub fn map_cpp_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(
        root,
        "cpp",
        map_node_kind,
        source,
        file,
        None,
        Some(&extract_type_attributes),
    )
}
