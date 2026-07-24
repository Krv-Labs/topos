//! Coverage tool — structural test coverage (UAST).

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::functors::profunctors::uast::structural_test_coverage::declaration_coverage;
use topos_engine::graphs::uast::models::UASTNode;

use crate::formatting::to_tool_result;
use crate::schemas::{CalculateCoverageInput, CoverageResult};
use crate::security::resolve_within_root;
use crate::server::ToposServer;

fn empty_coverage_result(error: String) -> CoverageResult {
    CoverageResult {
        mean_declaration_coverage: 0.0,
        best_declaration_recall: Vec::new(),
        declaration_locations: Vec::new(),
        stmt_recall: 0.0,
        expr_recall: 0.0,
        mean_test_precision: 0.0,
        f2_score: 0.0,
        declaration_path_recall_kgram: 0.0,
        uncovered_declarations: Vec::new(),
        put_declaration_count: 0,
        test_declaration_count: 0,
        warnings: Vec::new(),
        error: Some(error),
    }
}

enum ParseOutcome {
    Roots(Vec<ProgramMorphism>),
    Error(CoverageResult),
}

fn parse_roots(files: &[String], language: &str, label: &str) -> ParseOutcome {
    let mut morphisms = Vec::new();
    for path in files {
        let resolved = match resolve_within_root(path) {
            Ok(p) => p,
            Err(err) => {
                return ParseOutcome::Error(empty_coverage_result(format!(
                    "{label} file error: {err} for {path}"
                )))
            }
        };
        match ProgramMorphism::from_file(&resolved, "python") {
            Ok(morphism) => morphisms.push(morphism),
            Err(exc) => {
                return ParseOutcome::Error(empty_coverage_result(format!(
                    "Failed to parse {label} file {path}: {exc}"
                )))
            }
        }
    }
    let _ = language;
    ParseOutcome::Roots(morphisms)
}

fn render_uncovered(r: &CoverageResult) -> Vec<String> {
    let mut lines = Vec::new();
    if r.uncovered_declarations.is_empty() {
        lines.push("## ✅ 100% Structural Coverage".to_string());
        lines.push(
            "All declarations in the PUT are structurally represented in the test suite."
                .to_string(),
        );
    } else {
        lines.push("## Uncovered Declarations".to_string());
        for loc in &r.uncovered_declarations {
            if let Some(idx) = r.declaration_locations.iter().position(|l| l == loc) {
                let recall = r.best_declaration_recall.get(idx).copied().unwrap_or(0.0);
                lines.push(format!("- `{loc}` ({:.1}%)", recall * 100.0));
            } else {
                lines.push(format!("- `{loc}`"));
            }
        }
    }
    lines
}

pub(crate) fn render_coverage_md(r: &CoverageResult) -> String {
    let mut lines = vec![
        "# Structural Test Coverage (UAST)".to_string(),
        String::new(),
    ];
    if let Some(err) = &r.error {
        lines.push(format!("> ⚠️ **Error:** {err}"));
        return lines.join("\n");
    }
    lines.push(format!(
        "**Mean Declaration Coverage:** {:.1}%",
        r.mean_declaration_coverage * 100.0
    ));
    lines.push(format!("**F2 Score (Recall-weighted):** {:.3}", r.f2_score));
    lines.push(format!(
        "**Test Suite Precision:** {:.1}%",
        r.mean_test_precision * 100.0
    ));
    lines.push(String::new());
    lines.push("## Stratified Recall".to_string());
    lines.push(format!("- **Statements:** {:.1}%", r.stmt_recall * 100.0));
    lines.push(format!("- **Expressions:** {:.1}%", r.expr_recall * 100.0));
    lines.push(format!(
        "- **Paths (k-gram):** {:.1}%",
        r.declaration_path_recall_kgram * 100.0
    ));
    lines.push(String::new());
    lines.push("## Corpus Statistics".to_string());
    lines.push(format!(
        "- **PUT Declarations:** {}",
        r.put_declaration_count
    ));
    lines.push(format!(
        "- **Test Declarations:** {}",
        r.test_declaration_count
    ));
    lines.push(String::new());
    lines.extend(render_uncovered(r));
    lines.join("\n")
}

#[tool_router(router = coverage_router, vis = "pub(crate)")]
impl ToposServer {
    /// Measure how well a test suite exercises its program-under-test, via
    /// structural (UAST) coverage (read-only).
    ///
    /// A standalone signal, separate from the SIMPLE/COMPOSABLE/SECURE
    /// lattice; for a quality verdict use `topos_evaluate_*` instead.
    /// Computes UAST bipartite declaration matching and k-gram path
    /// recall. Returns a CoverageResult.
    #[tool(
        name = "topos_calculate_coverage",
        annotations(
            title = "Topos Structural Coverage",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_calculate_coverage(
        &self,
        Parameters(params): Parameters<CalculateCoverageInput>,
    ) -> CallToolResult {
        let put_morphisms = match parse_roots(&params.put_files, &params.language, "PUT") {
            ParseOutcome::Roots(m) => m,
            ParseOutcome::Error(model) => {
                let md = render_coverage_md(&model);
                return to_tool_result(&model, md);
            }
        };
        let test_morphisms = match parse_roots(&params.test_files, &params.language, "Test") {
            ParseOutcome::Roots(m) => m,
            ParseOutcome::Error(model) => {
                let md = render_coverage_md(&model);
                return to_tool_result(&model, md);
            }
        };

        let put_roots: Vec<&UASTNode> = put_morphisms
            .iter()
            .filter_map(|m| m.ast.as_ref())
            .map(|ast| &ast.uast_root)
            .collect();
        let test_roots: Vec<&UASTNode> = test_morphisms
            .iter()
            .filter_map(|m| m.ast.as_ref())
            .map(|ast| &ast.uast_root)
            .collect();

        if put_roots.is_empty() {
            let model = empty_coverage_result(
                "No valid PUT roots found (parsing failed or files empty).".to_string(),
            );
            let md = render_coverage_md(&model);
            return to_tool_result(&model, md);
        }

        match declaration_coverage(&put_roots, &test_roots, params.k, params.include_unknown) {
            Ok(report) => {
                let model = CoverageResult {
                    mean_declaration_coverage: report.mean_declaration_coverage,
                    best_declaration_recall: report.best_declaration_recall.clone(),
                    declaration_locations: report.declaration_locations.clone(),
                    stmt_recall: report.stmt_recall,
                    expr_recall: report.expr_recall,
                    mean_test_precision: report.mean_test_precision,
                    f2_score: report.f2_score(),
                    declaration_path_recall_kgram: report.declaration_path_recall_kgram,
                    uncovered_declarations: report.uncovered_declarations(),
                    put_declaration_count: report.put_declaration_count,
                    test_declaration_count: report.test_declaration_count,
                    warnings: Vec::new(),
                    error: None,
                };
                let md = render_coverage_md(&model);
                to_tool_result(&model, md)
            }
            Err(exc) => {
                let model = empty_coverage_result(format!("Coverage calculation failed: {exc:?}"));
                let md = render_coverage_md(&model);
                to_tool_result(&model, md)
            }
        }
    }
}
