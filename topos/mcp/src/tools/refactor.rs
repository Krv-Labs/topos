//! Unified refactoring-guidance suite (Methods Upgrade milestone + issue #150).
//!
//! One tool, four targets (ranked hotspot list + suggested action), each
//! surfacing a different structural-analysis engine:
//!
//! - `target="cycles"`: cycle-basis extraction on the CFG.
//! - `target="dependencies"`: balanced Forman curvature on the MDG.
//! - `target="process"`: directed Forman-Ricci curvature on GitNexus
//!   process graphs.
//! - `target="graphify"`: orphan/fragile-edge detection on a Graphify
//!   knowledge graph (`graphify-out/graph.json`) — a wholly independent
//!   external tool from GitNexus/the MDG (issue #150).
//!
//! All four are purely advisory — none of this feeds
//! SIMPLE/COMPOSABLE/SECURE scoring.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_engine::adapters::graphify::GRAPHIFY_GRAPH_FILE;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::functors::probes::cfg::homology::calculate_cycle_basis;
use topos_engine::functors::probes::graphify::orphans::calculate_graphify_orphans;
use topos_engine::functors::probes::mdg::curvature::calculate_mdg_curvature;
use topos_engine::functors::probes::process::curvature::calculate_process_curvature;
use topos_engine::graphs::graphify::{GraphifyConfidence, GraphifyGraph};
use topos_engine::graphs::process::object::ProcessGraph;

use crate::evaluation::{
    detect_language, load_dep_graph, resolve_gitnexus_dir, resolve_graphify_dir,
};
use crate::formatting::to_tool_result;
use crate::refactor_hotspots::render_hotspots_md;
use crate::schemas::{RefactorHotspot, RefactorInput, RefactorResult, RefactorTargetKind};
use crate::security::{read_safe_utf8_file, resolve_file_root, resolve_within_root};
use crate::server::ToposServer;

const DEFAULT_ORPHAN_DEGREE_THRESHOLD: usize = 1;

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
        tool_available: None,
        hotspots,
        error: None,
    };
    (model, md)
}

fn span(cycle: &topos_engine::functors::probes::cfg::homology::SourceCycle) -> usize {
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
            tool_available: Some(false),
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
            tool_available: Some(true),
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
        tool_available: Some(true),
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
            tool_available: Some(false),
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
            tool_available: Some(true),
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
        tool_available: Some(true),
        hotspots,
        error: None,
    };
    (model, md)
}

/// Fragile-edge hotspot rows don't have a natural scalar score (confidence
/// is categorical) — use a constant sentinel below every real orphan degree
/// so orphan rows still sort first when both kinds are truncated together.
const FRAGILE_EDGE_SCORE: f64 = -1.0;

fn refactor_graphify(params: &RefactorInput) -> (RefactorResult, String) {
    let project_root = match resolve_file_root() {
        Ok(root) => root,
        Err(err) => {
            let model = err_result(RefactorTargetKind::Graphify, &params.filepath, err);
            return (model, render_hotspots_md("Graphify orphan hotspots", &[]));
        }
    };

    let graph = resolve_graphify_dir(params.graphify_dir.as_deref(), &project_root)
        .and_then(|dir| GraphifyGraph::from_json_file(dir.join(GRAPHIFY_GRAPH_FILE)).ok());
    let Some(graph) = graph else {
        let model = RefactorResult {
            target: "graphify".to_string(),
            filepath: params.filepath.clone(),
            betti_1: None,
            gitnexus_available: None,
            tool_available: Some(false),
            hotspots: Vec::new(),
            error: None,
        };
        return (model, render_hotspots_md("Graphify orphan hotspots", &[]));
    };

    let result = calculate_graphify_orphans(&graph, DEFAULT_ORPHAN_DEGREE_THRESHOLD);

    let mut hotspots: Vec<RefactorHotspot> = result
        .orphan_nodes
        .iter()
        .filter(|node| {
            node.source_file
                .as_deref()
                .is_some_and(|f| f == params.filepath)
        })
        .map(|node| RefactorHotspot {
            kind: "graphify_orphan".to_string(),
            label: node.label.clone(),
            filepath: params.filepath.clone(),
            line_start: None,
            line_end: None,
            score: node.degree as f64,
            suggestion: "Low-connectivity node in the Graphify knowledge graph: consider \
                         whether this symbol is dead code, or whether it should be linked \
                         into the rest of the module (import, call, or reference it \
                         explicitly)."
                .to_string(),
        })
        .chain(
            result
                .fragile_edges
                .iter()
                .filter(|edge| {
                    // FragileEdge.source/target are Graphify node ids, not
                    // file paths — scope by looking up each endpoint's own
                    // node.source_file instead.
                    [&edge.source, &edge.target].into_iter().any(|id| {
                        graph
                            .node(id)
                            .and_then(|n| n.source_file.as_deref())
                            .is_some_and(|f| f == params.filepath)
                    })
                })
                .map(|edge| RefactorHotspot {
                    kind: "graphify_fragile_edge".to_string(),
                    label: format!("{} -> {} ({})", edge.source, edge.target, edge.relation),
                    filepath: params.filepath.clone(),
                    line_start: None,
                    line_end: None,
                    score: FRAGILE_EDGE_SCORE,
                    suggestion: format!(
                        "Graphify only {} this relationship rather than directly observing it \
                         in the AST; verify it reflects a real dependency.",
                        match edge.confidence {
                            GraphifyConfidence::Ambiguous => "flagged as ambiguous",
                            _ => "inferred",
                        }
                    ),
                }),
        )
        .collect();
    hotspots.truncate(params.limit);

    let md = render_hotspots_md("Graphify orphan hotspots", &hotspots);
    let model = RefactorResult {
        target: "graphify".to_string(),
        filepath: params.filepath.clone(),
        betti_1: None,
        gitnexus_available: None,
        tool_available: Some(true),
        hotspots,
        error: None,
    };
    (model, md)
}

