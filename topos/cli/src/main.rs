//! `topos` — standalone Rust CLI for structural code-quality evaluation.
//!
//! Human commands call directly into `topos-engine`; `topos mcp` launches the
//! in-process `topos-mcp` server. `topos update` reports which distribution
//! channel a binary came from and delegates to that channel's own upgrade
//! command, rather than replacing package-manager-managed files itself;
//! `topos uninstall` removes agent harness registrations, not the binary.

mod commands;

use std::fmt::Write as _;
use std::io::IsTerminal;

use clap::{Parser, Subcommand};
use console::Style;

use commands::{
    compare, config, coverage, depgraph, evaluate, inspect, install, mcp, pr_recap, update,
};

const ROOT_COMMANDS: [(&str, &str); 12] = [
    ("evaluate", "Score a file or directory"),
    ("inspect", "Explain one file"),
    ("pr-recap", "Review a change or a pull request"),
    ("config", "Set project priorities"),
    ("compare", "Compare two files"),
    ("coverage", "Compare source structure with tests"),
    ("depgraph", "Build the COMPOSABLE graph"),
    ("install", "Configure agent harnesses to use Topos"),
    ("uninstall", "Remove Topos from agent harnesses"),
    ("update", "Check for and install a newer release"),
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
    /// Build the GitNexus graph used by COMPOSABLE.
    Depgraph(depgraph::DepgraphArgs),
    /// Configure agent harnesses (Claude Code, Codex, Gemini, ...) to use Topos.
    Install(install::InstallArgs),
    /// Remove Topos-owned entries from agent harnesses.
    Uninstall(install::UninstallArgs),
    /// Check for a newer release and offer to install it.
    #[command(after_long_help = update::LONG_HELP)]
    Update(update::UpdateArgs),
    /// Show which agent harnesses are configured to use Topos.
    Status(install::StatusArgs),
    /// Start the MCP server over stdio.
    Mcp(mcp::McpArgs),
    /// Structural before/after for a git range or a pull request number.
    #[command(name = "pr-recap", after_long_help = pr_recap::LONG_HELP)]
    PrRecap(pr_recap::PrRecapArgs),
}

fn main() {
    match pre_dispatch() {
        PreDispatch::Exit(code) => std::process::exit(code),
        PreDispatch::Help => return,
        PreDispatch::Run => {}
    }
    run_command()
}

/// What `main` has to do before a subcommand can take over.
///
/// The bare invocation and a lone `-h` are intercepted here rather than by clap,
/// because root help is hand-rendered (`root_help`) and `disable_help_subcommand`
/// means clap would otherwise claim them. Returning the decision rather than
/// exiting keeps `main` a dispatcher.
enum PreDispatch {
    /// Print help on stderr and exit with this code. 2 for a bare `topos`,
    /// which is clap's own convention for "you gave me nothing to do".
    Exit(i32),
    /// Help was printed on stdout; nothing failed.
    Help,
    /// A real subcommand was named.
    Run,
}

fn pre_dispatch() -> PreDispatch {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let styled = |term: bool| term && std::env::var_os("NO_COLOR").is_none();
    match args.as_slice() {
        [] => {
            eprint!("{}", root_help(styled(std::io::stderr().is_terminal())));
            PreDispatch::Exit(2)
        }
        [only] if matches!(only.to_str(), Some("-h" | "--help")) => {
            print!("{}", root_help(styled(std::io::stdout().is_terminal())));
            PreDispatch::Help
        }
        _ => PreDispatch::Run,
    }
}

/// Parse, dispatch, and report the outcome.
fn run_command() {
    let cli = Cli::parse();
    // `topos update` and `topos mcp` are excluded from the passive notice: the
    // first *is* the notice, and printing "an update is available" over the
    // update command's own report would be noise. The MCP server surfaces its
    // own notice on the tool-result channel instead.
    //
    // `topos uninstall` is excluded too, and for a harder reason: the notice
    // writes its cache under `~/.local/state/topos`, which is exactly the
    // directory uninstall prunes. Offering it after a teardown would recreate
    // the state the user just asked us to remove, and the e2e suite asserts
    // that uninstall leaves no file behind.
    let quiet_notice = wants_no_notice(&cli.command);
    let result = dispatch(cli.command);
    if let Err(message) = result {
        eprintln!("Error: {message}");
        std::process::exit(1);
    }
    // Printed *after* the command, so it lands below the output rather than
    // scrolling away above it, and only on the success path — a command that
    // failed has bigger news.
    if !quiet_notice {
        passive_notice();
    }
}

/// Commands that suppress the passive update notice, and why.
///
/// A predicate rather than an inline `matches!` so the list is named, greppable,
/// and can grow without `main` growing.
fn wants_no_notice(command: &Command) -> bool {
    matches!(
        command,
        Command::Update(_) | Command::Mcp(_) | Command::Uninstall(_)
    )
}

/// The one place a subcommand name becomes a call.
///
/// Split from [`run_command`] so the match is the whole of the dispatch layer,
/// which is what keeps adding a command to one arm instead of three places.
fn dispatch(command: Command) -> Result<(), String> {
    match command {
        Command::Config(args) => config::run(args),
        Command::Evaluate(args) => evaluate::run(args),
        Command::Inspect(args) => inspect::run(args),
        Command::Compare(args) => compare::run(args),
        Command::Coverage(args) => coverage::run(args),
        Command::Depgraph(args) => depgraph::run(args),
        Command::Install(args) => install::run_install(args),
        Command::Uninstall(args) => install::run_uninstall(args),
        Command::Update(args) => update::run(args),
        Command::Status(args) => install::run_status(args),
        Command::Mcp(args) => mcp::run(args),
        Command::PrRecap(args) => pr_recap::run(args),
    }
}

/// The 24-hour "an update is available" line, or nothing at all.
///
/// Refreshes the cache first when it is stale, so a user who never runs
/// `topos update` still hears about a new release. That costs one `stat` on
/// every normal run and at most one `curl` a day.
fn passive_notice() {
    let Ok(home) = topos_mcp::paths::home_dir() else {
        return;
    };
    topos_mcp::update::notice::refresh(&home);
    if let Some(notice) = topos_mcp::update::notice::cli_notice(&home, env!("CARGO_PKG_VERSION")) {
        // stderr, so `--json` on stdout stays machine-readable.
        eprintln!("{notice}");
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
            "depgraph",
            "install",
            "uninstall",
            "update",
            "status",
            "mcp",
            "pr-recap",
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
        assert_eq!(ROOT_COMMANDS.len(), 12);
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
