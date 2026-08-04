//! `topos uninstall` — remove Topos-owned entries from one or more agent
//! harnesses. Defaults to a dry-run preview; the caller decides whether to
//! actually apply (see `mod.rs`'s `--apply` handling).

use std::path::Path;

use console::Style;

use super::integrations::{
    self, antigravity_pointer_path, backup_path, claude_desktop_config_path, clear_created_files,
    delete_if_empty_and_owned, delete_text_if_blank_and_owned, remove_antigravity_import,
    remove_codex_entry, remove_copilot_block, remove_mcp_entry, remove_owned_file,
    skill_path_agents, skill_path_claude, State, SKILL_MD,
};
use crate::commands::render::{paint, RenderOptions};

fn removed(opts: RenderOptions) -> String {
    paint("●", Style::new().red(), opts)
}

fn absent(opts: RenderOptions) -> String {
    paint("○", Style::new().dim(), opts)
}

fn err(opts: RenderOptions) -> String {
    paint("✕", Style::new().red(), opts)
}

/// Shared three-way branch every uninstall step follows: nothing to do,
/// a dry-run preview, or an actual removal.
fn removal_step(
    state: State,
    dry_run: bool,
    absent_msg: &str,
    preview_msg: &str,
    removed_msg: &str,
    apply: impl FnOnce() -> Result<bool, String>,
    opts: RenderOptions,
) -> bool {
    if state == State::Absent {
        println!("│    {} {absent_msg}", absent(opts));
        return true;
    }
    if dry_run {
        println!("│    {} [dry run] {preview_msg}", removed(opts));
        return true;
    }
    match apply() {
        Ok(_) => {
            println!("│    {} {removed_msg}", removed(opts));
            true
        }
        Err(e) => {
            println!("│    {} {e}", err(opts));
            false
        }
    }
}

type Handler = fn(&Path, bool, RenderOptions) -> bool;

/// One entry per supported harness id — a data table instead of a match arm
/// per harness keeps `run` itself flat regardless of how many harnesses
/// exist.
const HANDLERS: &[(&str, Handler)] = &[
    ("claude", uninstall_claude),
    ("claude-desktop", uninstall_claude_desktop),
    ("codex", uninstall_codex),
    ("gemini", uninstall_gemini),
    ("copilot", uninstall_copilot),
    ("skills", uninstall_skills),
    ("antigravity", uninstall_antigravity),
];

pub(crate) fn run(
    home: &Path,
    selected: &[String],
    dry_run: bool,
    purge_backups: bool,
) -> Result<(), String> {
    let opts = RenderOptions::stdout();
    print_header(dry_run, opts);

    let mut success = true;
    for id in selected {
        success &= dispatch(id, home, dry_run, opts);
    }

    if purge_backups && !dry_run {
        println!("│  Purging backup files...");
        purge_backup_files(home, opts);
        println!("│");
    }

    print_summary(success, dry_run, opts)
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
            format!("┌  Topos Harness Uninstall{mode}"),
            Style::new().bold(),
            opts
        )
    );
    println!("│");
}

fn print_summary(success: bool, dry_run: bool, opts: RenderOptions) -> Result<(), String> {
    let summary = if dry_run {
        "Done. (dry run — re-run with --apply to make these changes)"
    } else {
        "Done."
    };
    println!("└  {}", paint(summary, Style::new().bold(), opts));
    if success {
        Ok(())
    } else {
        Err("one or more harnesses failed to uninstall cleanly".to_string())
    }
}

fn uninstall_claude(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint("Claude Code (~/.claude)", Style::new().bold(), opts)
    );
    let config = home.join(".claude.json");
    let mut success = removal_step(
        integrations::json_mcp_state(&config),
        dry_run,
        "no MCP server entry found",
        &format!("would remove MCP server entry from {}", config.display()),
        &format!("removed MCP server entry from {}", config.display()),
        || remove_mcp_entry(&config, false),
        opts,
    );
    if !dry_run {
        delete_if_empty_and_owned(&config, home, "claude", false).ok();
    }
    let skill = skill_path_claude(home);
    success &= removal_step(
        integrations::skill_state(&skill),
        dry_run,
        "no skill file found",
        &format!("would remove {}", skill.display()),
        &format!("removed {}", skill.display()),
        || remove_owned_file(&skill, SKILL_MD, false),
        opts,
    );
    if !dry_run {
        clear_created_files(home, "claude").ok();
    }
    println!("│");
    success
}

