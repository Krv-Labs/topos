//! C++ → UAST mapper.

use tree_sitter::Node;

use super::mapper_common::map_tree_sitter_to_uast;
use super::models::UASTNode;

pub fn map_node_kind(kind: &str) -> &'static str {
    match kind {
        "function_definition" => "FunctionDecl",
        "class_definition" | "struct_item" | "enum_item" | "impl_item" => "TypeDecl",
        "function_item" => "FunctionDecl",
        "method_definition" => "MethodDecl",
        "lexical_declaration" | "variable_declaration" => "VarDecl",
        "if_statement" => "IfStmt",
        "for_statement" => "ForStmt",
        "while_statement" => "WhileStmt",
        "match_statement" => "MatchStmt",
        "return_statement" => "ReturnStmt",
        "break_statement" => "BreakStmt",
        "continue_statement" => "ContinueStmt",
        "throw_statement" => "ThrowStmt",
        "try_statement" => "TryStmt",
        "expression_statement" => "ExprStmt",
        "assignment" | "augmented_assignment" => "AssignExpr",
        "binary_expression" | "boolean_operator" => "BinaryExpr",
        "unary_expression" => "UnaryExpr",
        "call_expression" => "CallExpr",
        "member_expression" | "field_expression" | "subscript" => "MemberExpr",
        "identifier" => "Identifier",
        "module" | "program" | "translation_unit" | "source_file" => "File",
        s if s.ends_with("literal") || matches!(s, "string" | "integer" | "float") => "Literal",
        _ => "Unknown",
    }
}

pub fn map_cpp_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(root, "cpp", map_node_kind, source, file, None)
}
