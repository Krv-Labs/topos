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
//! SIMPLE/COMPOSABLE/SECURE scoring.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_core::core::morphism::ProgramMorphism;
use topos_core::functors::probes::cfg::homology::calculate_cycle_basis;
use topos_core::functors::probes::mdg::curvature::calculate_mdg_curvature;
use topos_core::functors::probes::process::curvature::calculate_process_curvature;
use topos_core::graphs::process::object::ProcessGraph;

use crate::evaluation::{detect_language, load_dep_graph, resolve_gitnexus_dir};
use crate::formatting::to_tool_result;
use crate::refactor_hotspots::render_hotspots_md;
use crate::schemas::{RefactorHotspot, RefactorInput, RefactorResult, RefactorTargetKind};
use crate::security::{read_safe_utf8_file, resolve_file_root, resolve_within_root};
use crate::server::ToposServer;

fn err_result(target: RefactorTargetKind, filepath: &str, error: String) -> RefactorResult {
    RefactorResult {
        target: target.as_str().to_string(),
        filepath: filepath.to_string(),
        betti_1: None,
        gitnexus_available: None,
        hotspots: Vec::new(),
        error: Some(error),
    }
}

fn refactor_cycles(params: &RefactorInput) -> (RefactorResult, String) {
    let resolved = match resolve_within_root(&params.filepath) {
        Ok(path) => path,
        Err(err) => {
            let model = err_result(RefactorTargetKind::Cycles, &params.filepath, err);
            return (model, render_hotspots_md("Cycle hotspots", &[]));
        }
    };
    let source = match read_safe_utf8_file(&resolved.to_string_lossy()) {
        Ok(source) => source,
        Err(err) => {
            let model = err_result(RefactorTargetKind::Cycles, &params.filepath, err);
            return (model, render_hotspots_md("Cycle hotspots", &[]));
        }
    };

    let language = detect_language(&resolved);
    let mut morphism = ProgramMorphism::new(&source, language);
    let Some(cfg) = morphism.build_cfg().cloned() else {
        let model = err_result(
            RefactorTargetKind::Cycles,
            &params.filepath,
            "Could not build a control-flow graph.".to_string(),
        );
        return (model, render_hotspots_md("Cycle hotspots", &[]));
    };

    let result = calculate_cycle_basis(&cfg);
    let mut ranked = result.cycles.clone();
    ranked.sort_by_key(|b| std::cmp::Reverse(span(b)));
    ranked.truncate(params.limit);

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
        hotspots,
        error: None,
    };
    (model, md)
}

fn span(cycle: &topos_core::functors::probes::cfg::homology::SourceCycle) -> usize {
    match (cycle.start_line, cycle.end_line) {
        (Some(start), Some(end)) => end.saturating_sub(start),
        _ => 0,
    }
}

fn refactor_dependencies(params: &RefactorInput) -> (RefactorResult, String) {
    let project_root = match resolve_file_root() {
        Ok(root) => root,
        Err(err) => {
            let model = err_result(RefactorTargetKind::Dependencies, &params.filepath, err);
            return (model, render_hotspots_md("Dependency hotspots", &[]));
        }
    };
    let gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir.as_deref(), &project_root);
    let (mdg, _) = load_dep_graph(gitnexus_dir.as_deref(), &params.filepath);
    let Some(mdg) = mdg else {
        let model = RefactorResult {
            target: "dependencies".to_string(),
            filepath: params.filepath.clone(),
            betti_1: None,
            gitnexus_available: Some(false),
            hotspots: Vec::new(),
            error: None,
        };
        return (model, render_hotspots_md("Dependency hotspots", &[]));
    };

    let Some(file_id) = mdg.file_node_id().map(str::to_string) else {
        let model = RefactorResult {
            target: "dependencies".to_string(),
            filepath: params.filepath.clone(),
            betti_1: None,
            gitnexus_available: Some(true),
            hotspots: Vec::new(),
            error: None,
        };
        return (model, render_hotspots_md("Dependency hotspots", &[]));
    };

    let curvature = calculate_mdg_curvature(&mdg, &file_id, "IMPORTS");
    let hotspots: Vec<RefactorHotspot> = curvature
        .edges
        .iter()
        .take(params.limit)
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
        hotspots,
        error: None,
    };
    (model, md)
}

fn refactor_process(params: &RefactorInput) -> (RefactorResult, String) {
    let project_root = match resolve_file_root() {
        Ok(root) => root,
        Err(err) => {
            let model = err_result(RefactorTargetKind::Process, &params.filepath, err);
            return (model, render_hotspots_md("Process choke points", &[]));
        }
    };
    let gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir.as_deref(), &project_root);
    let (mdg, _) = load_dep_graph(gitnexus_dir.as_deref(), &params.filepath);
    let Some(mdg) = mdg else {
        let model = RefactorResult {
            target: "process".to_string(),
            filepath: params.filepath.clone(),
            betti_1: None,
            gitnexus_available: Some(false),
            hotspots: Vec::new(),
            error: None,
        };
        return (model, render_hotspots_md("Process choke points", &[]));
    };

    let Some(file_id) = mdg.file_node_id().map(str::to_string) else {
        let model = RefactorResult {
            target: "process".to_string(),
            filepath: params.filepath.clone(),
            betti_1: None,
            gitnexus_available: Some(true),
            hotspots: Vec::new(),
            error: None,
        };
        return (model, render_hotspots_md("Process choke points", &[]));
    };

    let process_graph = ProcessGraph::from_mdg(&mdg, params.filepath.clone());
    let touching = process_graph.paths_touching_file(&file_id);
    let subgraph = ProcessGraph::from_paths(params.filepath.clone(), touching);
    let curvature = calculate_process_curvature(&subgraph, None);

    let hotspots: Vec<RefactorHotspot> = curvature
        .edges
        .iter()
        .take(params.limit)
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
        hotspots,
        error: None,
    };
    (model, md)
}

#[tool_router(router = refactor_router, vis = "pub(crate)")]
impl ToposServer {
    /// Refactor hotspots (read-only). Three targets: `cycles` (CFG cycle
    /// basis pointing at loop bodies), `dependencies` (MDG Forman curvature
    /// naming load-bearing import edges), `process` (process-graph choke
    /// points). All purely advisory — none feeds the lattice score.
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
    pub fn topos_refactor(&self, Parameters(params): Parameters<RefactorInput>) -> CallToolResult {
        let (model, md) = match params.target {
            RefactorTargetKind::Cycles => refactor_cycles(&params),
            RefactorTargetKind::Dependencies => refactor_dependencies(&params),
            RefactorTargetKind::Process => refactor_process(&params),
        };
        to_tool_result(&model, md)
    }
}
