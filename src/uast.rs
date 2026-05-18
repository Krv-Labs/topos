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
    fn new(
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
    fn new(parser: String, parser_version: String, node_kind: String) -> Self {
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
    fn new(
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
