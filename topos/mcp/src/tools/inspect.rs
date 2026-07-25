//! Detailed inspection of a code unit — every metric exposed, function
//! table, entropy breakdown.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::policies::simple::describe_entropy_ratio;
use topos_engine::functors::probes::ast::complexity::calculate_function_complexity_entries;
use topos_engine::functors::probes::ast::entropy::calculate_kolmogorov_proxy;

use crate::diagnostics::overlay_for_source;
use crate::evaluation::{
    classify_code_string, classify_file, detect_language, ensure_gitnexus_dir, gitnexus_warnings,
};
use crate::formatting::{
    render_evaluation_md, to_evaluation_result, to_tool_result, EvalResultOptions,
};
use crate::metric_locations::{build_metric_locations, function_entry_from_complexity};
use crate::schemas::{
    resolve_priority, EvaluationResult, FunctionEntry, InspectCodeInput, InspectionResult,
    PrioritySource,
};
use crate::security::{read_safe_utf8_file, resolve_file_root, resolve_within_root};
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

/// A classified inspection subject, plus the COMPOSABLE plumbing that
/// produced it: whether a ModuleDependencyGraph actually made it into the
/// classification, and the warnings explaining it when one didn't.
struct InspectClassification {
    result: ClassificationResult,
    coupling_available: bool,
    warnings: Vec<String>,
}

/// Classify a file the way `topos_evaluate_file` does — same
/// [`ensure_gitnexus_dir`] resolve-or-generate decision, same
/// [`classify_file`] attachment, same [`gitnexus_warnings`] explanation — so
/// the two tools cannot report different medals for the same source
/// (issue #216: inspect used to skip the MDG entirely and read one medal
/// lower).
fn classify_inspected_file(
    params: &InspectCodeInput,
    project_root: &Path,
    path: &Path,
    priority: Priority,
) -> Result<InspectClassification, String> {
    let outcome = ensure_gitnexus_dir(
        params.gitnexus_dir.as_deref(),
        project_root,
        params.no_composable,
        /* capture = */ true,
    );
    let gitnexus_dir = outcome.gitnexus_dir;

    let (result, dep_graph, load_error) = classify_file(path, priority, gitnexus_dir.as_deref())?;
    let mut warnings = gitnexus_warnings(
        params.gitnexus_dir.as_deref(),
        project_root,
        gitnexus_dir.as_deref(),
        dep_graph.is_some(),
        load_error.as_deref(),
    );
    if let Some(note) = outcome.generation_note {
        warnings.insert(0, note);
    }
    Ok(InspectClassification {
        result,
        coupling_available: dep_graph.is_some(),
        warnings,
    })
}

