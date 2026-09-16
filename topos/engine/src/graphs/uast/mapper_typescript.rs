//! TypeScript → UAST mapper. Extends the JavaScript mapper's node-kind
//! table with the declaration forms tree-sitter-typescript adds.

use std::collections::HashMap;

use tree_sitter::Node;

use super::mapper_common::{logical_operator_attribute, map_tree_sitter_to_uast};
use super::mapper_javascript::map_node_kind as map_javascript_node_kind;
use super::models::{AttributeValue, UASTNode};

pub fn map_node_kind(node: &Node) -> &'static str {
    match node.kind() {
        "interface_declaration"
        | "type_alias_declaration"
        | "enum_declaration"
        | "abstract_class_declaration" => "TypeDecl",
        "property_signature" | "public_field_definition" => "VarDecl",
        "module" | "internal_module" => "File",
        // `ambient_declaration` covers `declare var/let/const/function/class/module` —
        // treating as VarDecl is a lossy simplification, but "declaration of
        // something at ambient scope" maps naturally to the variable-declaration
        // family for structural comparison purposes.
        "ambient_declaration" => "VarDecl",
        _other => map_javascript_node_kind(node),
    }
}

fn type_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "interface_declaration" => Some("interface"),
        "abstract_class_declaration" => Some("abstractClass"),
        "class_declaration" => Some("class"),
        "enum_declaration" => Some("enum"),
        "type_alias_declaration" => Some("typeAlias"),
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
    attrs
}

pub fn map_typescript_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(
        root,
        "typescript",
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
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn maps_typescript_subscript_expression_to_member_expr() {
        let source = "function requiredFunction(obj: any, key: string) { return obj[key]; }\n";
        let tree = parse(source);
        let uast = map_typescript_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        fn find_member_expr(node: &UASTNode) -> bool {
            if node.kind == "MemberExpr" && node.native.node_kind == "subscript_expression" {
                return true;
            }
            node.children.iter().any(find_member_expr)
        }
        assert!(
            find_member_expr(&uast),
            "typescript subscript_expression was not mapped to MemberExpr"
        );
    }

    #[test]
    fn maps_type_declarations_and_attributes() {
        let source = "interface Foo {}\ntype Bar = number;\nenum Baz { A }\n";
        let tree = parse(source);
        let uast = map_typescript_tree_to_uast(tree.root_node(), source.as_bytes(), None);
        let mut type_kinds = Vec::new();
        for child in &uast.children {
            if child.kind == "TypeDecl" {
                if let Some(AttributeValue::Str(k)) = child.attributes.get("typeKind") {
                    type_kinds.push(k.clone());
                }
            }
        }
        assert_eq!(type_kinds, vec!["interface", "typeAlias", "enum"]);
    }
}