#[tool_router(router = refactor_router, vis = "pub(crate)")]
impl ToposServer {
    /// Refactor hotspots (read-only). Four targets: `cycles` (CFG cycle
    /// basis pointing at loop bodies), `dependencies` (MDG Forman curvature
    /// naming load-bearing import edges), `process` (process-graph choke
    /// points), `graphify` (orphan nodes / fragile inferred edges in a
    /// Graphify knowledge graph). All purely advisory — none feeds the
    /// lattice score.
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
            RefactorTargetKind::Graphify => refactor_graphify(&params),
        };
        to_tool_result(&model, md)
    }
}

#[cfg(test)]
mod graphify_dispatch_tests {
    use super::*;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn params(filepath: &str, graphify_dir: &str) -> RefactorInput {
        RefactorInput {
            target: RefactorTargetKind::Graphify,
            filepath: filepath.to_string(),
            gitnexus_dir: None,
            graphify_dir: Some(graphify_dir.to_string()),
            limit: 10,
        }
    }

    #[test]
    fn missing_graph_reports_tool_unavailable_without_an_error() {
        // A graphify_dir override that doesn't exist on disk (but is safely
        // inside the file root — resolve_file_root() resolves to this
        // crate's own manifest dir under `cargo test`) must degrade
        // gracefully, mirroring refactor_dependencies/refactor_process's
        // no-op contract: no hotspots, no error, just tool_available: false.
        let (model, _) = refactor_graphify(&params(
            "src/lib.rs",
            "topos-nonexistent-graphify-out-dir-for-tests",
        ));
        assert_eq!(model.tool_available, Some(false));
        assert!(model.error.is_none());
        assert!(model.hotspots.is_empty());
    }

    #[test]
    fn present_graph_scopes_hotspots_to_the_requested_file() {
        // resolve_graphify_dir requires the resolved path to be inside
        // resolve_file_root() (this crate's manifest dir under `cargo
        // test`) — the OS temp dir is outside that, so the fixture must
        // live under `target/` instead.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "topos_mcp_refactor_graphify_test_{}_{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("graph.json"),
            r#"{
                "nodes": [
                    {"id": "a", "label": "a()", "source_file": "src/a.rs"},
                    {"id": "b", "label": "b()", "source_file": "src/b.rs"}
                ],
                "links": [
                    {"source": "a", "target": "b", "confidence": "INFERRED",
                     "relation": "calls"}
                ]
            }"#,
        )
        .unwrap();

        let (model, _) = refactor_graphify(&params("src/a.rs", &dir.to_string_lossy()));

        assert_eq!(model.tool_available, Some(true));
        assert!(model.error.is_none());
        // Node "a" is an orphan (degree 1 <= threshold) and its
        // source_file matches; node "b" (also degree 1) belongs to a
        // different file and must not leak in. The fragile edge touches
        // "src/a.rs" as its source, so it's included too.
        assert!(model.hotspots.iter().all(|h| h.filepath == "src/a.rs"));
        assert!(model.hotspots.iter().any(|h| h.kind == "graphify_orphan"));
        assert!(model
            .hotspots
            .iter()
            .any(|h| h.kind == "graphify_fragile_edge"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
