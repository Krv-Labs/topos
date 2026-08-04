# CLI harness install / uninstall (`topos install`)

Status: **DRAFT IMPLEMENTATION** (v0.4.4) — owned by @sgathrid; needs a
review pass on the schema assumptions called out in `integrations.rs`
before it ships.
Issue: [#256](https://github.com/Krv-Labs/topos/issues/256).

A first implementation lands in `topos/cli/src/commands/install/`,
following the harness-installer pattern from
[sgathrid/brian](https://github.com/sgathrid/brian)
(`wikicli/lifecycle/{integrations,install,uninstall}.py`): a flat
per-harness dispatch, marker-based idempotent edits, atomic writes with
`.topos.backup` snapshots, and an install-state file so uninstall only
ever touches what a previous `topos install` created. The sections below
are updated to describe what actually shipped; see "Changes from the
original plan" at the end for what moved and why.

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
| `topos install` | TTY, no args: interactive multi-select (↑↓/`j`/`k` move, space toggle, `a` toggle-all, enter confirm, esc cancel). Non-TTY: requires explicit harness names or `--all`. |
| `topos uninstall` | Always previews first (dry-run); `--apply` performs the removal, or (in a TTY) a `[y/N]` prompt after the preview does. |
| `topos install status` | Per-harness ✓ active / ▲ stale / ○ absent; summary `N/M active`. `--json` for agents. |

## Module layout

```text
topos/cli/src/commands/install/
  mod.rs           # clap args, target resolution (explicit/--all/interactive), dispatch
  integrations.rs  # per-harness paths, State (Active/Stale/Absent), atomic write+backup,
                    # ownership-tracking state file — the brian-derived core
  menu.rs          # multi-select TTY checkbox list
  configure.rs     # topos install: one function per harness
  uninstall.rs     # topos uninstall: one function per harness, mirrors configure.rs
  status.rs        # topos install status
```

No `adapters/` directory or `HarnessAdapter` trait: brian's own installer
uses a flat per-harness `if/elif` dispatch over shared helper functions
rather than trait objects, and porting that directly kept each harness's
install/uninstall/state-check logic next to its counterpart instead of
split across a trait impl per file. `ToposPaths`-style resolution wasn't
needed either — the MCP entry is the literal `{"command": "topos", "args":
["mcp"]}` already documented in `skills/topos/SKILL.md`'s `claude mcp add`
example (PATH-based, matching how the skill tells users to configure it
by hand today), and the skill content is `include_str!`'d from
`skills/topos/SKILL.md` into the binary — a real filesystem checkout of
this repo isn't available once `topos` is installed globally.

Wired into [`topos/cli/src/main.rs`](../../topos/cli/src/main.rs) as
top-level `Command::Install` / `Command::Uninstall`, with `status` as a
nested clap subcommand of `install` (`topos install status`).

## v1 harness matrix

| Harness | Config location | Install action |
| --- | --- | --- |
| Claude Code | `~/.claude.json` (**not** `settings.json` — that file is hooks/permissions only) | MCP server entry + skill file at `~/.claude/skills/topos/SKILL.md` |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS; `~/.config/Claude/...` kept as a cleanup target on Linux, where Desktop isn't shipped) | MCP server entry |
| Codex CLI | `~/.codex/config.toml` | `[mcp_servers.topos]` table via `toml_edit` (preserves comments/formatting) |
| Gemini CLI | `~/.gemini/settings.json` | MCP server entry |
| Copilot CLI | `~/.copilot/copilot-instructions.md` | `<!-- topos:start/end -->` marker-delimited instruction block |
| Cursor & VS Code | `~/.agents/skills/topos/SKILL.md` + `~/.cursor/mcp.json` | Skill file + MCP server entry |
| Antigravity | `~/.gemini/GEMINI.md` + `~/.gemini/topos-skill.md` | `@import` line pointing at a written-out skill copy |

Every harness shipped in v1; none needed an `--experimental` gate.

## Safety rules

- Merge into existing JSON/TOML; never wipe unrelated MCP servers or other keys (verified: a foreign `mcpServers` entry and unrelated top-level keys both survive install + uninstall in `integrations.rs`'s tests).
- The `topos` key itself is the ownership marker for MCP entries — install/uninstall only ever touch `mcpServers.topos` / `[mcp_servers.topos]`.
- A separate install-state file (`~/.local/state/topos/install.json`) records which config *files* `topos install` created from scratch, so uninstall only deletes a now-empty file if Topos was the one who created it — a pre-existing file that happens to end up empty is left alone.
- A written skill file is only ever deleted by uninstall if its content still matches exactly what was written; a user edit is preserved and reported, not clobbered.
- All writes: read → parse → merge → atomic write (temp file + rename), with a `.topos.backup` snapshot of the previous contents (`--purge-backups` on uninstall clears those).
- Dry-run (the uninstall default, and `topos install --dry-run`) makes zero filesystem changes — covered by a test that diffs file contents before/after a dry-run uninstall.

## Tests

- Inline `#[cfg(test)]` unit tests in `integrations.rs` (tempdir-based, matching the existing style in `commands/config.rs`) covering: JSON/TOML/text marker round-trips, stale-entry detection and removal, dry-run no-ops, owned-file content-match removal, and install-state ownership tracking.
- Manually verified end-to-end against a scratch `$HOME`: real install/uninstall across all seven harnesses, idempotent re-install, foreign JSON/TOML/Markdown content preserved throughout, and backup purge.
- Not added: fixture files under `topos/cli/tests/fixtures/install/` — the tempdir-based inline tests cover the same ground with less scaffolding.

## Docs (with implementation)

- `skills/topos/SKILL.md` still documents the manual `claude mcp add` path; pointing it at `topos install` instead is a follow-up once this is out of draft.
- CLI `--help` covers the new commands (`topos install --help`, `topos uninstall --help`).

## Dependencies on the v0.4.4 train

Landed after #258 (`--gitnexus-dir` as project root) and the rest of
#266–#270 — this branch was rebased onto the `release/v0.4.4` tip once
those merged.

## Acceptance (from #256)

- [x] Interactive install multi-select in TTY
- [x] Dry-run uninstall preview by default
- [x] Status shows configured vs detected harnesses (`topos install status`, `--json` for agents)
- [x] Install + uninstall idempotent (re-running install reports "already configured" / "already up to date" with no writes; verified manually and via the `integrations.rs` round-trip tests)
- [x] Non-TTY path works without prompts (explicit harness names or `--all`; errors clearly if neither is given)

## Out of scope for v1

- Windows-specific Desktop paths beyond what's needed to clean up a prior install
- Auto-installing the `topos` binary itself (assume already on `PATH`)
- Publishing to ClawHub from this command

## Changes from the original plan

- **Claude Code's MCP config location.** The original sketch pointed at
  `~/.claude/settings.json`; that file is hooks/permissions/statusLine
  only. User-scope MCP servers live in `~/.claude.json`, matching what
  `claude mcp add` itself writes.
- **No session-start hooks.** brian's installer injects wiki context via
  `SessionStart` hooks on Claude Code/Gemini/Codex. Topos has no context
  to inject at session start — its integration point is MCP tools — so
  those harnesses get an MCP server entry instead of a hook, and the
  hook-marker-stripping machinery in brian's `integrations.py`
  (`WIKI_HOOK_MARKERS`, the Codex `[features]`/`writable_roots` state
  machine) wasn't ported at all.
- **Codex via `toml_edit`, not regex.** brian edits `config.toml` with
  regex over raw text because Python's `tomllib` is read-only. Rust's
  `toml_edit` (already a dependency, used by `commands/config.rs`) reads,
  edits, and writes back while preserving comments/formatting, so the
  Codex adapter is a plain table insert/remove instead of a hand-rolled
  block-matching regex.
- **Skill delivery is a file write, not a symlink.** brian's installer
  always runs from inside a persistent git checkout, so it symlinks
  `internal/skills/wiki-context` into each harness's skills directory.
  A globally-installed `topos` binary has no such checkout, so the skill
  content is `include_str!`'d into the binary at compile time and written
  out (not symlinked) to each target path; uninstall only deletes it if
  the on-disk content still matches exactly, so a user edit is preserved.
