//! Graphify knowledge-graph generation (side-effecting).
//!
//! Companion to the read-only `topos_refactor(target="graphify")`: that tool
//! only ever reads an already-generated `graphify-out/graph.json`, so a
//! separate tool is needed for MCP clients that can't shell out to `graphify`
//! themselves. Mirrors `tools/depgraph.rs`'s generate/status split, minus a
//! dedicated status tool — Graphify's own SHA256 content-cache already makes
//! `graphify update` cheap to call unconditionally, so there's no analogous
//! "stale" state worth reporting separately.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_engine::adapters::graphify::{
    ensure_graphify_graph, graphify_out_dir, GRAPHIFY_GRAPH_FILE,
};

use crate::formatting::to_tool_result;
use crate::schemas::{GenerateGraphifyInput, GenerateGraphifyResult};
use crate::security::{resolve_file_root, resolve_within_root};
use crate::server::ToposServer;

fn generate_error(message: String) -> GenerateGraphifyResult {
    GenerateGraphifyResult {
        ok: false,
        returncode: 1,
        graphify_out_dir: None,
        generated: false,
        message: message.clone(),
        error: Some(message),
    }
}

pub(crate) fn render_generate_graphify_md(r: &GenerateGraphifyResult) -> String {
    if let Some(err) = &r.error {
        return format!("**Error:** {err}");
    }
    let head = if r.ok && r.generated {
        "Graphify graph generated."
    } else if r.ok {
        "Graphify graph already current."
    } else {
        "Generation failed."
    };
    let mut lines = vec![format!("**{head}**"), r.message.clone()];
    if let Some(dir) = &r.graphify_out_dir {
        lines.push(format!("**graphify-out:** `{dir}`"));
    }
    lines.join("\n")
}

#[tool_router(router = graphify_router, vis = "pub(crate)")]
impl ToposServer {
    /// Generate the Graphify knowledge graph (`graphify-out/graph.json`)
    /// via the external `graphify` CLI (side-effecting).
    ///
    /// Skips running `graphify` when a graph is already present, unless
    /// `force=true`. Wholly independent of `.gitnexus`/GitNexus — feeds
    /// only `topos_refactor(target="graphify")`, never SIMPLE/COMPOSABLE/
    /// SECURE.
    #[tool(
        name = "topos_generate_graphify_graph",
        annotations(
            title = "Topos Generate Graphify Graph",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    pub fn topos_generate_graphify_graph(
        &self,
        Parameters(params): Parameters<GenerateGraphifyInput>,
    ) -> CallToolResult {
        let project_root = match resolve_file_root() {
            Ok(root) => root,
            Err(err) => {
                let model = generate_error(err);
                let md = render_generate_graphify_md(&model);
                return to_tool_result(&model, md);
            }
        };
        let target_dir = match &params.directory {
            Some(dir) => match resolve_within_root(dir) {
                Ok(resolved) if resolved.is_dir() => resolved,
                Ok(resolved) => {
                    let model = generate_error(format!("Not a directory: {}", resolved.display()));
                    let md = render_generate_graphify_md(&model);
                    return to_tool_result(&model, md);
                }
                Err(err) => {
                    let model = generate_error(err);
                    let md = render_generate_graphify_md(&model);
                    return to_tool_result(&model, md);
                }
            },
            None => project_root.clone(),
        };

        if !params.force {
            let graph_file = graphify_out_dir(&target_dir).join(GRAPHIFY_GRAPH_FILE);
            if graph_file.is_file() {
                let model = GenerateGraphifyResult {
                    ok: true,
                    returncode: 0,
                    graphify_out_dir: Some(
                        graphify_out_dir(&target_dir).to_string_lossy().to_string(),
                    ),
                    generated: false,
                    message: "Graphify graph already current.".to_string(),
                    error: None,
                };
                let md = render_generate_graphify_md(&model);
                return to_tool_result(&model, md);
            }
        }

        let result = ensure_graphify_graph(&target_dir, true, None);
        let model = GenerateGraphifyResult {
            ok: result.ok,
            returncode: result.returncode,
            graphify_out_dir: result
                .graphify_out_dir
                .map(|p| p.to_string_lossy().to_string()),
            generated: result.ok,
            message: result.message.clone(),
            error: if result.ok {
                None
            } else {
                Some(result.message)
            },
        };
        let md = render_generate_graphify_md(&model);
        to_tool_result(&model, md)
    }
}
