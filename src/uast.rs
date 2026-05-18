use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[pyclass(get_all)]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SourceSpan {
    pub file: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[pymethods]
impl SourceSpan {
    #[new]
    #[pyo3(signature = (file, start_byte, end_byte, start_line, start_column, end_line, end_column))]
    pub fn new(
        file: Option<String>,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
    ) -> Self {
        Self {
            file,
            start_byte,
            end_byte,
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }
}

#[pyclass(get_all)]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct NativeRef {
    pub parser: String,
    pub parser_version: String,
    pub node_kind: String,
}

#[pymethods]
impl NativeRef {
    #[new]
    pub fn new(parser: String, parser_version: String, node_kind: String) -> Self {
        Self {
            parser,
            parser_version,
            node_kind,
        }
    }
}

#[pyclass(get_all)]
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UASTNode {
    pub kind: String,
    pub lang: String,
    pub span: SourceSpan,
    pub native: NativeRef,
    pub attributes: HashMap<String, String>, // Simplified for now, Python uses Any
    pub children: Vec<UASTNode>,
    pub id: String,
}

#[pymethods]
impl UASTNode {
    #[new]
    #[pyo3(signature = (kind, lang, span, native, attributes=None, children=None, id=String::new()))]
    pub fn new(
        kind: String,
        lang: String,
        span: SourceSpan,
        native: NativeRef,
        attributes: Option<HashMap<String, String>>,
        children: Option<Vec<UASTNode>>,
        id: String,
    ) -> Self {
        Self {
            kind,
            lang,
            span,
            native,
            attributes: attributes.unwrap_or_default(),
            children: children.unwrap_or_default(),
            id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that a `SourceSpan` is correctly initialized with the provided start and end coordinates.
    #[test]
    fn test_source_span_creation() {
        let span = SourceSpan::new(Some("test.py".to_string()), 0, 10, 1, 0, 1, 10);
        assert_eq!(span.file, Some("test.py".to_string()));
        assert_eq!(span.start_byte, 0);
        assert_eq!(span.end_byte, 10);
    }

    /// Verifies that a `NativeRef` correctly stores parser version and node type information.
    #[test]
    fn test_native_ref_creation() {
        let native = NativeRef::new(
            "tree-sitter".to_string(),
            "0.20.0".to_string(),
            "function_definition".to_string(),
        );
        assert_eq!(native.parser, "tree-sitter");
        assert_eq!(native.node_kind, "function_definition");
    }

    /// Tests the successful instantiation of a basic `UASTNode` with default empty child and attribute collections.
    #[test]
    fn test_uast_node_creation() {
        let span = SourceSpan::new(None, 0, 0, 0, 0, 0, 0);
        let native = NativeRef::new("p".to_string(), "v".to_string(), "k".to_string());
        let node = UASTNode::new(
            "module".to_string(),
            "python".to_string(),
            span,
            native,
            None,
            None,
            "root".to_string(),
        );
        assert_eq!(node.kind, "module");
        assert_eq!(node.id, "root");
        assert_eq!(node.children.len(), 0);
    }
}
