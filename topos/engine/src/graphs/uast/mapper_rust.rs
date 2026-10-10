//! Rust → UAST mapper.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::mapper_common::{logical_operator_attribute, map_tree_sitter_to_uast, TestNodeFilter};
use super::models::{AttributeValue, UASTNode};

/// Classify the attribute syntax, never text inside strings or comments.
fn test_attribute(node: &Node, source: &[u8]) -> Option<&'static str> {
    if node.kind() != "attribute_item" {
        return None;
    }
    let attribute = node.named_child(0)?;
    let path = attribute.named_child(0)?;
    match path.kind() {
        "identifier" if path.utf8_text(source).ok()? == "cfg" => {
            let arguments = attribute.named_child(1)?;
            if arguments.kind() != "token_tree" {
                return None;
            }
            let mut cursor = arguments.walk();
            let mut tokens = arguments
                .named_children(&mut cursor)
                .filter(|n| !matches!(n.kind(), "line_comment" | "block_comment"));
            let token = tokens.next()?;
            (token.kind() == "identifier"
                && token.utf8_text(source).ok()? == "test"
                && tokens.next().is_none())
            .then_some("cfg")
        }
        "identifier" | "scoped_identifier" => {
            let name = if path.kind() == "identifier" {
                path
            } else {
                path.child_by_field_name("name")?
            };
            (name.utf8_text(source).ok()? == "test").then_some("test")
        }
        _ => None,
    }
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
                if test_attribute(sibling, source) == Some("cfg") {
                    pending_test_attr = true;
                    dropped.insert(sibling.id());
                }
                continue;
            }
            if matches!(sibling.kind(), "line_comment" | "block_comment") {
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

pub fn map_node_kind(node: &Node) -> &'static str {
    let kind = node.kind();
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

fn extract_attributes(node: &Node, source: &[u8]) -> HashMap<String, AttributeValue> {
    let mut attrs = extract_type_attributes(node, source);
    attrs.extend(logical_operator_attribute(node, source));
    if let Some(kind) = test_attribute(node, source) {
        attrs.insert(
            "rustTestAttribute".to_string(),
            AttributeValue::Str(kind.to_string()),
        );
    }
    attrs
}

pub fn map_rust_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(
        root,
        "rust",
        map_node_kind,
        source,
        file,
        Some(&CfgTestFilter),
        Some(&extract_attributes),
    )
}

/// Like [`map_rust_tree_to_uast`] but keeps `#[cfg(test)]` items, so
/// structural coverage can count inline tests instead of discarding them.
pub fn map_rust_tree_to_uast_with_tests(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(
        root,
        "rust",
        map_node_kind,
        source,
        file,
        None,
        Some(&extract_attributes),
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
    fn cfg_test_filter_uses_syntax_and_skips_comments() {
        let source = r#"
            #[doc = "cfg(test)"]
            fn production() {}
            #[cfg( /* condition */ test )]
            // explanation
            #[allow(dead_code)]
            mod tests { fn checks() {} }
        "#;
        let tree = parse(source);
        let uast = map_rust_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        let declarations =
            crate::functors::profunctors::uast::structural_test_coverage::extract_declarations(
                &uast,
            );
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].span.start_line, 3);
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
