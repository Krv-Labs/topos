//! Unified refactoring-guidance suite (Methods Upgrade milestone).
//!
//! One tool, three targets (ranked hotspot list + suggested action), each
//! surfacing a different structural-analysis engine:
//!
//! - `target="cycles"`: cycle-basis extraction on the CFG.
//! - `target="dependencies"`: balanced Forman curvature on the MDG.
//! - `target="process"`: directed Forman-Ricci curvature on GitNexus
//!   process graphs.
//!
//! All three are purely advisory — none of this feeds
//! SIMPLE/COMPOSABLE/SECURE/NAVIGABLE scoring.

use std::sync::atomic::AtomicBool;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{tool, tool_router, ErrorData as McpError};
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::functors::probes::cfg::homology::calculate_cycle_basis;
use topos_engine::functors::probes::mdg::curvature::calculate_mdg_curvature;
use topos_engine::functors::probes::process::curvature::calculate_process_curvature;
use topos_engine::graphs::process::object::ProcessGraph;

use crate::evaluation::{
    detect_language, load_dep_graph, resolve_gitnexus_dir, resolve_mcp_composable_project_root,
    resolve_override_for_root,
};
use crate::execution::{is_cancelled, EvalCancelled};
use crate::formatting::to_tool_result;
use crate::refactor_hotspots::render_hotspots_md;
use crate::schemas::{RefactorHotspot, RefactorInput, RefactorResult, RefactorTargetKind};
use crate::security::{
    composable_default_root, read_safe_utf8_file, resolve_project_path, resolve_within_root,
};
use crate::server::ToposServer;

fn err_result(target: RefactorTargetKind, filepath: &str, error: String) -> RefactorResult {
    RefactorResult {
        target: target.as_str().to_string(),
        filepath: filepath.to_string(),
        betti_1: None,
        gitnexus_available: None,
        tool_available: None,
        hotspots: Vec::new(),
        error: Some(error),
    }
}

fn err_response(
    target: RefactorTargetKind,
    title: &str,
    filepath: &str,
    error: String,
) -> (RefactorResult, String) {
    let md = format!("## {title}\n\n> ⚠️ **Error:** {error}");
    let model = err_result(target, filepath, error);
    (model, md)
}

fn refactor_cycles(
    params: &RefactorInput,
    cancel_flag: Option<&AtomicBool>,
) -> Result<(RefactorResult, String), EvalCancelled> {
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    let resolved = match resolve_within_root(&params.filepath) {
        Ok(path) => path,
        Err(err) => {
            return Ok(err_response(
                RefactorTargetKind::Cycles,
                "Cycle hotspots",
                &params.filepath,
                err,
            ));
        }
    };
    let source = match read_safe_utf8_file(&resolved.to_string_lossy()) {
        Ok(source) => source,
        Err(err) => {
            return Ok(err_response(
                RefactorTargetKind::Cycles,
                "Cycle hotspots",
                &params.filepath,
                err,
            ));
        }
    };
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }

    let language = detect_language(&resolved);
    let mut morphism = ProgramMorphism::new(&source, language);
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    let Some(cfg) = morphism.build_cfg().cloned() else {
        return Ok(err_response(
            RefactorTargetKind::Cycles,
            "Cycle hotspots",
            &params.filepath,
            "Could not build a control-flow graph.".to_string(),
        ));
    };
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }

    let limit = params.limit.clamp(1, 50);
    let result = calculate_cycle_basis(&cfg);
    let mut ranked = result.cycles.clone();
    ranked.sort_by_key(|b| std::cmp::Reverse(span(b)));
    ranked.truncate(limit);

    let hotspots: Vec<RefactorHotspot> = ranked
        .iter()
        .map(|cycle| RefactorHotspot {
            kind: "cycle".to_string(),
            label: format!("cycle over blocks {:?}", cycle.block_ids),
            filepath: params.filepath.clone(),
            line_start: cycle.start_line,
            line_end: cycle.end_line,
            score: span(cycle) as f64,
            suggestion: "Extract this loop/branch body into its own function to isolate the \
                         cycle and shrink cyclomatic complexity."
                .to_string(),
        })
        .collect();

    let title = format!("Cycle hotspots (betti_1={})", result.betti_1);
    let md = render_hotspots_md(&title, &hotspots);
    let model = RefactorResult {
        target: "cycles".to_string(),
        filepath: params.filepath.clone(),
        betti_1: Some(result.betti_1),
        gitnexus_available: None,
        tool_available: None,
        hotspots,
        error: None,
    };
    Ok((model, md))
}

