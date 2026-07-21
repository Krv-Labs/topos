//! Documentation tool — read the Topos docs as an MCP tool.
//!
//! Exists because MCP resources (`topos://docs/*`) are not uniformly
//! reachable from agents across clients: Claude Code bridges resources to
//! the model implicitly; some other clients do not. This tool exposes the
//! same markdown content via a tool call so critical context (especially
//! the workflow guide) is reachable everywhere.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_router};

use crate::docs::doc_content;
use crate::schemas::GetDocInput;
use crate::server::ToposServer;

#[tool_router(router = docs_router, vis = "pub(crate)")]
impl ToposServer {
    /// Return a Topos documentation page as Markdown.
    ///
    /// Use when your MCP client does not expose resource fetching to the
    /// agent. Clients that do surface resources should prefer the
    /// equivalent resource URI for efficiency: `topos://docs/{topic}`.
    ///
    /// Topics: `agent-contract` (compact loop contract, read first for
    /// refactors), `lattice` (the 8-element 3-cube H(G_qual)), `metrics`
    /// (every metric key, thresholds, interpretation), `preferences`
    /// (strict generator rankings and preference walks), `priority`
    /// (priority profiles), `workflows` (the expanded refactor loop guide).
    #[tool(
        name = "topos_get_doc",
        annotations(
            title = "Topos Documentation",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_get_doc(&self, Parameters(params): Parameters<GetDocInput>) -> CallToolResult {
        CallToolResult::success(vec![ContentBlock::text(doc_content(params.topic))])
    }
}
