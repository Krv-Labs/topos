//! Detailed inspection of a code unit — every metric exposed, function
//! table, entropy breakdown.

use std::collections::HashMap;
use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::policies::simple::describe_entropy_ratio;
use topos_engine::functors::probes::ast::complexity::calculate_function_complexity_entries;
use topos_engine::functors::probes::ast::entropy::calculate_kolmogorov_proxy;

use crate::diagnostics::overlay_for_source;
use crate::evaluation::{classify_code_string, detect_language};
use crate::formatting::{
    render_evaluation_md, to_evaluation_result, to_tool_result, EvalResultOptions,
};
use crate::metric_locations::{build_metric_locations, function_entry_from_complexity};
use crate::schemas::{
    resolve_priority, EvaluationResult, FunctionEntry, InspectCodeInput, InspectionResult,
    PrioritySource,
};
use crate::security::{read_safe_utf8_file, resolve_within_root};
use crate::server::ToposServer;
use crate::tools::evaluate::overlay_opts;

struct LoadedSource {
    source: String,
    file_path: Option<PathBuf>,
}

fn load_source(params: &InspectCodeInput) -> Result<LoadedSource, String> {
    match (&params.code, &params.filepath) {
        (Some(_), Some(_)) | (None, None) => {
            Err("Provide exactly one of `code` or `filepath`.".to_string())
        }
        (Some(code), None) => Ok(LoadedSource {
            source: code.clone(),
            file_path: None,
        }),
        (None, Some(filepath)) => {
            let resolved = resolve_within_root(filepath)?;
            let source = read_safe_utf8_file(filepath)?;
            Ok(LoadedSource {
                source,
                file_path: Some(resolved),
            })
        }
    }
}

fn inspection_language(params: &InspectCodeInput, file_path: Option<&PathBuf>) -> String {
    match file_path {
        Some(path) => detect_language(path).to_string(),
        None => params.language.clone(),
    }
}

fn err_inspection(
    priority: Priority,
    priority_source: PrioritySource,
    msg: String,
) -> CallToolResult {
    let empty =
        EvaluationResult::error_result("evaluation failed", priority, priority_source, msg.clone());
    let model = InspectionResult {
        evaluation: empty,
        functions: HashMap::new(),
        function_entries: Vec::new(),
        total_functions: 0,
        entropy_compression_ratio: None,
        entropy_interpretation: None,
        error: Some(msg),
    };
    let md = render_inspection_md(&model, true);
    to_tool_result(&model, md)
}

pub(crate) fn render_inspection_md(r: &InspectionResult, verbose: bool) -> String {
    if let Some(err) = &r.error {
        return format!("**Error:** {err}");
    }
    let e = &r.evaluation;
    let mut lines = vec![
        format!(
            "**Lattice:** {} {}",
            e.lattice_symbol,
            e.lattice_element.as_str()
        ),
        format!("**Total functions:** {}", r.total_functions),
    ];
    if !r.function_entries.is_empty() {
        lines.push(String::new());
        lines.push("## Top functions (by complexity)".to_string());
        lines.push("| Function | Line | Complexity |".to_string());
        lines.push("| --- | ---: | ---: |".to_string());
        for fn_entry in &r.function_entries {
            let safe_name = fn_entry.name.replace(['\n', '\r'], " ").replace('|', "\\|");
            lines.push(format!(
                "| `{safe_name}` | {} | {} |",
                fn_entry.line, fn_entry.complexity
            ));
        }
    }
    if let Some(ratio) = r.entropy_compression_ratio {
        lines.push(String::new());
        let interp = r
            .entropy_interpretation
            .as_ref()
            .map(|i| format!(" — {i}"))
            .unwrap_or_default();
        lines.push(format!("**Entropy compression ratio:** {ratio:.3}{interp}"));
    }
    lines.push(String::new());
    lines.push(render_evaluation_md(e, Some("Evaluation"), verbose));
    lines.join("\n")
}

#[tool_router(router = inspect_router, vis = "pub(crate)")]
impl ToposServer {
    /// Full metric breakdown for a single code unit (inline string or
    /// file).
    ///
    /// Read-only; provide exactly one of `code` or `filepath`. Use when you
    /// need the per-function detail behind a verdict; use `topos_evaluate_*`
    /// when the medal alone is enough. Returns an InspectionResult: the
    /// lattice `evaluation`, a *top-N* function complexity table
    /// (`top_n_functions`, default 10), `total_functions`, and entropy
    /// details.
    #[tool(
        name = "topos_inspect_code",
        annotations(
            title = "Topos Detailed Inspection",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_inspect_code(
        &self,
        Parameters(params): Parameters<InspectCodeInput>,
    ) -> CallToolResult {
        let (priority, priority_source) = resolve_priority(params.preferences.as_ref());

        let loaded = match load_source(&params) {
            Ok(loaded) => loaded,
            Err(msg) => return err_inspection(priority, priority_source, msg),
        };
        let language = inspection_language(&params, loaded.file_path.as_ref());

        let result = match classify_code_string(&loaded.source, &language, priority) {
            Ok(result) => result,
            Err(exc) => return err_inspection(priority, priority_source, exc),
        };

        let prefs = match params.preferences.as_ref().map(|p| p.to_preferences()) {
            Some(Err(exc)) => return err_inspection(priority, priority_source, exc),
            Some(Ok(p)) => Some(p),
            None => None,
        };

        let overlay = overlay_for_source(
            &loaded.source,
            &language,
            &result,
            loaded.file_path.as_deref(),
            &params.allow,
        );
        let mut opts = EvalResultOptions::new();
        opts.preferences = prefs.as_ref();
        opts.priority_source = priority_source;
        opts.adjusted_verdict = overlay.as_ref().map(|o| &o.verdict);
        overlay_opts(overlay.as_ref(), &mut opts);
        opts.verbose = params.verbose;
        opts.metric_locations = build_metric_locations(&loaded.source, &language, &result);
        let evaluation = to_evaluation_result(&result, false, opts);

        // Use the same AST decision-node probe that feeds
        // `ast.max_function_complexity` so this table never disagrees with
        // the failing gate.
        let morphism = ProgramMorphism::new(&loaded.source, &language);
        let mut all_funcs: Vec<FunctionEntry> = Vec::new();
        if let Some(ast) = morphism.ast.as_ref() {
            if morphism.is_valid() {
                all_funcs = calculate_function_complexity_entries(&ast.uast_root, &loaded.source)
                    .iter()
                    .map(|fc| function_entry_from_complexity(fc, "ast"))
                    .collect();
            }
        }

        let mut top_entries = all_funcs.clone();
        top_entries.sort_by_key(|e| std::cmp::Reverse(e.complexity));
        top_entries.truncate(params.top_n_functions);
        let top_funcs: HashMap<String, i64> = top_entries
            .iter()
            .map(|e| (e.name.clone(), e.complexity))
            .collect();

        let ratio = calculate_kolmogorov_proxy(&morphism.source);
        let interpretation = describe_entropy_ratio(ratio);

        let model = InspectionResult {
            evaluation,
            functions: top_funcs,
            function_entries: top_entries,
            total_functions: all_funcs.len(),
            entropy_compression_ratio: Some(ratio),
            entropy_interpretation: Some(interpretation),
            error: None,
        };
        let md = render_inspection_md(&model, params.verbose);
        to_tool_result(&model, md)
    }
}
