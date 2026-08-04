# CLI harness install / uninstall (`topos install`)

Status: **HARDENING** (v0.4.4) — owned by @sgathrid. The draft shipped in
#256; the review pass it asked for is tracked in "Hardening pass" below.
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
  mod.rs           # clap args, target resolution (explicit/--all/interactive)
  integrations.rs  # the HARNESSES table + Artifact and its dispatch
  edits.rs         # one read-modify-write pair per config format, atomic+backed-up
  ownership.rs     # which files install created; owned deletion
  menu.rs          # multi-select TTY checkbox list
  configure.rs     # topos install: one loop over the table
  uninstall.rs     # topos uninstall: mirrors configure.rs
  status.rs        # topos install status
```

No `adapters/` directory or `HarnessAdapter` trait. The first draft used a
flat per-harness dispatch ported from brian's `if/elif` chain — one
`install_*` and one `uninstall_*` function each — which spread the harness set
across seven parallel id-keyed lists; see "Hardening pass" for why that was
replaced by a single `HARNESSES` table of artifacts rather than by a trait.

The skill content is `include_str!`'d from `skills/topos/SKILL.md` into the
binary, since a filesystem checkout of this repo isn't available once `topos`
is installed globally. The MCP entry records the absolute path of the running
binary rather than the bare `topos` that `skills/topos/SKILL.md`'s `claude mcp
add` example uses: harnesses launched by the desktop environment have no shell
`PATH` (finding 2).

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

## Hardening pass

Findings from a review of the shipped draft, each reproduced against a
scratch `$HOME` before being written down. Ordered by severity.

### P0 — correctness and safety

1. **`~/.claude.json` loses its `0600` mode.** `atomic_write` writes a fresh
   temp file (default umask) and renames it over the target, so the mode is
   reset rather than preserved. `~/.claude.json` ships `0600` and holds OAuth
   account state and project history; installing widens it to `0644`.
   *Fix:* carry the existing file's permissions onto the temp file before the
   rename.

2. **A bare `topos` command only resolves for harnesses that inherit a shell
   `PATH`.** Claude Desktop, Cursor, and Antigravity are launched by the
   desktop environment, not a login shell, so a `topos` in `~/.local/bin` or
   `~/.cargo/bin` fails to spawn and the server silently never starts.
   *Fix:* write the absolute `std::env::current_exe()` path. Detection stays
   location-agnostic by accepting any `mcpServers.topos` whose `args` are
   `["mcp"]` and whose `command` basename is `topos`, so re-installing from a
   different location is not reported as stale.

3. **The report claims writes and removals that did not happen.**
   `apply_step` / `removal_step` discard the `Ok(bool)` their closures
   return and print the applied message unconditionally. Reproduced: with a
   locally edited `~/.claude/skills/topos/SKILL.md`, `topos uninstall claude
   --apply` prints `● removed …/SKILL.md` while the file is left in place.
   `remove_owned_file` behaves correctly — only the reporting lies, which
   makes the "a user edit is preserved and reported" rule above untrue.
   *Fix:* branch on the returned bool and report preserved-vs-removed.

4. **`topos uninstall --apply` with no target removes every harness.** In a
   non-TTY with no names and no `--all`, uninstall falls back to "everything
   not `Absent`" and, with `--apply`, mutates all seven without a
   confirmation. `install` rejects exactly this input.
   *Fix:* make the non-interactive path require explicit names or `--all`,
   matching install.

### P1 — data safety and robustness

5. **`.topos.backup` is overwritten by every later write,** so the pristine
   pre-Topos snapshot is gone after the second install (verified: an install
   over a stale entry replaced the original backup with the stale content).
   *Fix:* first write wins — never overwrite an existing backup.

6. **A full re-serialization reorders every key.** `serde_json`'s default map
   is a `BTreeMap`, so merging one entry alphabetizes a 260 KB
   `~/.claude.json` that Claude Code itself also writes.
   *Fix:* enable `serde_json`'s `preserve_order` feature for the CLI crate.

7. **Blank-file deletion bypasses the ownership check.**
   `remove_copilot_block` deletes the instructions file whenever the residue
   is blank, and `remove_antigravity_import` does the same to `GEMINI.md`
   (`fs::remove_file(&gemini_md).ok()`), even though
   `delete_text_if_blank_and_owned` exists for precisely this. The
   antigravity pointer file is also deleted unconditionally, unlike every
   other written file, which goes through `remove_owned_file`'s content-match
   policy.
   *Fix:* route both through the existing owned-deletion helpers.

8. **`--all` creates directories for harnesses that are not installed,**
   leaving `~/.copilot`, `~/.cursor`, and friends behind on machines that
   never had them. *Fix:* `--all` selects detected harnesses; naming a
   harness explicitly still forces it.

9. **`purge_backup_files` keeps its own path list** and omits the skill files
   and the antigravity pointer, so their backups survive `--purge-backups`.

10. **The install-state file ignores `XDG_STATE_HOME`,** hardcoding
    `~/.local/state/topos/install.json`.

### P2 — structure (`topos evaluate --language rust`)

`topos evaluate topos/cli/src/commands/install -r --language rust` scores the
module 🥈 SILVER / 46%. Five of six files are over the SIMPLE cyclomatic gate
of 15 (measured on the production code, with the inline test modules stripped
so the tests don't inflate the number):

| File | `cfg.cyclomatic` | worst single function |
| --- | --- | --- |
| `integrations.rs` | 140 | 8 |
| `uninstall.rs` | 37 | 4 |
| `mod.rs` | 36 | 9 |
| `menu.rs` | 32 | 10 |
| `configure.rs` | 29 | 4 |
| `status.rs` | 13 | 8 |

`--info` ranks the same two changes on every failing file: cut branching, and
rebalance an instability of 1.00.

A gate of 15 per file is not this crate's working standard — `config.rs` is at
51 and `evaluate/summary.rs` at 62 — and no single function here is above 10.
So the goal is not the gate; it is the duplication the score is pointing at.

The branching has one source. The harness set is spelled out in seven
parallel lists keyed by id — `SUPPORTED`, `harness_name`, `detect_dir`,
`integration_state`, `configure::HANDLERS`, `uninstall::HANDLERS`, and
`purge_backup_files`'s candidates — so adding a harness means seven edits and
missing one is silent. Finding 9 is already that bug.

*Fix:* collapse them into one `const HARNESSES: &[Harness]` table where each
entry names its label, its detection directory, and the artifacts it owns
(`JsonMcp`, `TomlMcp`, `MarkerBlock`, `OwnedFile`, `ImportLine`). Install,
uninstall, status, detection, and backup purging each become a single loop
over `harness.artifacts`, which deletes the fourteen near-identical
`install_*` / `uninstall_*` functions. This removes a layer rather than
adding one: no trait, no adapter per harness, one table.

The flat per-harness dispatch was ported from brian on purpose (see "Module
layout"), and it was the right call while the artifact kinds were still being
discovered. They have stopped moving — five kinds cover all seven harnesses —
so the table is now the smaller design.

#### Result

The table landed, and `integrations.rs` then split three ways — the same move
`render.rs` documents making out of `evaluate.rs` ("printing is a separate
concern ... bundling both pushed the file's cyclomatic total over the SIMPLE
gate"):

| Module | Holds |
| --- | --- |
| `integrations.rs` | the `HARNESSES` table, `Artifact`, and their dispatch |
| `edits.rs` | one read-modify-write pair per config format |
| `ownership.rs` | which files install created, and owned deletion |

Production code, tests stripped from both sides:

| File | `cfg.cyclomatic` | worst function |
| --- | --- | --- |
| `edits.rs` | 90 | 7 |
| `integrations.rs` | 39 | 7 |
| `mod.rs` | 37 | 5 |
| `menu.rs` | 32 (untouched) | 10 |
| `uninstall.rs` | 28 | 6 |
| `configure.rs` | 21 | 5 |
| `ownership.rs` | 20 | 5 |
| `status.rs` | 17 | 4 |

- Worst file: **140 → 90**. SIMPLE average **21% → 27%**.
- Production lines: **1730 → 1652**, while absorbing all ten fixes.
- `configure.rs` 342 → 117 and `uninstall.rs` 358 → 158, since the fourteen
  per-harness functions became two loops.
- Tests: 8 → 16, all of them regression tests for the findings above.

Two honest caveats. `edits.rs` at 90 is now the outlier, but its worst single
function is 7 — it is a collection of small independent primitives, not hard
logic, so splitting it further would be chasing the metric rather than the
complexity. And `menu.rs` was left alone: nothing in the findings touches it,
and its 32 is the keypress/render dispatch a checkbox list needs.

### Not changing

- The harness config locations themselves. Each was re-checked against the
  matrix above and is correct; Copilot CLI keeps its marker block rather than
  an MCP entry.
- The `--apply`-instead-of-`--dry-run` shape of uninstall. Preview-by-default
  is the safer asymmetry and matches #256's mock-up.
