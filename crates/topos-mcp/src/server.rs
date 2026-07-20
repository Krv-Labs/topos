//! Topos MCP server — the [`ServerHandler`] impl plus the tool-router sum.
//!
//! Server name follows the `{service}_mcp` convention. Transport is stdio
//! by default; the `topos-mcp` binary (see `main.rs`) launches [`serve`].
//!
//! The nine tool modules under [`crate::tools`] each contribute a named
//! `#[tool_router]` (e.g. `evaluate_router`, `assess_router`); they are
//! summed here into one [`ToolRouter`]. Resources (`topos://docs/*`) and
//! the `topos_refactor_until_ideal` prompt are implemented directly in the
//! [`ServerHandler`] methods, since they are small and static.

use std::future::Future;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    GetPromptRequestParams, GetPromptResult, Implementation, ListPromptsResult,
    ListResourcesResult, PaginatedRequestParams, Prompt, PromptArgument, PromptMessage,
    ProtocolVersion, ReadResourceRequestParams, ReadResourceResult, Resource, ResourceContents,
    Role, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer};

use crate::docs::{doc_content_for_slug, DOC_SLUGS};

const SERVER_INSTRUCTIONS: &str = "Topos evaluates structural code quality on a diamond lattice. \
For agent loops, load the compact contract with `topos_get_doc(topic=\"agent-contract\")` or \
fetch `topos://docs/agent-contract`. Default loop: topos_evaluate_file, edit in place, then \
topos_assess_worktree_change (baseline = git ref). For uncommitted or untracked baselines, \
snapshot first with topos_begin_refactor, then verify with topos_assess_snapshot. Use \
topos_assess_improvement only for side-by-side variants. Use gitnexus_dir (default: ./.gitnexus) \
to enable COMPOSABLE/IDEAL; check graph state with topos_depgraph_status and build/refresh it \
with topos_generate_depgraph. topos_calculate_coverage reports test-suite coverage — structural \
(UAST) declaration matching and k-gram recall — as a separate signal, outside the lattice.";

const REFACTOR_PROMPT_NAME: &str = "topos_refactor_until_ideal";

/// The Topos MCP server handler. Holds the combined [`ToolRouter`]; all
/// state the tools need (file root, snapshot store, dep-graph cache) is
/// process-global, so this struct itself is empty.
#[derive(Clone)]
pub struct ToposServer {
    tool_router: ToolRouter<ToposServer>,
}

impl Default for ToposServer {
    fn default() -> Self {
        Self::new()
    }
}

impl ToposServer {
    pub fn new() -> Self {
        let tool_router = Self::evaluate_router()
            + Self::assess_router()
            + Self::compare_router()
            + Self::coverage_router()
            + Self::depgraph_router()
            + Self::docs_router()
            + Self::inspect_router()
            + Self::preferences_router()
            + Self::refactor_router();
        ToposServer { tool_router }
    }
}

fn doc_resources() -> Vec<Resource> {
    DOC_SLUGS
        .iter()
        .map(|slug| {
            Resource::new(
                format!("topos://docs/{slug}"),
                format!("topos_{}", slug.replace('-', "_")),
            )
            .with_mime_type("text/markdown")
            .with_description(doc_description(slug))
        })
        .collect()
}

fn doc_description(slug: &str) -> &'static str {
    match slug {
        "agent-contract" => {
            "Compact outcome-first contract for agent loops: targets, gates, risks, and \
             next-tool fields."
        }
        "lattice" => {
            "The 8-element 3-cube H(G_qual) over {SIMPLE, COMPOSABLE, SECURE}; bottom = SLOP, \
             top = IDEAL."
        }
        "metrics" => "Every metric key, good ranges, and how they roll up into dimension scores.",
        "priority" => "Priority profiles (simple / composable / secure) and when to use each.",
        "preferences" => {
            "User preferences over G_qual: induced total order on Ω and the targeted relaxation \
             walk toward the ideal intersection."
        }
        "workflows" => {
            "The canonical agent refactor loop: review → plan → refactor → re-measure. Read \
             first."
        }
        _ => "Topos documentation.",
    }
}

fn refactor_prompt_definition() -> Prompt {
    Prompt::new(
        REFACTOR_PROMPT_NAME,
        Some(
            "Scaffolds the canonical Topos refactor loop (review → plan → refactor → \
             re-measure) with a concrete target, tool call sequence, and termination criteria.",
        ),
        Some(vec![
            PromptArgument::new("filepath")
                .with_description("Target file to refactor.")
                .with_required(true),
            PromptArgument::new("priority").with_description(
                "Which generator to prioritize (simple, composable, or secure; default secure).",
            ),
            PromptArgument::new("max_iterations")
                .with_description("Budget for iterations before stopping (default 5)."),
            PromptArgument::new("preferences").with_description(
                "Optional strict total order on the three generators, comma-separated \
                 (e.g. \"composable,secure,simple\").",
            ),
        ]),
    )
}

