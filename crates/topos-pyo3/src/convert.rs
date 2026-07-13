//! Conversions between `topos-core` graph types and pyo3-facing types.

use crate::uast::{NativeRef, SourceSpan, UASTNode};
use topos_core::graphs::uast::models::{AttributeValue, UASTNode as CoreUAST};

pub fn core_uast_to_py(node: &CoreUAST) -> UASTNode {
    UASTNode {
        kind: node.kind.clone(),
        lang: node.lang.clone(),
        span: SourceSpan {
            file: node.span.file.clone(),
            start_byte: node.span.start_byte,
            end_byte: node.span.end_byte,
            start_line: node.span.start_line,
            start_column: node.span.start_column,
            end_line: node.span.end_line,
            end_column: node.span.end_column,
        },
        native: NativeRef {
            parser: node.native.parser.clone(),
            parser_version: node.native.parser_version.clone(),
            node_kind: node.native.node_kind.clone(),
        },
        attributes: node
            .attributes
            .iter()
            .map(|(k, v)| (k.clone(), attr_to_string(v)))
            .collect(),
        children: node.children.iter().map(core_uast_to_py).collect(),
        id: node.id.clone(),
    }
}

fn attr_to_string(value: &AttributeValue) -> String {
    match value {
        AttributeValue::Bool(b) => b.to_string(),
        AttributeValue::Str(s) => s.clone(),
    }
}

pub fn py_uast_to_core(node: &UASTNode) -> CoreUAST {
    CoreUAST {
        kind: node.kind.clone(),
        lang: node.lang.clone(),
        span: topos_core::graphs::uast::models::SourceSpan {
            file: node.span.file.clone(),
            start_byte: node.span.start_byte,
            end_byte: node.span.end_byte,
            start_line: node.span.start_line,
            start_column: node.span.start_column,
            end_line: node.span.end_line,
            end_column: node.span.end_column,
        },
        native: topos_core::graphs::uast::models::NativeRef {
            parser: node.native.parser.clone(),
            parser_version: node.native.parser_version.clone(),
            node_kind: node.native.node_kind.clone(),
        },
        attributes: node
            .attributes
            .iter()
            .map(|(k, v)| (k.clone(), attr_from_string(v)))
            .collect(),
        children: node.children.iter().map(py_uast_to_core).collect(),
        id: node.id.clone(),
    }
}

fn attr_from_string(value: &str) -> AttributeValue {
    match value {
        "true" => AttributeValue::Bool(true),
        "false" => AttributeValue::Bool(false),
        other => AttributeValue::Str(other.to_string()),
    }
}
