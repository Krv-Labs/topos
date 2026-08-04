//! `topos install` — configure one or more agent harnesses to use Topos.

use std::path::Path;

use console::Style;

use super::integrations::{
    self, antigravity_pointer_path, claude_desktop_config_path, record_created_file,
    set_antigravity_import, set_codex_entry, set_copilot_block, set_mcp_entry, skill_path_agents,
    skill_path_claude, write_skill, State,
};
use crate::commands::render::{paint, RenderOptions};

fn ok(opts: RenderOptions) -> String {
    paint("✓", Style::new().green(), opts)
}

fn pending(opts: RenderOptions) -> String {
    paint("○", Style::new().color256(208), opts)
}

fn err(opts: RenderOptions) -> String {
    paint("✕", Style::new().red(), opts)
}

/// Shared three-way branch every install step follows: already active, a
/// dry-run preview, or an actual write. `apply` performs the write and
/// returns whether anything changed.
fn apply_step(
    state: State,
    dry_run: bool,
    active_msg: &str,
    preview_msg: &str,
    applied_msg: &str,
    apply: impl FnOnce() -> Result<bool, String>,
    opts: RenderOptions,
) -> bool {
    match state {
        State::Active => {
            println!("│    {} {active_msg}", ok(opts));
            true
        }
        _ if dry_run => {
            println!("│    {} [dry run] {preview_msg}", pending(opts));
            true
        }
        _ => match apply() {
            Ok(_) => {
                println!("│    {} {applied_msg}", ok(opts));
                true
            }
            Err(e) => {
                println!("│    {} {e}", err(opts));
                false
            }
        },
    }
}

type Handler = fn(&Path, bool, RenderOptions) -> bool;

/// One entry per supported harness id — a data table instead of a match arm
/// per harness keeps `run` itself flat regardless of how many harnesses
/// exist.
const HANDLERS: &[(&str, Handler)] = &[
    ("claude", install_claude),
    ("claude-desktop", install_claude_desktop),
    ("codex", install_codex),
    ("gemini", install_gemini),
    ("copilot", install_copilot),
    ("skills", install_skills),
    ("antigravity", install_antigravity),
];

pub(crate) fn run(home: &Path, selected: &[String], dry_run: bool) -> Result<(), String> {
    let opts = RenderOptions::stdout();
    print_header(dry_run, opts);

    let mut success = true;
    for id in selected {
        success &= dispatch(id, home, dry_run, opts);
    }

    print_summary(success, opts)
}

fn dispatch(id: &str, home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    match HANDLERS.iter().find(|(harness_id, _)| *harness_id == id) {
        Some((_, handler)) => handler(home, dry_run, opts),
        None => {
            println!("│  {} unknown harness: {id}", err(opts));
            false
        }
    }
}

fn print_header(dry_run: bool, opts: RenderOptions) {
    let mode = if dry_run {
        " (DRY RUN — PREVIEW ONLY, NO CHANGES MADE)"
    } else {
        ""
    };
    println!(
        "{}",
        paint(
            format!("┌  Topos Harness Install{mode}"),
            Style::new().bold(),
            opts
        )
    );
    println!("│");
}

fn print_summary(success: bool, opts: RenderOptions) -> Result<(), String> {
    if success {
        println!("└  {}", paint("Done.", Style::new().bold(), opts));
        Ok(())
    } else {
        println!(
            "└  {}",
            paint(
                "Incomplete — existing files were preserved; review the errors above.",
                Style::new().bold(),
                opts
            )
        );
        Err("one or more harnesses failed to configure".to_string())
    }
}

fn install_claude(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint("Claude Code (~/.claude)", Style::new().bold(), opts)
    );
    let config = home.join(".claude.json");
    let existed = config.is_file();
    let mut success = apply_step(
        integrations::json_mcp_state(&config),
        dry_run,
        "MCP server already configured",
        &format!("would add MCP server entry to {}", config.display()),
        &format!("added MCP server entry to {}", config.display()),
        || {
            let changed = set_mcp_entry(&config)?;
            if changed && !existed {
                record_created_file(home, "claude", &config)?;
            }
            Ok(changed)
        },
        opts,
    );
    let skill = skill_path_claude(home);
    success &= apply_step(
        integrations::skill_state(&skill),
        dry_run,
        "skill already up to date",
        &format!("would write skill to {}", skill.display()),
        &format!("wrote skill to {}", skill.display()),
        || write_skill(&skill),
        opts,
    );
    println!("│");
    success
}

