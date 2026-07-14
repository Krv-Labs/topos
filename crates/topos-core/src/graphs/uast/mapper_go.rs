//! Go → UAST mapper.

use std::collections::HashMap;

use tree_sitter::Node;

use super::mapper_common::map_tree_sitter_to_uast;
use super::models::{AttributeValue, UASTNode};

pub fn map_node_kind(kind: &str) -> &'static str {
    match kind {
        "function_declaration" => "FunctionDecl",
        "method_declaration" => "MethodDecl",
        "type_declaration" => "TypeDecl",
        "const_declaration" | "var_declaration" => "VarDecl",
        // `x := expr`
        "short_var_declaration" => "VarDecl",
        "if_statement" => "IfStmt",
        // Go's single loop keyword covers all forms.
        "for_statement" => "ForStmt",
        "expression_switch_statement" | "type_switch_statement" => "MatchStmt",
        // channel-select: structurally a multi-way branch.
        "select_statement" => "MatchStmt",
        "return_statement" => "ReturnStmt",
        "break_statement" => "BreakStmt",
        "continue_statement" => "ContinueStmt",
        "expression_statement" => "ExprStmt",
        "assignment_statement" => "AssignExpr",
        // `x++` / `x--`
        "inc_statement" => "AssignExpr",
        // Neither has a dedicated UAST control-flow kind; mapped
        // structurally (by node type, not callee identifier) as plain
        // expression statements, consistent with how `panic(...)` stays
        // an ordinary CallExpr below.
        "go_statement" => "ExprStmt", // `go f()` goroutine launch
        "defer_statement" => "ExprStmt",
        "binary_expression" => "BinaryExpr",
        "unary_expression" => "UnaryExpr",
        // Covers `panic(...)` too.
        "call_expression" => "CallExpr",
        // `x.y`, `pkg.Func`, method targets.
        "selector_expression" => "MemberExpr",
        "index_expression" => "MemberExpr",
        "identifier" | "field_identifier" | "type_identifier" | "package_identifier" => {
            "Identifier"
        }
        "int_literal"
        | "float_literal"
        | "imaginary_literal"
        | "rune_literal"
        | "interpreted_string_literal"
        | "raw_string_literal"
        | "true"
        | "false"
        | "nil" => "Literal",
        "source_file" => "File",
        _ => "Unknown",
    }
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
