//! `topos mcp` — launch the Topos MCP server (stdio).
//!
//! The server itself lives in the `topos-mcp` crate; this subcommand just
//! spins up a Tokio runtime and hands off to [`topos_mcp::server::serve`].
//! It exists so the single `topos` binary can act as the MCP server (the VS
//! Code extension invokes `topos mcp`), while the standalone `topos-mcp`
//! binary remains the entry point for the PyPI wheel.

use clap::Args;

#[derive(Args)]
pub struct McpArgs {}

pub fn run(_args: McpArgs) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to start async runtime: {e}"))?;
    runtime
        .block_on(topos_mcp::server::serve())
        .map_err(|e| format!("MCP server error: {e}"))
}