fn install_claude_desktop(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    let config = claude_desktop_config_path(home);
    println!(
        "│  {}",
        paint(
            format!("Claude Desktop App ({})", config.display()),
            Style::new().bold(),
            opts
        )
    );
    let existed = config.is_file();
    let success = apply_step(
        integrations::json_mcp_state(&config),
        dry_run,
        "MCP server already configured",
        &format!("would add MCP server entry to {}", config.display()),
        &format!("added MCP server entry to {}", config.display()),
        || {
            let changed = set_mcp_entry(&config)?;
            if changed && !existed {
                record_created_file(home, "claude-desktop", &config)?;
            }
            Ok(changed)
        },
        opts,
    );
    println!("│");
    success
}

fn install_codex(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint("Codex CLI (~/.codex)", Style::new().bold(), opts)
    );
    let config = home.join(".codex/config.toml");
    let existed = config.is_file();
    let success = apply_step(
        integrations::codex_state(&config),
        dry_run,
        "MCP server already configured",
        "would add [mcp_servers.topos] to ~/.codex/config.toml",
        "added [mcp_servers.topos] to ~/.codex/config.toml",
        || {
            let changed = set_codex_entry(&config)?;
            if changed && !existed {
                record_created_file(home, "codex", &config)?;
            }
            Ok(changed)
        },
        opts,
    );
    println!("│");
    success
}

fn install_gemini(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint("Gemini CLI (~/.gemini)", Style::new().bold(), opts)
    );
    let config = home.join(".gemini/settings.json");
    let existed = config.is_file();
    let success = apply_step(
        integrations::json_mcp_state(&config),
        dry_run,
        "MCP server already configured",
        &format!("would add MCP server entry to {}", config.display()),
        &format!("added MCP server entry to {}", config.display()),
        || {
            let changed = set_mcp_entry(&config)?;
            if changed && !existed {
                record_created_file(home, "gemini", &config)?;
            }
            Ok(changed)
        },
        opts,
    );
    println!("│");
    success
}

fn install_copilot(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint("GitHub Copilot CLI (~/.copilot)", Style::new().bold(), opts)
    );
    let config = home.join(".copilot/copilot-instructions.md");
    let existed = config.is_file();
    let success = apply_step(
        integrations::copilot_state(&config),
        dry_run,
        "instructions already present",
        "would add an instruction block to ~/.copilot/copilot-instructions.md",
        "added an instruction block to ~/.copilot/copilot-instructions.md",
        || {
            let changed = set_copilot_block(&config)?;
            if changed && !existed {
                record_created_file(home, "copilot", &config)?;
            }
            Ok(changed)
        },
        opts,
    );
    println!("│");
    success
}

fn install_skills(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint(
            "Cursor & VS Code (~/.agents/skills)",
            Style::new().bold(),
            opts
        )
    );
    let skill = skill_path_agents(home);
    let mut success = apply_step(
        integrations::skill_state(&skill),
        dry_run,
        "skill already up to date",
        &format!("would write skill to {}", skill.display()),
        &format!("wrote skill to {}", skill.display()),
        || write_skill(&skill),
        opts,
    );
    let mcp_config = home.join(".cursor/mcp.json");
    let existed = mcp_config.is_file();
    success &= apply_step(
        integrations::json_mcp_state(&mcp_config),
        dry_run,
        "MCP server already configured",
        &format!("would add MCP server entry to {}", mcp_config.display()),
        &format!("added MCP server entry to {}", mcp_config.display()),
        || {
            let changed = set_mcp_entry(&mcp_config)?;
            if changed && !existed {
                record_created_file(home, "skills", &mcp_config)?;
            }
            Ok(changed)
        },
        opts,
    );
    println!("│");
    success
}

fn install_antigravity(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint(
            "Google Antigravity (~/.gemini/GEMINI.md)",
            Style::new().bold(),
            opts
        )
    );
    let pointer = antigravity_pointer_path(home);
    let success = apply_step(
        integrations::antigravity_state(home),
        dry_run,
        "import already configured",
        &format!(
            "would write {} and add its @import to ~/.gemini/GEMINI.md",
            pointer.display()
        ),
        &format!(
            "wrote {} and added its @import to ~/.gemini/GEMINI.md",
            pointer.display()
        ),
        || set_antigravity_import(home),
        opts,
    );
    println!("│");
    success
}
