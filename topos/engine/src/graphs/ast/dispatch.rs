//! Parser dispatch — parses source into a [`ParseResult`] (tree-sitter
//! tree + UAST) for any supported language.
//!
//! Tree-sitter is *the* AST engine, by design decision (PR #159): there
//! is deliberately no provider trait, no backend enum, and no dispatch
//! singleton, because there is exactly one parsing path. (The Python
//! original grew a `ParserDispatch` with `TreeSitterProvider` /
//! `NativeAstProvider` selected by an `AstBackend`; the "native"
//! CPython-`ast` path was never consumed downstream and is not carried
//! forward.) Supporting another engine someday means revisiting this
//! module, not pre-abstracting it now.

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

/// Sanitize dynamic type import expressions inside TypeScript generic type
/// arguments (e.g. `load<typeof import("...")>()`) so tree-sitter's parser
/// recognizes `<` as the opening of `type_arguments` rather than a binary
/// less-than comparison. Exact byte length is preserved so all node spans
/// align with the original source.
fn sanitize_typescript_type_imports(source: &str) -> String {
    if !source.contains("import(") && !source.contains("import (") && !source.contains("import`") {
        return source.to_string();
    }

    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut out_bytes = bytes.to_vec();

    let mut i = 0;
    while i < len {
        if bytes[i] == b'<' {
            let start = i + 1;
            let mut j = start;
            while j < len
                && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\n' || bytes[j] == b'\r')
            {
                j += 1;
            }

            let is_typeof_import = bytes[j..].starts_with(b"typeof import(")
                || bytes[j..].starts_with(b"typeof import (")
                || bytes[j..].starts_with(b"typeof import`");
            let is_import = bytes[j..].starts_with(b"import(")
                || bytes[j..].starts_with(b"import (")
                || bytes[j..].starts_with(b"import`");

            if is_typeof_import || is_import {
                // Find matching `>` for this type argument list
                let mut depth = 1;
                let mut k = start;
                let mut in_str: Option<u8> = None;
                let mut in_paren: usize = 0;
                while k < len && depth > 0 {
                    let b = bytes[k];
                    if let Some(quote) = in_str {
                        if b == quote && (k == 0 || bytes[k - 1] != b'\\') {
                            in_str = None;
                        }
                    } else {
                        match b {
                            b'"' | b'\'' | b'`' => in_str = Some(b),
                            b'(' => in_paren += 1,
                            b')' => in_paren = in_paren.saturating_sub(1),
                            b'<' => depth += 1,
                            b'>' if in_paren == 0 => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    k += 1;
                }

                if depth == 0 && k < len {
                    let span_len = k - start;
                    if span_len >= 3 {
                        out_bytes[start] = b'a';
                        out_bytes[start + 1] = b'n';
                        out_bytes[start + 2] = b'y';
                        out_bytes[start + 3..k].fill(b' ');
                    }
                    i = k + 1;
                    continue;
                }
            }
        }
        i += 1;
    }

    String::from_utf8(out_bytes).unwrap_or_else(|_| source.to_string())
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

    let parse_input = if language == "typescript" {
        sanitize_typescript_type_imports(source)
    } else {
        source.to_string()
    };

    let tree = parser
        .parse(&parse_input, None)
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
    fn a_rust_local_named_raw_is_not_a_parse_error() {
        // `raw` is not a reserved word, so `&raw` on an ordinary local is
        // valid, rustc-accepted code — but it collides with the `&raw const`
        // / `&raw mut` raw-borrow syntax. tree-sitter-rust 0.23 resolved that
        // collision by erroring, which marked the whole file unparseable and
        // silently scored it SLOP with no explanation (#285). One such file
        // shipped in this repo. Locks the 0.24 grammar in.
        let result = parse_source("fn f(){ let raw = 1; g(&raw); }", "rust", None).unwrap();
        assert!(!result.has_errors, "a local named `raw` broke the parse");
    }

    #[test]
    fn rust_raw_borrow_syntax_still_parses() {
        // The other side of the same collision: genuine raw-borrow syntax
        // must keep working, so the fix cannot be to drop the grammar rule.
        let source = "fn f(){ let mut x = 1; let p = &raw mut x; let q = &raw const x; }";
        let result = parse_source(source, "rust", None).unwrap();
        assert!(!result.has_errors, "raw-borrow syntax regressed");
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

    #[test]
    fn typescript_dynamic_type_imports_in_generics_parse_cleanly() {
        let cases = [
            "const x = load<typeof import('./module')>();\n",
            "const x2 = load<import('./module')>();\n",
            "const x3 = load<import('./module').Type>();\n",
            "const x4 = load<typeof import('./module').Type>();\n",
            "const x5 = fn<typeof import('./mod'), Other>();\n",
            "const x6 = fn<import('./mod'), Other>();\n",
            "const x7 = obj.method<typeof import('./mod')>();\n",
            "const x8 = await load<typeof import('./mod')>();\n",
            "type X = import('ai').StopCondition;\n",
            "type Y = typeof import('ai');\n",
        ];
        for case in cases {
            let res = parse_source(case, "typescript", None).unwrap();
            assert!(!res.has_errors, "failed to parse cleanly: {case}");
        }
    }
}
