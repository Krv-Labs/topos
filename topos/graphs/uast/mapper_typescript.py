from __future__ import annotations

from tree_sitter import Node

from topos.graphs.uast.mapper_common import map_tree_sitter_to_uast
from topos.graphs.uast.mapper_javascript import (
    map_node_kind as map_javascript_node_kind,
)
from topos.graphs.uast.models import UASTNode

# Tree-sitter TypeScript grammar nodes not covered by the JavaScript mapper.
_TS_EXTRA = {
    "interface_declaration": "TypeDecl",
    "type_alias_declaration": "TypeDecl",
    "enum_declaration": "TypeDecl",
    "property_signature": "VarDecl",
    "public_field_definition": "VarDecl",
    "abstract_class_declaration": "TypeDecl",
    "module": "File",
    "internal_module": "File",
    # ambient_declaration covers `declare var/let/const/function/class/module` —
    # treating as VarDecl is a lossy simplification, but "declaration of something
    # at ambient scope" maps naturally to the variable-declaration family for
    # structural comparison purposes.
    "ambient_declaration": "VarDecl",
}


_TYPE_KIND = {
    "interface_declaration": "interface",
    "abstract_class_declaration": "abstractClass",
    "class_declaration": "class",
    "enum_declaration": "enum",
    "type_alias_declaration": "typeAlias",
}


def map_node_kind(node: Node) -> str:
    if node.type in _TS_EXTRA:
        return _TS_EXTRA[node.type]
    return map_javascript_node_kind(node)


def extract_type_attributes(node: Node) -> dict[str, object]:
    type_kind = _TYPE_KIND.get(node.type)
    return {"typeKind": type_kind} if type_kind is not None else {}


def map_typescript_tree_to_uast(root: Node, file: str | None = None) -> UASTNode:
    return map_tree_sitter_to_uast(
        root=root,
        language="typescript",
        map_node_kind=map_node_kind,
        file=file,
        extract_attributes=extract_type_attributes,
    )
