//! Structural comparison tools: AST edit distance between two programs.

use std::path::Path;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::functors::profunctors::ast::compare::calculate_ast_distance;

use crate::evaluation::detect_language;
use crate::formatting::to_tool_result;
use crate::schemas::{CompareCodeInput, CompareFilesInput, ComparisonResult};
use crate::security::read_safe_utf8_file;
use crate::server::ToposServer;

fn failed_comparison(error: String, source_valid: bool, target_valid: bool) -> ComparisonResult {
    ComparisonResult {
        raw_distance: 0.0,
        normalized_distance: 0.0,
        similarity: 0.0,
        operations: Default::default(),
        source_valid,
        target_valid,
        warnings: Vec::new(),
        error: Some(error),
    }
}

pub(crate) fn render_comparison_md(r: &ComparisonResult) -> String {
    if let Some(err) = &r.error {
        return format!("**Error:** {err}");
    }
    let mut lines = vec![
        format!("**Normalized distance:** {:.3}", r.normalized_distance),
        format!("**Similarity:** {:.3}", r.similarity),
        format!("**Raw distance:** {:.1}", r.raw_distance),
        format!(
            "**Validity:** source={}, target={}",
            r.source_valid, r.target_valid
        ),
    ];
    if !r.operations.is_empty() {
        let mut pairs: Vec<_> = r.operations.iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        let ops = pairs
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("**Operations:** {ops}"));
    }
    lines.join("\n")
}

fn compare_texts(
    source_code: &str,
    source_language: &str,
    target_code: &str,
    target_language: &str,
) -> ComparisonResult {
    let src = ProgramMorphism::new(source_code, source_language);
    let tgt = ProgramMorphism::new(target_code, target_language);

    if !(src.is_valid() && tgt.is_valid()) {
        return failed_comparison(
            "Failed to parse one or both code snippets.".to_string(),
            src.is_valid(),
            tgt.is_valid(),
        );
    }

    let (Some(src_ast), Some(tgt_ast)) = (src.ast.as_ref(), tgt.ast.as_ref()) else {
        return failed_comparison(
            "Failed to parse one or both code snippets.".to_string(),
            src.is_valid(),
            tgt.is_valid(),
        );
    };

    let result = calculate_ast_distance(src_ast, tgt_ast);
    ComparisonResult {
        raw_distance: result.raw_distance as f64,
        normalized_distance: result.normalized_distance,
        similarity: 1.0 - result.normalized_distance,
        operations: result
            .operations
            .into_iter()
            .map(|(k, v)| (k, v as i64))
            .collect(),
        source_valid: true,
        target_valid: true,
        warnings: Vec::new(),
        error: None,
    }
}

fn finish_comparison(model: ComparisonResult) -> CallToolResult {
    let md = render_comparison_md(&model);
    to_tool_result(&model, md)
}

fn compare_code_impl(params: &CompareCodeInput) -> CallToolResult {
    finish_comparison(compare_texts(
        &params.source_code,
        &params.language,
        &params.target_code,
        &params.language,
    ))
}

/// Compare two file bodies using the language implied by each path's suffix.
fn compare_file_texts(
    source_path: &str,
    source_code: &str,
    target_path: &str,
    target_code: &str,
) -> ComparisonResult {
    compare_texts(
        source_code,
        detect_language(Path::new(source_path)),
        target_code,
        detect_language(Path::new(target_path)),
    )
}

#[tool_router(router = compare_router, vis = "pub(crate)")]
impl ToposServer {
    /// Compute the AST (tree-edit) distance between two source-code
    /// strings.
    ///
    /// Read-only and idempotent; parses both snippets in memory, never
    /// writes or scores. Use for clone detection or to measure refactor
    /// impact; the `topos_assess_*` tools already fold this in as an
    /// anti-gaming check, so call it directly only for the raw number.
    /// Returns a ComparisonResult: `normalized_distance` in [0, 1],
    /// `similarity` (= 1 - it), `raw_distance`, an `operations` edit-count
    /// map, and `source_valid`/`target_valid` (`error` set if either fails
    /// to parse).
    #[tool(
        name = "topos_compare_code",
        annotations(
            title = "Topos Structural Comparison",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_compare_code(
        &self,
        Parameters(params): Parameters<CompareCodeInput>,
    ) -> CallToolResult {
        compare_code_impl(&params)
    }

    /// Compute the AST (tree-edit) distance between two source files on
    /// disk.
    ///
    /// Read-only; parses both files, never writes or scores. Each file is
    /// parsed in the language implied by its suffix, the same way
    /// `topos compare` does. Use for clone detection or refactor impact; use
    /// `topos_assess_*` for a quality verdict. Returns a ComparisonResult
    /// (see `topos_compare_code`).
    #[tool(
        name = "topos_compare_files",
        annotations(
            title = "Topos Structural Comparison",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_compare_files(
        &self,
        Parameters(params): Parameters<CompareFilesInput>,
    ) -> CallToolResult {
        let source_text = match read_safe_utf8_file(&params.source) {
            Ok(text) => text,
            Err(err) => {
                let model = failed_comparison(format!("Source file error: {err}"), false, false);
                return to_tool_result(&model, render_comparison_md(&model));
            }
        };
        let target_text = match read_safe_utf8_file(&params.target) {
            Ok(text) => text,
            Err(err) => {
                let model = failed_comparison(format!("Target file error: {err}"), true, false);
                return to_tool_result(&model, render_comparison_md(&model));
            }
        };
        finish_comparison(compare_file_texts(
            &params.source,
            &source_text,
            &params.target,
            &target_text,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_suffix_parses_as_rust() {
        let model = compare_file_texts(
            "src/a.rs",
            "fn alpha() -> i32 { 1 }\n",
            "src/b.rs",
            "fn beta() -> i32 { 2 }\n",
        );
        assert!(model.error.is_none(), "{model:?}");
        assert!(model.source_valid && model.target_valid);
        assert!(model.similarity > 0.0);
    }

    #[test]
    fn python_suffix_still_parses_as_python() {
        let model = compare_file_texts(
            "a.py",
            "def alpha():\n    return 1\n",
            "b.py",
            "def beta():\n    return 2\n",
        );
        assert!(model.error.is_none(), "{model:?}");
        assert!(model.source_valid && model.target_valid);
    }
}