fn uninstall_claude_desktop(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    let config = claude_desktop_config_path(home);
    println!(
        "│  {}",
        paint(
            format!("Claude Desktop App ({})", config.display()),
            Style::new().bold(),
            opts
        )
    );
    let success = removal_step(
        integrations::json_mcp_state(&config),
        dry_run,
        "no MCP server entry found",
        &format!("would remove MCP server entry from {}", config.display()),
        &format!("removed MCP server entry from {}", config.display()),
        || remove_mcp_entry(&config, false),
        opts,
    );
    if !dry_run {
        delete_if_empty_and_owned(&config, home, "claude-desktop", false).ok();
        clear_created_files(home, "claude-desktop").ok();
    }
    println!("│");
    success
}

fn uninstall_codex(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint("Codex CLI (~/.codex)", Style::new().bold(), opts)
    );
    let config = home.join(".codex/config.toml");
    let success = removal_step(
        integrations::codex_state(&config),
        dry_run,
        "no MCP server entry found",
        "would remove [mcp_servers.topos] from ~/.codex/config.toml",
        "removed [mcp_servers.topos] from ~/.codex/config.toml",
        || remove_codex_entry(&config, false),
        opts,
    );
    if !dry_run {
        delete_text_if_blank_and_owned(&config, home, "codex", false).ok();
        clear_created_files(home, "codex").ok();
    }
    println!("│");
    success
}

fn uninstall_gemini(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint("Gemini CLI (~/.gemini)", Style::new().bold(), opts)
    );
    let config = home.join(".gemini/settings.json");
    let success = removal_step(
        integrations::json_mcp_state(&config),
        dry_run,
        "no MCP server entry found",
        &format!("would remove MCP server entry from {}", config.display()),
        &format!("removed MCP server entry from {}", config.display()),
        || remove_mcp_entry(&config, false),
        opts,
    );
    if !dry_run {
        delete_if_empty_and_owned(&config, home, "gemini", false).ok();
        clear_created_files(home, "gemini").ok();
    }
    println!("│");
    success
}

fn uninstall_copilot(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint("GitHub Copilot CLI (~/.copilot)", Style::new().bold(), opts)
    );
    let config = home.join(".copilot/copilot-instructions.md");
    let success = removal_step(
        integrations::copilot_state(&config),
        dry_run,
        "no instruction block found",
        "would remove the instruction block from ~/.copilot/copilot-instructions.md",
        "removed the instruction block from ~/.copilot/copilot-instructions.md",
        || remove_copilot_block(&config, false),
        opts,
    );
    if !dry_run {
        clear_created_files(home, "copilot").ok();
    }
    println!("│");
    success
}

fn uninstall_skills(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint(
            "Cursor & VS Code (~/.agents/skills)",
            Style::new().bold(),
            opts
        )
    );
    let skill = skill_path_agents(home);
    let mut success = removal_step(
        integrations::skill_state(&skill),
        dry_run,
        "no skill file found",
        &format!("would remove {}", skill.display()),
        &format!("removed {}", skill.display()),
        || remove_owned_file(&skill, SKILL_MD, false),
        opts,
    );
    let mcp_config = home.join(".cursor/mcp.json");
    success &= removal_step(
        integrations::json_mcp_state(&mcp_config),
        dry_run,
        "no MCP server entry found",
        &format!(
            "would remove MCP server entry from {}",
            mcp_config.display()
        ),
        &format!("removed MCP server entry from {}", mcp_config.display()),
        || remove_mcp_entry(&mcp_config, false),
        opts,
    );
    if !dry_run {
        delete_if_empty_and_owned(&mcp_config, home, "skills", false).ok();
        clear_created_files(home, "skills").ok();
    }
    println!("│");
    success
}

fn uninstall_antigravity(home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    println!(
        "│  {}",
        paint(
            "Google Antigravity (~/.gemini/GEMINI.md)",
            Style::new().bold(),
            opts
        )
    );
    let pointer = antigravity_pointer_path(home);
    let success = removal_step(
        integrations::antigravity_state(home),
        dry_run,
        "no @import found",
        &format!(
            "would remove the @import from ~/.gemini/GEMINI.md and delete {}",
            pointer.display()
        ),
        &format!(
            "removed the @import from ~/.gemini/GEMINI.md and deleted {}",
            pointer.display()
        ),
        || remove_antigravity_import(home, false),
        opts,
    );
    println!("│");
    success
}

fn purge_backup_files(home: &Path, opts: RenderOptions) {
    let candidates = [
        home.join(".claude.json"),
        claude_desktop_config_path(home),
        home.join(".codex/config.toml"),
        home.join(".gemini/settings.json"),
        home.join(".copilot/copilot-instructions.md"),
        home.join(".cursor/mcp.json"),
        home.join(".gemini/GEMINI.md"),
    ];
    for path in candidates {
        let backup = backup_path(&path);
        if backup.is_file() && std::fs::remove_file(&backup).is_ok() {
            println!(
                "│    {} removed backup: {}",
                removed(opts),
                backup.display()
            );
        }
    }
}