fn span(cycle: &topos_engine::functors::probes::cfg::homology::SourceCycle) -> usize {
    match (cycle.start_line, cycle.end_line) {
        (Some(start), Some(end)) => end.saturating_sub(start),
        _ => 0,
    }
}

fn refactor_dependencies(
    params: &RefactorInput,
    cancel_flag: Option<&AtomicBool>,
) -> Result<(RefactorResult, String), EvalCancelled> {
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    let (_path, detected_project) = match resolve_project_path(&params.filepath) {
        Ok(context) => context,
        Err(err) => {
            return Ok(err_response(
                RefactorTargetKind::Dependencies,
                "Dependency hotspots",
                &params.filepath,
                err,
            ));
        }
    };
    let composable_root = composable_default_root(&detected_project);
    let project_root =
        resolve_mcp_composable_project_root(params.gitnexus_dir.as_deref(), &composable_root);
    let resolved_override =
        resolve_override_for_root(params.gitnexus_dir.as_deref(), &composable_root);
    let gitnexus_dir = resolve_gitnexus_dir(resolved_override.as_deref(), &project_root);
    let (mdg, _) = load_dep_graph(gitnexus_dir.as_deref(), &params.filepath);
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    let Some(mdg) = mdg else {
        let model = RefactorResult {
            target: "dependencies".to_string(),
            filepath: params.filepath.clone(),
            betti_1: None,
            gitnexus_available: Some(false),
            tool_available: Some(false),
            hotspots: Vec::new(),
            error: None,
        };
        return Ok((model, render_hotspots_md("Dependency hotspots", &[])));
    };

    let Some(file_id) = mdg.file_node_id().map(str::to_string) else {
        let model = RefactorResult {
            target: "dependencies".to_string(),
            filepath: params.filepath.clone(),
            betti_1: None,
            gitnexus_available: Some(true),
            tool_available: Some(true),
            hotspots: Vec::new(),
            error: None,
        };
        return Ok((model, render_hotspots_md("Dependency hotspots", &[])));
    };

    let limit = params.limit.clamp(1, 50);
    let curvature = calculate_mdg_curvature(&mdg, &file_id, "IMPORTS");
    let hotspots: Vec<RefactorHotspot> = curvature
        .edges
        .iter()
        .take(limit)
        .map(|(src, dst, score)| RefactorHotspot {
            kind: "dependency_edge".to_string(),
            label: format!("{src} -> {dst}"),
            filepath: params.filepath.clone(),
            line_start: None,
            line_end: None,
            score: *score,
            suggestion: if *score < 0.0 {
                "Highly negative curvature: many otherwise-unrelated modules route coupling \
                 through this edge. Consider extracting a shared interface or reducing the \
                 shared surface to strengthen it."
                    .to_string()
            } else {
                "Well-supported dependency edge; no action needed.".to_string()
            },
        })
        .collect();

    let md = render_hotspots_md("Dependency hotspots", &hotspots);
    let model = RefactorResult {
        target: "dependencies".to_string(),
        filepath: params.filepath.clone(),
        betti_1: None,
        gitnexus_available: Some(true),
        tool_available: Some(true),
        hotspots,
        error: None,
    };
    Ok((model, md))
}

