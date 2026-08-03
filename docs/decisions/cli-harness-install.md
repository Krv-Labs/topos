# CLI harness install / uninstall (`topos install`)

Status: **PLANNED** (v0.4.4) — implementation owned by @sgathrid.  
Issue: [#256](https://github.com/Krv-Labs/topos/issues/256).

This document is the implementation plan. No install/uninstall code ships
in this PR; merge the plan into `release/v0.4.4` so work can proceed in
follow-up commits on this branch (or a successor PR stacked on the
release train).

## Goal

Ship interactive, idempotent agent-harness setup from the Topos CLI:

```text
topos install [--dry-run] [--yes] [harness...]
topos uninstall [--dry-run] [--yes] [harness...]
topos install status   # or: topos status install
```

Inspired by skills.sh-style TTY selection and the existing
`topos config` interactive selector
([`topos/cli/src/commands/config.rs`](../../topos/cli/src/commands/config.rs)).

## Commands

| Command | Behavior |
| --- | --- |
| `topos install` | TTY: multi-select harnesses (↑↓, space, `a`, enter). Non-TTY: require explicit harness names or `--yes --all-detected`. |
| `topos uninstall` | Default to dry-run preview of every file change; require `--apply` or confirmation to mutate. |
| `topos install status` | Per-harness ✓/○ with paths; summary `N/M active`. Prefer `--json` for agents. |

## Module layout

```text
topos/cli/src/commands/install/
  mod.rs           # clap + dispatch
  registry.rs      # HarnessId, detection, paths
  interactive.rs   # multi-select TUI (reuse console patterns from config.rs)
  plan.rs          # InstallPlan / UninstallPlan (dry-run preview)
  apply.rs         # Idempotent apply/remove
  adapters/
    claude_code.rs
    claude_desktop.rs
    codex.rs
    gemini.rs
    copilot.rs
    cursor_vscode.rs
    antigravity.rs
```

Wire into [`topos/cli/src/main.rs`](../../topos/cli/src/main.rs) as
`Command::Install` / `Command::Uninstall` (or a single `install`
subcommand with `status` / nested uninstall — prefer matching the issue
mockups: top-level `install` + `uninstall`).

## Adapter trait

```rust
trait HarnessAdapter {
    fn id(&self) -> HarnessId;
    fn detect(&self) -> Detection; // NotFound | Detected | Active
    fn plan_install(&self, topos: &ToposPaths) -> Plan;
    fn plan_uninstall(&self) -> Plan;
    fn apply(&self, plan: &Plan) -> Result<(), String>;
}
```

`ToposPaths` resolves: `topos` on `PATH`, skill source (`skills/topos`),
MCP stdio command (`topos mcp`).

## v1 harness matrix

| Harness | Config location | Install action |
| --- | --- | --- |
| Claude Code | `~/.claude/settings.json` | MCP server entry + skill link |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) | MCP server entry |
| Codex CLI | `~/.codex/config.toml` | MCP block |
| Gemini CLI | `~/.gemini/settings.json` | SessionStart hook + skill |
| Copilot CLI | `~/.copilot/copilot-instructions.md` | Instruction block |
| Cursor & VS Code | `~/.agents/skills` and/or `.cursor/mcp.json` | Skill symlink + MCP config |
| Antigravity | `~/.gemini/GEMINI.md` | `@import` line + permission rules |

If a harness’s on-disk format is unstable at implementation time, gate it
behind `--experimental` rather than blocking the release — but aim to
ship all seven.

## Safety rules

- Merge into existing JSON/TOML; never wipe unrelated MCP servers.
- Mark Topos-owned entries (e.g. `"managedBy": "topos"` or equivalent per format).
- Uninstall removes only Topos-owned entries.
- All writes: read → parse → merge → atomic write (temp + rename).
- `--dry-run` makes zero filesystem changes.

## Tests

- Fixture unit tests per adapter under `topos/cli/tests/fixtures/install/`.
- Integration: `topos install status --json` (no TTY).
- Snapshot dry-run output strings for uninstall preview.

## Docs (with implementation)

- [`skills/topos/SKILL.md`](../../skills/topos/SKILL.md) — prefer `topos install` over manual MCP setup.
- CLI `--help` for the new commands.
- Optional note in distribution docs after OpenWiki regen.

## Dependencies on the v0.4.4 train

Land **after** #258 (`--gitnexus-dir` as project root) so install docs
describe final COMPOSABLE root semantics. Stack/rebase this branch onto
`release/v0.4.4` once #266–#270 are merged.

## Acceptance (from #256)

- [ ] Interactive install multi-select in TTY
- [ ] Dry-run uninstall preview by default
- [ ] Status shows configured vs detected harnesses
- [ ] Install + uninstall idempotent
- [ ] Non-TTY path works without prompts

## Out of scope for v1

- Windows-specific Desktop paths beyond what adapters can detect cheaply
- Auto-installing the `topos` binary itself (assume already on `PATH`)
- Publishing to ClawHub from this command
