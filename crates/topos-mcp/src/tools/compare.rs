//! Structural comparison tools: AST edit distance between two programs.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_core::core::morphism::ProgramMorphism;
use topos_core::functors::profunctors::ast::compare::calculate_ast_distance;

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

fn compare_code_impl(params: &CompareCodeInput) -> CallToolResult {
    let src = ProgramMorphism::new(&params.source_code, &params.language);
    let tgt = ProgramMorphism::new(&params.target_code, &params.language);

    if !(src.is_valid() && tgt.is_valid()) {
        let model = failed_comparison(
            "Failed to parse one or both code snippets.".to_string(),
            src.is_valid(),
            tgt.is_valid(),
        );
        return to_tool_result(&model, render_comparison_md(&model));
    }

    let (Some(src_ast), Some(tgt_ast)) = (src.ast.as_ref(), tgt.ast.as_ref()) else {
        let model = failed_comparison(
            "Failed to parse one or both code snippets.".to_string(),
            src.is_valid(),
            tgt.is_valid(),
        );
        return to_tool_result(&model, render_comparison_md(&model));
    };

    let result = calculate_ast_distance(src_ast, tgt_ast);
    let model = ComparisonResult {
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
    };
    to_tool_result(&model, render_comparison_md(&model))
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
    /// Read-only; parses both files, never writes or scores. Use for clone
    /// detection or refactor impact; use `topos_assess_*` for a quality
    /// verdict. Returns a ComparisonResult (see `topos_compare_code`).
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
        compare_code_impl(&CompareCodeInput {
            source_code: source_text,
            target_code: target_text,
            language: "python".to_string(),
        })
    }
}
