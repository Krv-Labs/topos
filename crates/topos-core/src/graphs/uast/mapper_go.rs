//! Go → UAST mapper.

use tree_sitter::Node;

use super::mapper_common::map_tree_sitter_to_uast;
use super::models::UASTNode;

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

pub fn map_go_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(root, "go", map_node_kind, source, file, None)
}
