//! Parser dispatch — parses source into a [`ParseResult`] (tree-sitter
//! tree + UAST) for any supported language.
//!
//! # Deviation from the Python original
//!
//! Python's dispatch layer is a `ParserDispatch` holding two
//! `AstProvider` implementations (`TreeSitterProvider`,
//! `NativeAstProvider`) selected by an `AstBackend` (`"tree-sitter"` /
//! `"native"` / `"hybrid"`), plus a module-level singleton with a
//! test-only `reset_dispatch()`. The two providers exist because Python
//! *can* additionally build a CPython `ast.Module` for Python source
//! (`NativeAstProvider`) — but nothing consumes that native AST even on
//! the Python side today (`native_ast` is `None` for every other
//! language and unused downstream). Rust has no equivalent to CPython's
//! `ast` module, so there is exactly one provider here: tree-sitter. One
//! implementation doesn't need a trait, a backend enum, or a stateless
//! singleton to select between it — this module is a plain function.

use tree_sitter::{Language, Parser};

use super::types::{ParseResult, ParserProvenance};
use crate::graphs::uast::mapper_common::parser_identity;
use crate::graphs::uast::mapper_cpp::map_cpp_tree_to_uast;
use crate::graphs::uast::mapper_go::map_go_tree_to_uast;
use crate::graphs::uast::mapper_javascript::map_javascript_tree_to_uast;
use crate::graphs::uast::mapper_python::map_python_tree_to_uast;
use crate::graphs::uast::mapper_rust::map_rust_tree_to_uast;
use crate::graphs::uast::mapper_typescript::map_typescript_tree_to_uast;

/// Failure to parse a source string — an unsupported language, or a
/// cancelled/failed tree-sitter parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchError(pub String);

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DispatchError {}

fn tree_sitter_language(language: &str, file: Option<&str>) -> Result<Language, DispatchError> {
    Ok(match language {
        "python" => tree_sitter_python::LANGUAGE.into(),
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        // Matches Python's `parse_typescript`: the TSX grammar when the
        // file path ends in `.tsx`, otherwise plain TypeScript.
        "typescript" => {
            if file.is_some_and(|f| f.ends_with(".tsx")) {
                tree_sitter_typescript::LANGUAGE_TSX.into()
            } else {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            }
        }
        "cpp" => tree_sitter_cpp::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        other => return Err(DispatchError(format!("Language '{other}' not supported"))),
    })
}

/// Parse `source` and build both the tree-sitter tree and the UAST for
/// it in one pass.
pub fn parse_source(
    source: &str,
    language: &str,
    file: Option<&str>,
) -> Result<ParseResult, DispatchError> {
    let ts_language = tree_sitter_language(language, file)?;
    let mut parser = Parser::new();
    parser
        .set_language(&ts_language)
        .map_err(|e| DispatchError(format!("failed to load {language} grammar: {e}")))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| DispatchError(format!("parsing {language} source was cancelled")))?;

    let source_bytes = source.as_bytes();
    let uast_root = match language {
        "python" => map_python_tree_to_uast(tree.root_node(), source_bytes, file),
        "rust" => map_rust_tree_to_uast(tree.root_node(), source_bytes, file),
        "javascript" => map_javascript_tree_to_uast(tree.root_node(), source_bytes, file),
        "typescript" => map_typescript_tree_to_uast(tree.root_node(), source_bytes, file),
        "cpp" => map_cpp_tree_to_uast(tree.root_node(), source_bytes, file),
        "go" => map_go_tree_to_uast(tree.root_node(), source_bytes, file),
        other => return Err(DispatchError(format!("Language '{other}' not supported"))),
    };

    let (parser_name, parser_version) = parser_identity(language);
    let has_errors = tree.root_node().has_error();
    let node_kind = tree.root_node().kind().to_string();

    Ok(ParseResult {
        tree,
        source: source.to_string(),
        language: language.to_string(),
        provenance: ParserProvenance {
            parser: parser_name.to_string(),
            parser_version: parser_version.to_string(),
            node_kind,
        },
        uast_root,
        has_errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_python_and_builds_uast() {
        let result = parse_source("x = 1 + 2", "python", None).unwrap();
        assert!(!result.has_errors);
        assert_eq!(result.language, "python");
        assert!(!result.uast_root.children.is_empty());
    }

    #[test]
    fn parses_rust_and_builds_uast() {
        let result = parse_source("fn main() {}", "rust", None).unwrap();
        assert!(!result.has_errors);
    }

    #[test]
    fn unsupported_language_is_an_error_not_a_panic() {
        assert!(parse_source("x = 1", "cobol", None).is_err());
    }

    #[test]
    fn tsx_file_extension_selects_tsx_grammar() {
        // The .tsx-specific `<Foo />` JSX syntax only parses cleanly
        // under the TSX grammar; a plain-TypeScript parse would report
        // an error node here.
        let result = parse_source("const el = <Foo />;", "typescript", Some("Widget.tsx")).unwrap();
        assert!(!result.has_errors);
    }
}
