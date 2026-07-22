//! JavaScript → UAST mapper.

use tree_sitter::Node;

use super::mapper_common::map_tree_sitter_to_uast;
use super::models::UASTNode;

pub fn map_node_kind(kind: &str) -> &'static str {
    const NODE_KIND_TABLE: &[(&str, &str)] = &[
        ("function_definition", "FunctionDecl"),
        ("function_declaration", "FunctionDecl"),
        ("function", "FunctionDecl"),
        ("function_expression", "FunctionDecl"),
        ("arrow_function", "FunctionDecl"),
        ("class_definition", "TypeDecl"),
        ("class_declaration", "TypeDecl"),
        ("method_definition", "MethodDecl"),
        ("lexical_declaration", "VarDecl"),
        ("variable_declaration", "VarDecl"),
        ("if_statement", "IfStmt"),
        ("for_statement", "ForStmt"),
        ("while_statement", "WhileStmt"),
        ("switch_statement", "MatchStmt"),
        ("return_statement", "ReturnStmt"),
        ("break_statement", "BreakStmt"),
        ("continue_statement", "ContinueStmt"),
        ("throw_statement", "ThrowStmt"),
        ("try_statement", "TryStmt"),
        ("expression_statement", "ExprStmt"),
        ("assignment", "AssignExpr"),
        ("augmented_assignment", "AssignExpr"),
        ("binary_expression", "BinaryExpr"),
        ("boolean_operator", "BinaryExpr"),
        ("unary_expression", "UnaryExpr"),
        ("call_expression", "CallExpr"),
        ("member_expression", "MemberExpr"),
        ("field_expression", "MemberExpr"),
        ("subscript", "MemberExpr"),
        ("identifier", "Identifier"),
        ("module", "File"),
        ("program", "File"),
        ("translation_unit", "File"),
        ("source_file", "File"),
    ];
    if let Some((_, mapped)) = NODE_KIND_TABLE.iter().find(|(k, _)| *k == kind) {
        return mapped;
    }
    if kind.ends_with("literal") || matches!(kind, "string" | "integer" | "float") {
        return "Literal";
    }
    "Unknown"
}

pub fn map_javascript_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(root, "javascript", map_node_kind, source, file, None, None)
}
