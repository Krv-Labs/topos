//! TypeScript → UAST mapper. Extends the JavaScript mapper's node-kind
//! table with the declaration forms tree-sitter-typescript adds.

use tree_sitter::Node;

use super::mapper_common::map_tree_sitter_to_uast;
use super::mapper_javascript::map_node_kind as map_javascript_node_kind;
use super::models::UASTNode;

pub fn map_node_kind(kind: &str) -> &'static str {
    match kind {
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
        other => map_javascript_node_kind(other),
    }
}

pub fn map_typescript_tree_to_uast(root: Node, source: &[u8], file: Option<&str>) -> UASTNode {
    map_tree_sitter_to_uast(root, "typescript", map_node_kind, source, file, None)
}
