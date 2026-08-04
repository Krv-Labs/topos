//! `topos` — standalone Rust CLI for structural code-quality evaluation.
//!
//! Human commands call directly into `topos-engine`; `topos mcp` launches the
//! in-process `topos-mcp` server. Update and uninstall remain package-manager
//! responsibilities rather than CLI subcommands.

mod commands;

use std::fmt::Write as _;
use std::io::IsTerminal;

use clap::{Parser, Subcommand};
use console::Style;

use commands::{compare, config, coverage, depgraph, evaluate, graphify, inspect, install, mcp};

const ROOT_COMMANDS: [(&str, &str); 11] = [
    ("evaluate", "Score a file or directory"),
    ("inspect", "Explain one file"),
    ("config", "Set project priorities"),
    ("compare", "Compare two files"),
    ("coverage", "Compare source structure with tests"),
    ("depgraph", "Build the COMPOSABLE graph"),
    ("graphify", "Inspect graph health"),
    ("install", "Configure agent harnesses to use Topos"),
    ("uninstall", "Remove Topos from agent harnesses"),
    ("status", "Show which harnesses are configured"),
    ("mcp", "Start the MCP server"),
];

#[derive(Parser)]
#[command(
    name = "topos",
    version,
    about = "Category-theoretic code quality evaluation.",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// View or edit project settings.
    Config(config::ConfigArgs),
    /// Score files and directories across the quality pillars.
    Evaluate(evaluate::EvaluateArgs),
    /// Explain one file with metrics, functions, and guidance.
    Inspect(inspect::InspectArgs),
    /// Compare structural distance between two files.
    Compare(compare::CompareArgs),
    /// Compare source structure with tests without executing them.
    #[command(
        arg_required_else_help = true,
        after_help = "Examples:\n  topos coverage src/lib.rs --tests tests/lib.rs --language rust\n  topos coverage src/ --tests tests/ --recursive --language rust"
    )]
    Coverage(coverage::CoverageArgs),
    /// Generate or inspect a Graphify knowledge graph.
    Graphify(graphify::GraphifyArgs),
    /// Build the GitNexus graph used by COMPOSABLE.
    Depgraph(depgraph::DepgraphArgs),
    /// Configure agent harnesses (Claude Code, Codex, Gemini, ...) to use Topos.
    Install(install::InstallArgs),
    /// Remove Topos-owned entries from agent harnesses.
    Uninstall(install::UninstallArgs),
    /// Show which agent harnesses are configured to use Topos.
    Status(install::StatusArgs),
    /// Start the MCP server over stdio.
    Mcp(mcp::McpArgs),
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty() {
        eprint!(
            "{}",
            root_help(std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none())
        );
        std::process::exit(2);
    }
    if args.len() == 1 && matches!(args[0].to_str(), Some("-h" | "--help")) {
        print!(
            "{}",
            root_help(std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none())
        );
        return;
    }
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Config(args) => config::run(args),
        Command::Evaluate(args) => evaluate::run(args),
        Command::Inspect(args) => inspect::run(args),
        Command::Compare(args) => compare::run(args),
        Command::Coverage(args) => coverage::run(args),
        Command::Graphify(args) => graphify::run(args),
        Command::Depgraph(args) => depgraph::run(args),
        Command::Install(args) => install::run_install(args),
        Command::Uninstall(args) => install::run_uninstall(args),
        Command::Status(args) => install::run_status(args),
        Command::Mcp(args) => mcp::run(args),
    };
    if let Err(message) = result {
        eprintln!("Error: {message}");
        std::process::exit(1);
    }
}

fn root_help(styled: bool) -> String {
    let emphasis = |text: &str, style: Style| {
        if styled {
            style.force_styling(true).apply_to(text).to_string()
        } else {
            text.to_string()
        }
    };
    let mut output = String::new();
    writeln!(
        output,
        "{}\n{}\n",
        emphasis(
            &format!("topos {}", env!("CARGO_PKG_VERSION")),
            Style::new().bold()
        ),
        emphasis(
            "Category-theoretic code quality evaluation.",
            Style::new().dim()
        )
    )
    .expect("writing to String cannot fail");
    writeln!(
        output,
        "{}\n    topos <command>\n",
        emphasis("Usage", Style::new().bold())
    )
    .expect("writing to String cannot fail");
    writeln!(output, "{}", emphasis("Commands", Style::new().bold()))
        .expect("writing to String cannot fail");
    for (command, description) in ROOT_COMMANDS {
        writeln!(
            output,
            "    {command:<11} {}",
            emphasis(description, Style::new().dim())
        )
        .expect("writing to String cannot fail");
    }
    writeln!(
        output,
        "\n{}\n    -h, --help\n    -V, --version\n\n{}",
        emphasis("Options", Style::new().bold()),
        emphasis(
            "Run `topos <command> --help` for details.",
            Style::new().dim()
        )
    )
    .expect("writing to String cannot fail");
    output
}

#[cfg(test)]
mod tests {
    use clap::{error::ErrorKind, Parser};

    use super::{root_help, Cli, ROOT_COMMANDS};

    #[test]
    fn root_help_uses_the_terminal_grammar_and_keeps_every_command() {
        let plain = root_help(false);
        assert!(plain.starts_with("topos "));
        assert!(plain.contains("\n    topos <command>\n"));
        for command in [
            "config",
            "evaluate",
            "inspect",
            "compare",
            "coverage",
            "graphify",
            "depgraph",
            "install",
            "uninstall",
            "status",
            "mcp",
        ] {
            assert!(
                plain.contains(&format!("\n    {command}")),
                "missing {command} from help"
            );
        }
        assert!(!plain.contains("◇"));
        assert!(!plain.contains("\n    help"));
        assert!(plain.ends_with("Run `topos <command> --help` for details.\n"));

        let styled = root_help(true);
        assert!(styled.contains("\u{1b}[1mCommands\u{1b}[0m"));
        assert!(styled.contains("\u{1b}[2mScore a file or directory\u{1b}[0m"));
        assert_eq!(ROOT_COMMANDS.len(), 11);
    }

    #[test]
    fn bare_coverage_shows_examples_instead_of_only_missing_arguments() {
        let error = match Cli::try_parse_from(["topos", "coverage"]) {
            Ok(_) => panic!("bare coverage unexpectedly parsed"),
            Err(error) => error,
        };
        assert_eq!(
            error.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        let help = error.to_string();
        assert!(help.contains("SOURCE_PATHS"));
        assert!(help.contains("topos coverage src/lib.rs --tests tests/lib.rs"));
    }
}
