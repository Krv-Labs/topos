//! JavaScript → UAST mapper.

use std::collections::HashMap;

use tree_sitter::Node;

use super::mapper_common::{logical_operator_attribute, map_tree_sitter_to_uast};
use super::models::{AttributeValue, UASTNode};

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
        ("catch_clause", "CatchClause"),
        ("expression_statement", "ExprStmt"),
        ("assignment", "AssignExpr"),
        ("augmented_assignment", "AssignExpr"),
        ("binary_expression", "BinaryExpr"),
        ("boolean_operator", "BinaryExpr"),
        ("ternary_expression", "ConditionalExpr"),
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

fn extract_attributes(node: &Node, source: &[u8]) -> HashMap<String, AttributeValue> {
    logical_operator_attribute(node, source)
}

pub fn map_javascript_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(
        root,
        "javascript",
        map_node_kind,
        source,
        file,
        None,
        Some(&extract_attributes),
    )
}

#[cfg(test)]
mod tests {
    use super::super::models::AttributeValue;
    use super::*;
    use tree_sitter::Parser;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }

    fn collect_binary_operators(node: &UASTNode, out: &mut Vec<String>) {
        if node.kind == "BinaryExpr" {
            if let Some(AttributeValue::Str(op)) = node.attributes.get("operator") {
                out.push(op.clone());
            }
        }
        for child in &node.children {
            collect_binary_operators(child, out);
        }
    }

    #[test]
    fn records_logical_operators_on_binary_expr() {
        let source = "function f(a, b, c) { return a && b && c; }\n";
        let tree = parse(source);
        let uast = map_javascript_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        let mut ops = Vec::new();
        collect_binary_operators(&uast, &mut ops);
        assert_eq!(ops, vec!["&&", "&&"]);
    }
}