fn refactor_process(
    params: &RefactorInput,
    cancel_flag: Option<&AtomicBool>,
) -> Result<(RefactorResult, String), EvalCancelled> {
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    let (_path, detected_project) = match resolve_project_path(&params.filepath) {
        Ok(context) => context,
        Err(err) => {
            return Ok(err_response(
                RefactorTargetKind::Process,
                "Process choke points",
                &params.filepath,
                err,
            ));
        }
    };
    let composable_root = composable_default_root(&detected_project);
    let project_root =
        resolve_mcp_composable_project_root(params.gitnexus_dir.as_deref(), &composable_root);
    let resolved_override =
        resolve_override_for_root(params.gitnexus_dir.as_deref(), &composable_root);
    let gitnexus_dir = resolve_gitnexus_dir(resolved_override.as_deref(), &project_root);
    let (mdg, _) = load_dep_graph(gitnexus_dir.as_deref(), &params.filepath);
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    let Some(mdg) = mdg else {
        let model = RefactorResult {
            target: "process".to_string(),
            filepath: params.filepath.clone(),
            betti_1: None,
            gitnexus_available: Some(false),
            tool_available: Some(false),
            hotspots: Vec::new(),
            error: None,
        };
        return Ok((model, render_hotspots_md("Process choke points", &[])));
    };

    let Some(file_id) = mdg.file_node_id().map(str::to_string) else {
        let model = RefactorResult {
            target: "process".to_string(),
            filepath: params.filepath.clone(),
            betti_1: None,
            gitnexus_available: Some(true),
            tool_available: Some(true),
            hotspots: Vec::new(),
            error: None,
        };
        return Ok((model, render_hotspots_md("Process choke points", &[])));
    };

    let process_graph = ProcessGraph::from_mdg(&mdg, params.filepath.clone());
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    let touching = process_graph.paths_touching_file(&file_id);
    let subgraph = ProcessGraph::from_paths(params.filepath.clone(), touching);
    let curvature = calculate_process_curvature(&subgraph, None);

    let limit = params.limit.clamp(1, 50);
    let hotspots: Vec<RefactorHotspot> = curvature
        .edges
        .iter()
        .take(limit)
        .map(|(src, dst, score)| RefactorHotspot {
            kind: "process_transition".to_string(),
            label: format!("{src} -> {dst}"),
            filepath: params.filepath.clone(),
            line_start: None,
            line_end: None,
            score: *score,
            suggestion: if *score < 0.0 {
                "Choke point: many independent execution paths squeeze through this \
                 transition. Consider an asynchronous decoupling boundary (message queue / \
                 pub-sub), or acknowledge the simplicity trade-off of keeping it."
                    .to_string()
            } else {
                "Well-distributed transition; no action needed.".to_string()
            },
        })
        .collect();

    let md = render_hotspots_md("Process choke points", &hotspots);
    let model = RefactorResult {
        target: "process".to_string(),
        filepath: params.filepath.clone(),
        betti_1: None,
        gitnexus_available: Some(true),
        tool_available: Some(true),
        hotspots,
        error: None,
    };
    Ok((model, md))
}

fn refactor_with_cancel(
    params: &RefactorInput,
    cancel_flag: Option<&AtomicBool>,
) -> Result<CallToolResult, EvalCancelled> {
    let (model, md) = match params.target {
        RefactorTargetKind::Cycles => refactor_cycles(params, cancel_flag)?,
        RefactorTargetKind::Dependencies => refactor_dependencies(params, cancel_flag)?,
        RefactorTargetKind::Process => refactor_process(params, cancel_flag)?,
    };
    Ok(to_tool_result(&model, md))
}

#[tool_router(router = refactor_router, vis = "pub(crate)")]
impl ToposServer {
    /// Rank structural refactor hotspots for one file (read-only, advisory).
    ///
    /// Does not score the four pillars — use `topos_evaluate_*` for
    /// medals and `topos_assess_*` to verify edits afterward. `target`
    /// selects the engine: `cycles` (CFG loop/branch bodies),
    /// `dependencies` (MDG Forman curvature on imports; needs `.gitnexus`),
    /// or `process` (execution choke points; needs `.gitnexus`). Returns a
    /// RefactorResult with ranked `hotspots` (`kind`, `label`, `score`,
    /// `suggestion`, optional lines).
    #[tool(
        name = "topos_refactor",
        annotations(
            title = "Topos Refactor Suggestions",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn topos_refactor(
        &self,
        Parameters(params): Parameters<RefactorInput>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.executor()
            .run_inline(&context, move |cancel_flag| {
                refactor_with_cancel(&params, Some(cancel_flag))
            })
            .await
    }
}