/// Classify whichever subject was loaded: a file (COMPOSABLE reachable) or
/// an inline `code` string (it isn't).
fn classify_inspection(
    params: &InspectCodeInput,
    loaded: &LoadedSource,
    language: &str,
    priority: Priority,
) -> Result<InspectClassification, String> {
    // An inline string has no module for the dependency graph to key on, so
    // COMPOSABLE is out of reach exactly as in `topos_evaluate_code`.
    // Returning before `ensure_gitnexus_dir` also keeps a pure string call
    // from shelling out to `gitnexus analyze` for a graph nothing would read.
    let Some(path) = loaded.file_path.as_deref() else {
        return Ok(InspectClassification {
            result: classify_code_string(&loaded.source, language, priority)?,
            coupling_available: false,
            warnings: Vec::new(),
        });
    };
    let project_root = resolve_file_root()?;
    classify_inspected_file(params, &project_root, path, priority)
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
    /// Provide exactly one of `code` or `filepath`. Use when you need the
    /// per-function detail behind a verdict; use `topos_evaluate_*` when the
    /// medal alone is enough. Returns an InspectionResult: the lattice
    /// `evaluation`, a *top-N* function complexity table
    /// (`top_n_functions`, default 10), `total_functions`, and entropy
    /// details.
    ///
    /// With `filepath`, the verdict is scored on all three generators and
    /// agrees with `topos_evaluate_file`: unless `no_composable` is set,
    /// this generates/refreshes `.gitnexus` (given by `gitnexus_dir` or
    /// auto-detected at `<root>/.gitnexus`) when missing or stale, then
    /// attaches the ModuleDependencyGraph — so this tool is side-effecting.
    /// With inline `code` there is no module to place in the graph, so only
    /// SIMPLE/SECURE are reachable, as in `topos_evaluate_code`.
    #[tool(
        name = "topos_inspect_code",
        annotations(
            title = "Topos Detailed Inspection",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn topos_inspect_code(
        &self,
        Parameters(params): Parameters<InspectCodeInput>,
    ) -> CallToolResult {
        // Generation can shell out to `gitnexus analyze` (bounded by
        // TOPOS_DEPGRAPH_TIMEOUT, default 300s); offload so a slow/first-time
        // run cannot stall the transport, matching topos_evaluate_file.
        match tokio::task::spawn_blocking(move || inspect_code_sync(params)).await {
            Ok(tool_result) => tool_result,
            Err(join_err) => err_inspection(
                Priority::Simple,
                PrioritySource::Default,
                format!("inspection panicked: {join_err}"),
            ),
        }
    }
}

fn inspect_code_sync(params: InspectCodeInput) -> CallToolResult {
    let (priority, priority_source) = resolve_priority(params.preferences.as_ref());

    let loaded = match load_source(&params) {
        Ok(loaded) => loaded,
        Err(msg) => return err_inspection(priority, priority_source, msg),
    };
    let language = inspection_language(&params, loaded.file_path.as_ref());

    let classified = match classify_inspection(&params, &loaded, &language, priority) {
        Ok(classified) => classified,
        Err(exc) => return err_inspection(priority, priority_source, exc),
    };
    let result = classified.result;

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
    opts.warnings = classified.warnings;
    opts.adjusted_verdict = overlay.as_ref().map(|o| &o.verdict);
    overlay_opts(overlay.as_ref(), &mut opts);
    opts.verbose = params.verbose;
    opts.metric_locations = build_metric_locations(&loaded.source, &language, &result);
    let evaluation = to_evaluation_result(&result, classified.coupling_available, opts);

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

#[cfg(test)]
mod tests {
    use super::*;

    fn params_from(json: serde_json::Value) -> InspectCodeInput {
        serde_json::from_value(json).expect("deserialize InspectCodeInput")
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_mcp_inspect_test_{label}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// The `code`-string mode has no file to key a dependency graph on, so it
    /// must classify without ever consulting (let alone generating) GitNexus
    /// — same reachable pillars as `topos_evaluate_code`. Anything else
    /// would turn a pure string call into a subprocess.
    ///
    /// The knobs are sent explicitly here so the test pins the real gate —
    /// "no file path" — rather than passing by accident because the caller
    /// happened to leave `no_composable` at its default.
    #[test]
    fn inline_code_is_classified_without_gitnexus() {
        let params = params_from(serde_json::json!({
            "code": "def f():\n    return 1\n",
            "gitnexus_dir": "/nonexistent/.gitnexus",
            "no_composable": false,
        }));
        let loaded = load_source(&params).expect("inline code loads");
        assert!(loaded.file_path.is_none());

        let classified = classify_inspection(&params, &loaded, "python", Priority::Simple)
            .expect("inline classification runs");
        assert!(classified.result.is_parseable);
        assert!(
            !classified.coupling_available,
            "COMPOSABLE is unreachable without a file"
        );
        assert!(
            classified.warnings.is_empty(),
            "a string call has no gitnexus state to warn about, got {:?}",
            classified.warnings
        );
    }

    /// End-to-end on the string path: no filepath, no panic, and a rendered
    /// inspection rather than an error result.
    #[test]
    fn inline_code_inspection_succeeds_end_to_end() {
        let result = inspect_code_sync(params_from(serde_json::json!({
            "code": "def f(x):\n    if x:\n        return 1\n    return 0\n"
        })));
        assert_ne!(
            result.is_error,
            Some(true),
            "inline inspection must not error"
        );
    }

    /// `no_composable` skips *generation*: with no `.gitnexus` under the
    /// project root there is nothing to resolve either, so COMPOSABLE stays
    /// unscored and the warning says how to fix it — all without shelling
    /// out, which is what makes this deterministic on any machine.
    #[test]
    fn no_composable_skips_generation_and_leaves_coupling_unavailable() {
        let project_root = temp_dir("no_composable_root");
        let file = project_root.join("sample.py");
        std::fs::write(&file, "def f():\n    return 1\n").expect("write sample");

        let params = params_from(serde_json::json!({
            "filepath": "sample.py",
            "no_composable": true,
        }));
        let classified =
            classify_inspected_file(&params, &project_root, &file, Priority::Simple).expect("runs");

        assert!(!classified.coupling_available);
        assert!(
            classified
                .warnings
                .iter()
                .any(|w| w.contains("no .gitnexus directory found")),
            "expected the shared gitnexus_warnings explanation, got {:?}",
            classified.warnings
        );

        std::fs::remove_dir_all(&project_root).ok();
    }
}
