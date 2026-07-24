//! `topos` — standalone CLI binary for Topos structural code quality
//! evaluation, built directly on `topos-engine` (issue #147).
//!
//! Mirrors the command surface of `topos/cli/main.py` /
//! `topos/cli/commands/quality.py`, but is a fresh idiomatic-Rust
//! implementation rather than a line-for-line port: the Python CLI's
//! rich terminal styling (`click.style`) and its "Suggestions" /
//! "Security Findings" sections depend on `topos/mcp/schemas.py`
//! (Pydantic) and `topos.evaluation.{suggestions,suppression,
//! security_guidance}`, which stay out of `topos-engine`'s scope for this
//! pass — see each command module's doc comment for what is
//! deliberately not ported yet.
//!
//! The `mcp` subcommand launches the in-process Rust MCP server (see
//! `topos-mcp`). Still deliberately unported (see individual command docs):
//! `depgraph` (needs GitNexus wiring) and `update`/`uninstall` (pip-specific
//! self-update, obsolete for a cargo/homebrew-distributed binary).

mod commands;

use clap::{Parser, Subcommand};

use commands::{compare, coverage, depgraph, evaluate, graphify, inspect, mcp};

#[derive(Parser)]
#[command(
    name = "topos",
    version,
    about = "Topos: category-theoretic code quality evaluation."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Evaluate code quality using the characteristic morphism χ_S : P → Ω.
    Evaluate(evaluate::EvaluateArgs),
    /// Inspect detailed metrics for a single file.
    Inspect(inspect::InspectArgs),
    /// Compare structural distance between two programs.
    Compare(compare::CompareArgs),
    /// Measure structural (UAST) test coverage.
    Coverage(coverage::CoverageArgs),
    /// Graphify knowledge-graph generation and orphan detection (issue #150).
    Graphify(graphify::GraphifyArgs),
    /// GitNexus dependency-graph generation for COMPOSABLE scoring.
    Depgraph(depgraph::DepgraphArgs),
    /// Launch the Topos MCP server over stdio.
    Mcp(mcp::McpArgs),
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Evaluate(args) => evaluate::run(args),
        Command::Inspect(args) => inspect::run(args),
        Command::Compare(args) => compare::run(args),
        Command::Coverage(args) => coverage::run(args),
        Command::Graphify(args) => graphify::run(args),
        Command::Depgraph(args) => depgraph::run(args),
        Command::Mcp(args) => mcp::run(args),
    };
    if let Err(message) = result {
        eprintln!("Error: {message}");
        std::process::exit(1);
    }
}
