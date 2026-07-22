//! Rust → UAST mapper.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::mapper_common::{map_tree_sitter_to_uast, TestNodeFilter};
use super::models::{AttributeValue, UASTNode};

const CFG_TEST_MARKER: &str = "cfg(test)";

fn is_cfg_test_attribute(node: &Node, source: &[u8]) -> bool {
    node.kind() == "attribute_item"
        && node
            .utf8_text(source)
            .is_ok_and(|text| text.contains(CFG_TEST_MARKER))
}

/// Rust's [`TestNodeFilter`]: drop `#[cfg(test)]`-annotated items.
///
/// Tree-sitter-rust represents an attribute as a *preceding sibling* of
/// the item it annotates (both children of the same parent), not as a
/// descendant of that item — so this is a single forward scan over
/// `siblings`, not a per-node lookup: the `#[cfg(test)]` attribute
/// itself is dropped, and so is the item immediately following it
/// (skipping over any intervening non-`cfg(test)` attributes).
pub struct CfgTestFilter;

impl TestNodeFilter for CfgTestFilter {
    fn drop_set(&self, named_siblings: &[Node], source: &[u8]) -> HashSet<usize> {
        let mut dropped = HashSet::new();
        let mut pending_test_attr = false;
        for sibling in named_siblings {
            if sibling.kind() == "attribute_item" {
                if is_cfg_test_attribute(sibling, source) {
                    pending_test_attr = true;
                    dropped.insert(sibling.id());
                }
                continue;
            }
            if pending_test_attr {
                pending_test_attr = false;
                dropped.insert(sibling.id());
            }
        }
        dropped
    }
}

pub fn map_node_kind(kind: &str) -> &'static str {
    const NODE_KIND_TABLE: &[(&str, &str)] = &[
        ("struct_item", "TypeDecl"),
        ("enum_item", "TypeDecl"),
        ("impl_item", "TypeDecl"),
        ("trait_item", "TypeDecl"),
        ("function_item", "FunctionDecl"),
        ("let_declaration", "VarDecl"),
        ("if_expression", "IfStmt"),
        ("for_expression", "ForStmt"),
        ("while_expression", "WhileStmt"),
        ("loop_expression", "WhileStmt"),
        ("match_expression", "MatchStmt"),
        ("return_expression", "ReturnStmt"),
        ("break_expression", "BreakStmt"),
        ("continue_expression", "ContinueStmt"),
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

/// Martin Abstractness classification for `TypeDecl` nodes. `impl_item`
/// is intentionally absent: it implements an existing type rather than
/// declaring a new one, so it must not be double-counted in the
/// abstract/concrete ratio.
fn type_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "trait_item" => Some("trait"),
        "struct_item" => Some("struct"),
        "enum_item" => Some("enum"),
        _ => None,
    }
}

fn extract_type_attributes(node: &Node, _source: &[u8]) -> HashMap<String, AttributeValue> {
    match type_kind(node.kind()) {
        Some(kind) => HashMap::from([(
            "typeKind".to_string(),
            AttributeValue::Str(kind.to_string()),
        )]),
        None => HashMap::new(),
    }
}

pub fn map_rust_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(
        root,
        "rust",
        map_node_kind,
        source,
        file,
        Some(&CfgTestFilter),
        Some(&extract_type_attributes),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }

    fn collect_kinds(node: &UASTNode, out: &mut HashSet<String>) {
        out.insert(node.native.node_kind.clone());
        for child in &node.children {
            collect_kinds(child, out);
        }
    }

    #[test]
    fn drops_cfg_test_module() {
        let source = "fn add(a: i32, b: i32) -> i32 { a + b }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_adds() {}\n}\n";
        let tree = parse(source);
        let uast = map_rust_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        let mut kinds = HashSet::new();
        collect_kinds(&uast, &mut kinds);
        assert!(!kinds.contains("mod_item"));
        assert!(kinds.contains("function_item"));
    }

    #[test]
    fn keeps_non_test_attributes() {
        let source = "#[derive(Debug)]\nstruct Point { x: i32, y: i32 }\n";
        let tree = parse(source);
        let uast = map_rust_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        let mut kinds = HashSet::new();
        collect_kinds(&uast, &mut kinds);
        assert!(kinds.contains("struct_item"));
    }
}
