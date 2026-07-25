//! `topos-mcp` binary — the stdio MCP server entry point.
//!
//! Launched by the `topos mcp` CLI command (and directly by MCP clients
//! configured with this binary). All tools, resources, and the refactor
//! prompt are registered by [`topos_mcp::server::serve`].
//!
//! Argument handling is deliberately minimal — this is an MCP server, not a
//! CLI — but `--version` is honored because it is the first thing anyone
//! reaches for when working out which of several installed Topos servers a
//! host is actually running. Without it the binary answers a version query by
//! waiting for an `initialize` frame that never comes and then reporting a
//! closed connection, which reads like a broken install.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("{}", topos_mcp::build_info::render());
            Ok(())
        }
        Some("--help" | "-h") => {
            println!(
                "topos-mcp {} — Topos MCP server (stdio)\n\n\
                 Usage: topos-mcp [--version|--help]\n\n\
                 With no arguments, serves the Model Context Protocol over stdio and \n\
                 expects an MCP client on the other end; it is not interactive.\n\n\
                 Options:\n  \
                 -V, --version   Build identity: version, executable, file root, staleness\n  \
                 -h, --help      Print this message",
                topos_mcp::build_info::version_with_build()
            );
            Ok(())
        }
        _ => topos_mcp::server::serve().await,
    }
}
