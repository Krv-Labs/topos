//! `topos-mcp` binary — the stdio MCP server entry point.
//!
//! Launched by the `topos mcp` CLI command (and directly by MCP clients
//! configured with this binary). All tools, resources, and the refactor
//! prompt are registered by [`topos_mcp::server::serve`].

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    topos_mcp::server::serve().await
}