fn build_refactor_prompt_text(
    filepath: &str,
    priority: &str,
    max_iterations: i64,
    preferences: Option<&str>,
) -> String {
    let ranking: Vec<String> = match preferences {
        Some(p) => p
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        None => {
            let mut r = vec![priority.to_string()];
            for g in ["simple", "composable", "secure"] {
                if g != priority {
                    r.push(g.to_string());
                }
            }
            r
        }
    };
    let ranking_str = ranking.join(" ≻ ");
    let ranking_json = serde_json::to_string(&ranking).unwrap_or_else(|_| "[]".to_string());
    let pref_args = format!(", \"preferences\": {{\"ranking\": {ranking_json}}}");
    format!(
        "Improve `{filepath}` with Topos. Priority: **{priority}**. Iteration budget: \
         **{max_iterations}**. Preference order: `{ranking_str}`.\n\n\
         Use the compact contract in `topos://docs/agent-contract`. Success means a focused \
         structural change moves the target toward `preference_walk.next_step` or the fallback \
         target, preserves behavior, and leaves residual risks explicit.\n\n\
         Core tool calls:\n```json\n{{\"params\": {{\"filepath\": \"{filepath}\"{pref_args}}}}}\n```\n\
         Use with `topos_evaluate_file` to measure the current verdict.\n\n\
         ```json\n{{\"params\": {{\"filepath\": \"{filepath}\"{pref_args}}}}}\n```\n\
         Use with `topos_inspect_code` when the returned `agent_contract`, `guidance`, or \
         `suggestions` indicate inspection is needed.\n\n\
         ```json\n{{\"params\": {{\"filepath\": \"{filepath}\", \"baseline_ref\": \"HEAD\"{pref_args}}}}}\n```\n\
         Verification route:\n\
         - Default after in-place edits: `topos_assess_worktree_change` against `HEAD` or another git ref.\n\
         - Dirty or untracked baseline: call `topos_begin_refactor` before editing, then `topos_assess_snapshot`.\n\
         - Side-by-side variant only: use `topos_assess_improvement`.\n\n\
         Acceptance gates:\n\
         - Assessment `status` is `IMPROVEMENT` or `IMPROVEMENT_SCORE`.\n\
         - Assessment `status` is not `SUSPICIOUS_NO_STRUCTURAL_CHANGE`.\n\
         - Active SECURE findings are fixed or intentionally acknowledged and disclosed.\n\
         - Project rollup is checked after non-trivial cross-file changes.\n\
         - Relevant behavior tests, type checks, or linters pass when available; if unavailable \
         or not run, report that explicitly.\n\n\
         Return only the baseline, change summary, Topos verification, behavior verification, \
         and residual risks.\n"
    )
}

#[rmcp::tool_handler(router = self.tool_router)]
impl ServerHandler for ToposServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_protocol_version(ProtocolVersion::default())
        .with_server_info(Implementation::new("topos_mcp", env!("CARGO_PKG_VERSION")))
        .with_instructions(SERVER_INSTRUCTIONS)
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListResourcesResult::with_all_items(doc_resources())))
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResult, McpError>> + Send + '_ {
        let result = match request.uri.strip_prefix("topos://docs/") {
            Some(slug) => doc_content_for_slug(slug).map(|content| {
                ReadResourceResult::new(vec![ResourceContents::text(content, request.uri.clone())])
            }),
            None => None,
        };
        std::future::ready(result.ok_or_else(|| {
            McpError::resource_not_found(format!("Unknown resource: {}", request.uri), None)
        }))
    }

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListPromptsResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListPromptsResult::with_all_items(vec![
            refactor_prompt_definition(),
        ])))
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<GetPromptResult, McpError>> + Send + '_ {
        let result = if request.name == REFACTOR_PROMPT_NAME {
            let args = request.arguments.unwrap_or_default();
            let filepath = args
                .get("filepath")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let priority = args
                .get("priority")
                .and_then(|v| v.as_str())
                .unwrap_or("secure")
                .to_string();
            let max_iterations = args
                .get("max_iterations")
                .and_then(|v| v.as_i64())
                .or_else(|| {
                    args.get("max_iterations")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                })
                .unwrap_or(5);
            let preferences = args
                .get("preferences")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let text = build_refactor_prompt_text(
                &filepath,
                &priority,
                max_iterations,
                preferences.as_deref(),
            );
            Ok(GetPromptResult::new(vec![PromptMessage::new_text(
                Role::User,
                text,
            )]))
        } else {
            Err(McpError::invalid_params(
                format!("Unknown prompt: {}", request.name),
                None,
            ))
        };
        std::future::ready(result)
    }
}

/// Serve the Topos MCP server over stdio until the client disconnects.
pub async fn serve() -> anyhow::Result<()> {
    use rmcp::transport::io::stdio;
    use rmcp::ServiceExt;

    let service = ToposServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
