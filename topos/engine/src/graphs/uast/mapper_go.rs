//! Go → UAST mapper.

use std::collections::HashMap;

use tree_sitter::Node;

use super::mapper_common::map_tree_sitter_to_uast;
use super::models::{AttributeValue, UASTNode};

/// `go_statement`/`defer_statement` have no dedicated UAST control-flow
/// kind, so both map structurally (by node type, not callee identifier)
/// to plain expression statements -- consistent with how `panic(...)`
/// stays an ordinary CallExpr below.
pub fn map_node_kind(kind: &str) -> &'static str {
    const NODE_KIND_TABLE: &[(&str, &str)] = &[
        ("function_declaration", "FunctionDecl"),
        ("method_declaration", "MethodDecl"),
        ("type_declaration", "TypeDecl"),
        ("const_declaration", "VarDecl"),
        ("var_declaration", "VarDecl"),
        ("short_var_declaration", "VarDecl"), // `x := expr`
        ("if_statement", "IfStmt"),
        ("for_statement", "ForStmt"), // Go's single loop keyword covers all forms.
        ("expression_switch_statement", "MatchStmt"),
        ("type_switch_statement", "MatchStmt"),
        ("select_statement", "MatchStmt"), // channel-select: structurally a multi-way branch.
        ("return_statement", "ReturnStmt"),
        ("break_statement", "BreakStmt"),
        ("continue_statement", "ContinueStmt"),
        ("expression_statement", "ExprStmt"),
        ("assignment_statement", "AssignExpr"),
        ("inc_statement", "AssignExpr"), // `x++` / `x--`
        ("go_statement", "ExprStmt"),    // `go f()` goroutine launch
        ("defer_statement", "ExprStmt"),
        ("binary_expression", "BinaryExpr"),
        ("unary_expression", "UnaryExpr"),
        ("call_expression", "CallExpr"), // covers `panic(...)` too.
        ("selector_expression", "MemberExpr"), // `x.y`, `pkg.Func`, method targets.
        ("index_expression", "MemberExpr"),
        ("identifier", "Identifier"),
        ("field_identifier", "Identifier"),
        ("type_identifier", "Identifier"),
        ("package_identifier", "Identifier"),
        ("int_literal", "Literal"),
        ("float_literal", "Literal"),
        ("imaginary_literal", "Literal"),
        ("rune_literal", "Literal"),
        ("interpreted_string_literal", "Literal"),
        ("raw_string_literal", "Literal"),
        ("true", "Literal"),
        ("false", "Literal"),
        ("nil", "Literal"),
        ("source_file", "File"),
    ];
    NODE_KIND_TABLE
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or("Unknown", |(_, mapped)| mapped)
}

/// Classify a `type_declaration` as interface/struct via its `type_spec`
/// grandchild -- the discriminating grammar node lives one level below
/// the `TypeDecl`-mapped node itself (Go wraps every type declaration,
/// whether interface, struct, or alias, in the same outer
/// `type_declaration`). Aliases to primitives/other named types are
/// deliberately unclassified (no `typeKind`), excluding them from the
/// Abstractness ratio rather than miscounting them as concrete.
fn extract_type_attributes(node: &Node, _source: &[u8]) -> HashMap<String, AttributeValue> {
    if node.kind() != "type_declaration" {
        return HashMap::new();
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "type_spec" {
            continue;
        }
        let mut inner_cursor = child.walk();
        for grandchild in child.named_children(&mut inner_cursor) {
            let kind = match grandchild.kind() {
                "interface_type" => Some("interface"),
                "struct_type" => Some("struct"),
                _ => None,
            };
            if let Some(kind) = kind {
                return HashMap::from([(
                    "typeKind".to_string(),
                    AttributeValue::Str(kind.to_string()),
                )]);
            }
        }
    }
    HashMap::new()
}

pub fn map_go_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(
        root,
        "go",
        map_node_kind,
        source,
        file,
        None,
        Some(&extract_type_attributes),
    )
}
