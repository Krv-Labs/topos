# CLI harness install / uninstall (`topos install`)

Status: **IN REWRITE** (v0.4.4) — owned by @sgathrid. The first draft shipped
behind this branch reported 7/7 harnesses configured when verification against
real clients found only 3 of them actually loaded the server. This document
describes the replacement design, which is frozen as a module contract and
landing module by module; the harness table, config formats, entry shape and
binary resolution are implemented, and the display layer (`configure.rs`,
`uninstall.rs`, `status.rs`), `state.rs` and `residue.rs` are the remaining
pieces. Sections below say explicitly where they describe the contract rather
than landed code.
Issue: [#256](https://github.com/Krv-Labs/topos/issues/256).

The draft followed the harness-installer pattern from
[sgathrid/brian](https://github.com/sgathrid/brian)
(`wikicli/lifecycle/{integrations,install,uninstall}.py`): a flat per-harness
`if/elif` dispatch in one `integrations.rs`. That file is deleted. Several of
its behaviors are the defects this rewrite exists to fix, and it scored 0.0 on
Topos's own SIMPLE gate at cyclomatic complexity 140. What replaces it is a
table plus three format modules, with no per-harness branching anywhere in the
command layer.

## Goal

Register the Topos MCP server in every agent harness on the machine, and take it
back out without a trace — with an entry that actually spawns, and honest
reporting when it will not.

```text
topos install   [--dry-run] [--all] [harness...]
topos uninstall [--dry-run] [--yes] [--all] [--purge-backups] [harness...]
topos status         [--json]
topos install status [--json]
```

Interactive selection is styled after the existing `topos config` selector
([`topos/cli/src/commands/config.rs`](../../topos/cli/src/commands/config.rs)),
with checkboxes instead of a single choice.

## Commands

| Command | Behavior |
| --- | --- |
| `topos install` | TTY, no args: interactive multi-select (↑↓/`j`/`k` move, space toggle, `a` toggle-all, enter confirm, esc cancel), pre-checking harnesses that are active, need repair, or are detected on disk. Non-TTY: requires explicit harness ids or `--all`, and errors clearly if neither is given. |
| `topos install --dry-run` | Reports what each selected harness would get; writes nothing. |
| `topos uninstall` | Interactive: multi-select harnesses, then one confirm block on stderr listing what will be removed with **No** on top and pre-selected (arrow down to **Yes**). `--yes` skips the prompt. `--dry-run` prints the plan and stops. With no ids in a non-TTY, falls back to every harness whose state is not `Absent`. |
| `topos uninstall --purge-backups` | Additionally deletes the `.topos.backup` files earlier installs left behind. Candidates are generated from the harness table, never a hardcoded list. |
| `topos status` / `topos install status` | Every harness with its state, its per-harness message, any caveat note, and a residue section. `--json` for agents. |

`status` is deliberately reachable both ways: `topos install status` is where it
grew up, and `topos status` is what people type.

Interactivity is a three-way gate rather than a boolean, resolved once from
which of the three standard streams are terminals
([`mod.rs`](../../topos/cli/src/commands/install/mod.rs)):

- **Interactive** — stderr is a tty. Prompt on it. Both the multi-select menu and
  the plan+No/Yes confirm read `Term::stderr()`, so the gate and the read must
  be the same stream, or a pty-allocated CI job prompts and then blocks forever.
- **Headless** — no stream is a tty. True CI; uninstall applies.
- **Ambiguous** — stderr is redirected but stdout or stdin is a tty: a human who
  typed `topos uninstall 2>log.txt`. The preview went somewhere they may never
  see and there is nowhere left to prompt, so uninstall reports and exits
  non-zero rather than destructively applying.

## Harness matrix

Nine harnesses, one artifact each: the single MCP server registration in that
harness's user-scope config. Nothing else is written.

| id | Name | Config file | Format | Detected by |
| --- | --- | --- | --- | --- |
| `claude` | Claude Code | `~/.claude.json` | `mcpServers.topos` | `~/.claude` is a dir |
| `claude-desktop` | Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | `mcpServers.topos` | parent dir exists |
| `codex` | Codex CLI | `~/.codex/config.toml` | `[mcp_servers.topos]` | `~/.codex` is a dir |
| `gemini` | Gemini CLI | `~/.gemini/settings.json` | `mcpServers.topos` | `~/.gemini` is a dir |
| `copilot` | GitHub Copilot CLI | `~/.copilot/mcp-config.json` | `mcpServers.topos` | `~/.copilot` is a dir |
| `cursor` | Cursor | `~/.cursor/mcp.json` | `mcpServers.topos` | `~/.cursor` is a dir |
| `vscode` | VS Code | `~/Library/Application Support/Code/User/mcp.json` | `servers.topos` (JSONC) | parent dir exists |
| `antigravity` | Google Antigravity | `~/.gemini/config/mcp_config.json` | `mcpServers.topos` | see below |
| `pi` | pi | `~/.pi/agent/mcp.json` | `mcpServers.topos` | `~/.pi` is a dir |

Claude Desktop and VS Code use `~/.config/...` on Linux and `%APPDATA%\...` on
Windows ([`paths.rs`](../../topos/cli/src/commands/install/paths.rs)). Claude
Desktop is not distributed for Linux at all; the conventional Linux path is kept
only so `status` and `uninstall` can clean up an earlier install. Every path
function takes `home` explicitly and reads no globals besides `%APPDATA%`, so
the end-to-end suite can drive the real binary against a scratch `$HOME`.

`detect` pre-checks the interactive menu and never gates writing — asking for a
harness by id always writes it.

Three rows carry judgment calls worth recording:

**Claude Code** writes user-scope MCP servers to `~/.claude.json`, *not*
`~/.claude/settings.json`. That file is hooks, permissions and statusLine only.
`~/.claude.json` is what `claude mcp add` itself writes.

**pi** writes to `~/.pi/agent/mcp.json`, *not* `~/.pi/agent/settings.json`. That
file is pi's own settings — theme, provider, transport — with a documented key
set that has no `mcpServers` in it; an entry written there is read by nothing.
pi is also the only harness with no MCP client of its own ("No MCP. […] build an
extension that adds MCP support",
[`packages/coding-agent/README.md`](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/README.md)).
MCP is resolved by the
[`pi-mcp-adapter`](https://github.com/nicobailon/pi-mcp-adapter) extension,
which searches six locations; `~/.pi/agent/mcp.json` is the only one that is
pi's alone. `~/.config/mcp/mcp.json` and `~/.agents/mcp.json` rank higher in its
precedence order but are shared across tools, and a per-harness installer has no
business owning a file every other agent also reads. So pi carries the second
`note`: unconditional, because nothing on disk says whether the adapter is
installed, and a bare `✓` on an entry no client will read is the same silent
failure the Antigravity note exists to prevent.

**Antigravity** is not detected by "`~/.gemini` exists" — Gemini CLI creates that
directory, so keying off it would pre-check Antigravity for every Gemini user.
Detection is `~/.gemini/config/.migrated`, or any of `~/.gemini/antigravity`,
`-cli`, `-ide` being a directory. Antigravity also carries the other
`note`: when the migration marker is absent and a real (non-symlink)
`mcp_config.json` still sits in one of those data directories, Antigravity's
next launch whole-file-replaces `~/.gemini/config/mcp_config.json` from its app
data dir **with no merge**. Tested, and a pre-written topos entry was destroyed.
Install still writes, but a bare `✓` on an entry that is about to be discarded
is exactly the silent failure this rewrite exists to remove, so the note renders
in install output *and* in `status`, including on `Active` entries.

topos never writes `~/.gemini/antigravity/`, `-cli/`, `-ide/` or `-backup/`. On a
migrated machine `~/.gemini/antigravity/mcp_config.json` *is a symlink* into
`config/mcp_config.json` — residue of Antigravity's own `migrate.go` — so writing
there would sever the migration symlink and guarantee the entry is ignored.
There is no dual-write.

## The MCP entry, and why it has no `type` field

```json
{
  "mcpServers": {
    "topos": {
      "command": "/opt/homebrew/bin/topos",
      "args": ["mcp"]
    }
  }
}
```

VS Code is the sole exception: `servers.topos` additionally carries
`"type": "stdio"`. Codex gets the same two fields as `[mcp_servers.topos]`.

Three properties of that shape are load-bearing
([`artifact.rs`](../../topos/cli/src/commands/install/artifact.rs)):

- **No `type` on the six plain-JSON harnesses.** The bare `{command, args}` pair
  loads and connects on Copilot CLI 1.0.73, Gemini CLI 0.53.1 and Claude Code
  2.1.221. Gemini's shipped schema has no `required` array and Claude Desktop's
  has no `type` property at all. No literal value is portable either — Copilot's
  own writer emits `"type": "local"` where Claude Code's emits `"stdio"`.
- **`args` is exactly `["mcp"]`.** A bare `topos` prints usage and exits, which
  surfaces to the client as `-32000: Connection closed`.
- **Comparison is field-wise on `command` and `args`, never whole-value
  equality.** Clients normalize entries and add keys of their own; an equality
  check pins those harnesses at "stale" forever and rewrites the file on every
  run. Writes are field-wise too: only the keys topos owns are set, and a
  client-added `"type": "local"` survives untouched.

## Binary path resolution, and why it never canonicalizes

The draft recorded the literal string `"topos"`. That cannot spawn from
GUI-launched macOS apps, which inherit only `/usr/bin:/bin:/usr/sbin:/sbin` —
Claude Desktop, Cursor and Antigravity, on every install channel. An absolute
path is the only portable answer.

Picking *which* absolute path is the actual problem
([`binary.rs`](../../topos/cli/src/commands/install/binary.rs)):

1. `env::current_exe()`. A failure here is a hard error — there is no fallback
   to a bare `"topos"`, because a config entry that cannot spawn is worse than a
   loud error, it fails later and somewhere else.
2. If the result is relative (`_NSGetExecutablePath` is permitted to return one),
   join `env::current_dir()`. Join, not canonicalize.
3. Prefer the `$PATH` entry naming the same physical file, if there is one.
   First match wins — that is what a bare `topos` resolves to, so it is the
   spelling the user already means. Empty `$PATH` entries (POSIX-empty means
   cwd, which would yield a relative path) and non-absolute entries are dropped;
   a candidate must be a regular file, which is what skips a *directory* named
   `topos` and a *broken symlink* named `topos`.

**Nothing recorded is ever canonicalized.** `/opt/homebrew/bin/topos` is a
symlink into `../Cellar/topos/<version>/bin/topos`; recording the Cellar path
means the next `brew upgrade` silently breaks every harness. Canonicalizing for
*comparison* is required and fine — it is how the Windows branch of `same_file`
works — but its output never reaches a config.

**Identity is read with `fs::metadata`, which follows symlinks, never
`fs::symlink_metadata`.** On a Homebrew `$PATH` entry `symlink_metadata` yields
the symlink's own inode rather than the executable's — measured 240632144 versus
240632143 — so the alias search could never match and every Homebrew install
would record its Cellar path. Exactly one legitimate `symlink_metadata` call
exists in this module tree, in `fsops::atomic_write`.

The alias search runs unconditionally on every platform. A
`cfg(target_os = "linux")` gate would leave the Homebrew-pinning bug reachable on
exactly the machines that hit it.

Drift is therefore identity-based, never a string compare: one physical file
legitimately has several spellings (measured `/r2/bin/resolve2` versus
`/r2/lnkbin/resolve2`), and a string compare would rewrite every config on every
run. `drift` reports the first of these that holds, as a sentence fragment
usable directly in status output:

| Condition | Reported as |
| --- | --- |
| not absolute | `` `topos` is not an absolute path `` |
| `fs::metadata` errors | `` `<path>` no longer exists `` |
| not a regular file | `` `<path>` is not a file `` |
| Unix: no execute bit | `` `<path>` is not executable `` |
| not the same physical file | `` `<path>` is a different binary than the running topos at <binary> `` |

The draft's bare `"topos"` is relative, so it fails the first rule — which is how
an install written by the draft self-heals on the next `topos install`.

## The four states

| State | Glyph | Meaning | What to do |
| --- | --- | --- | --- |
| `Active` | `✓` green | Entry present, ours, and its `command` resolves to this binary. | Nothing. |
| `Incomplete` | `↻` orange | Ours, but in need of repair: path drift, or a VS Code entry missing `"type": "stdio"`. | `topos install`. **Never** "run uninstall" — there is nothing here for a user to clean up by hand. |
| `Conflict` | `▲` red | The file will not parse, the `topos` key holds something topos did not write, or a VS Code `mcp.json` carries comments. | topos reports the path and writes nothing. |
| `Absent` | `○` dim | No entry. | `topos install <id>`. |

`Incomplete` and `Conflict` always carry a `detail` explaining why; `Active` and
`Absent` take their message from the harness table, so a `✓` says *what* is
configured rather than a bare "configured".

Comments in a VS Code `mcp.json` are a refinement worth noting: they only force
`Conflict` when a write would be needed. A correct entry in a commented file is
still `Active` — it simply cannot be rewritten. When a write *is* needed, the
`Conflict` message carries the exact JSON entry to paste, so the fix does not
require opening an editor twice.

Ownership is by file name (`topos` / `topos.exe`) plus `args == ["mcp"]`,
deliberately not requiring the recorded path to still resolve: a drifted entry —
including the draft's bare `"topos"` — is still ours to repair or remove.

## Safety rules

The governing invariant: **topos mutates only files it registers an MCP server
into.** Prose instruction blocks, `@import` lines, `GEMINI.md` and skill files
are reported by `residue.rs` and never written, modified or deleted.

- Merge into existing JSON/TOML; never wipe unrelated MCP servers or other keys.
  Foreign sibling entries and unrelated top-level keys survive install and
  uninstall, under test.
- The `topos` key is the ownership marker. Install and uninstall only ever read,
  write or remove `mcpServers.topos` / `servers.topos` / `[mcp_servers.topos]`.
  A hand-made entry under that key is a `Conflict`: install refuses and uninstall
  leaves it alone.
- MCP keys *other* than `topos` whose command points at the topos binary
  (including a hand-rolled `topos-mcp`) are reported as duplicate
  registrations — two of them mean duplicate tool names and two `topos mcp`
  processes — but are never renamed or removed. They are the user's entries.
- Codex's `config.toml` is edited through `toml_edit`, so comments, key order and
  formatting survive an install/uninstall cycle untouched.
- Every write is read → parse → merge → temp file → rename. **The rename follows
  symlinks and replaces the target.** Users symlink `~/.claude.json`,
  `~/.gemini/settings.json`, `~/.codex/config.toml` and `~/.cursor/mcp.json` into
  dotfile repos (stow/chezmoi); renaming over the symlink would convert it to a
  regular file, orphan the dotfile repo, and leave that file behind after
  uninstall.
- `--dry-run` (and the uninstall preview) makes zero filesystem changes.
- Re-running install on a correct entry writes nothing at all — `apply` returns
  "already correct" rather than rewriting.

## Backups

Each write that needs one snapshots the previous contents to
`<name>.topos.backup`, cleared by `topos uninstall --purge-backups`.

The rule is **back up only content topos did not write**, and the *caller*
decides — `atomic_write` must not infer it from `path.is_file()` the way the
draft did. Verified failure: after install, corrupting the recorded command and
re-running install replaced `.claude.json.topos.backup` with a copy that already
contained the stale entry, destroying the pristine pre-install snapshot.
Self-healing makes that a routine event, not an edge case. In practice this
means a backup is taken only when the inspected state was `Absent`; a repair of
our own `Incomplete` entry passes `backup = false`. A backup is also skipped when
the file did not exist at all.

## Directory tracking and leave-no-trace

*Contract; `state.rs` is the module still landing.*

`~/.local/state/topos/install.json` (`%APPDATA%\topos` on Windows) records what
install brought into existence, so uninstall removes exactly that much and no
more:

```json
{ "harnesses": { "<id>": { "createdFiles": [...] } }, "createdDirs": [...] }
```

The draft's flat `{"<id>": {...}}` shape is tolerated and upgraded on read.

Directories are created one level at a time rather than with `create_dir_all`,
precisely so the caller knows which ancestors it created. Pruning removes them
child before parent, and only when a directory is *effectively* empty — holding
nothing but `.DS_Store` (present in `~/.cursor`, `~/.copilot` and
`~/.codex/skills` on a real machine, and impossible to reproduce in a scratch
`$HOME`) or our own `*.topos.backup` / `*.topos.tmp` leftovers. Shared
directories are never candidates even if topos created them: `$HOME`,
`~/.local`, `~/.local/state`, `~/.config`, `~/Library`,
`~/Library/Application Support`, `%APPDATA%`.

`install.json` is topos's own file: its writes never take a backup and never
recurse into directory recording. Clearing the last harness does *not* delete it
— `createdDirs` has to survive until pruning is done. Uninstall's order is fixed:

1. remove entries
2. purge `.topos.backup` files (only with `--purge-backups`)
3. read `createdDirs`
4. prune them
5. delete `install.json`
6. prune the state directory last

## Detect and warn: residue

*Contract; `residue.rs` is the module still landing.*

Some things topos will never modify must still not be invisible: artifacts left
by the earlier draft of this branch, and hand-made duplicate registrations. The
scan is strictly read-only.

| Path | Condition | Why topos will not touch it |
| --- | --- | --- |
| `~/.copilot/copilot-instructions.md` | contains `<!-- topos:start -->` | shared with other tools |
| `~/.gemini/GEMINI.md` | a line whose trimmed form starts with `@import` and mentions `topos-skill.md` | shared with Gemini CLI |
| `~/.gemini/topos-skill.md` | exists | draft-era skill copy |
| `~/.claude/skills/topos/SKILL.md` | exists | ClawHub / openclaw own skill distribution |
| `~/.agents/skills/topos/SKILL.md` | exists | openclaw's namespace, tracked in `~/.agents/.skill-lock.json`; harness skill dirs are symlink farms into it |
| every harness config | a non-`topos` MCP key pointing at the topos binary | the user's entry |

Skill-file advice points at `openclaw skills uninstall` or manual removal, never
at a topos command.

## Module layout

```text
topos/cli/src/commands/install/
  mod.rs         # clap args, target resolution, the interactivity gate, dispatch
  harness.rs     # the 8-row table — the only place harnesses differ
  paths.rs       # per-OS config locations, all taking `home` explicitly
  artifact.rs    # the three config shapes; State/Inspection; ownership
  json_entry.rs  # mcpServers.topos and VS Code's servers.topos
  toml_entry.rs  # [mcp_servers.topos] via toml_edit
  binary.rs      # which absolute path to record
  fsops.rs       # atomic writes, backups, JSONC stripping, directory pruning
  state.rs       # ~/.local/state/topos/install.json
  residue.rs     # read-only scan for things we report but never touch
  configure.rs   # topos install
  uninstall.rs   # topos uninstall
  status.rs      # topos status / topos install status
  menu.rs        # multi-select TTY checkbox list
```

`configure.rs`, `uninstall.rs` and `status.rs` hold **no per-harness
branching** — they iterate `HARNESSES` and the table carries the differences.
That is the structural difference from the draft's single `integrations.rs`, and
the reason the SIMPLE gate is now part of the definition of done: every module
here must score ≥ 0.40 under `topos evaluate --language rust`.

Wired into [`topos/cli/src/main.rs`](../../topos/cli/src/main.rs) as top-level
`Command::Install` / `Command::Uninstall` / `Command::Status`, with `status` also
a nested subcommand of `install`.

`serde_json` gains `features = ["preserve_order"]` in
[`topos/cli/Cargo.toml`](../../topos/cli/Cargo.toml). Without it every key in a
user's `~/.claude.json` is alphabetically re-sorted on install. It has to be
declared there rather than inherited: `resolver = "2"` keeps tree-sitter's
build-dependency copy from unifying the feature.

## Tests

Inline `#[cfg(test)]` unit tests per module, tempdir-based, matching the existing
style in `commands/config.rs`. The cases that exist because something actually
broke:

- A broken symlink named `topos` on `$PATH` is skipped. This is the assertion
  that fails under `symlink_metadata`.
- A directory named `topos` on `$PATH` is skipped.
- An empty `$PATH` entry (`"/a::/b"`) never yields a relative path, even when
  standing in a directory that holds the executable.
- Not on `$PATH` → the input is returned unchanged, with an explicit
  anti-vacuity assertion that canonicalizing *would* have changed it.
- Two different `$PATH` spellings of one physical file do not drift.
- A client reordering our keys and adding `"type": "local"` stays `Active`, and
  a second `apply` writes nothing.
- Repairing drift does not replace the pristine `.topos.backup`.
- A hand-made entry under the `topos` key is a `Conflict`, survives uninstall
  byte for byte, and makes `apply` error.
- A commented VS Code `mcp.json` is a `Conflict` whose message contains the
  entry to paste, and the comments are still there afterward.
- A symlinked config stays a symlink and its target receives the content.
- `prune_dirs` stops at a directory holding a real file, removes one holding
  only `.DS_Store`, and writes nothing under `dry_run`.
- A bare `~/.gemini` directory does not pre-check Antigravity; an unmigrated
  Antigravity install warns; its back-compat symlink does not.

All `$PATH`-mutating assertions live in a single `#[test]` fn: `cargo test` is
threaded and `set_var` is process-global.

Still to come with the display layer: an end-to-end suite driving the real binary
against a scratch `$HOME`, covering install → status → uninstall across all eight
harnesses with foreign content preserved throughout.

## Acceptance

#256's list, restated where the draft's wording described something that never
existed, with its single "install + uninstall idempotent" item split in two
because the two halves now depend on different modules. The Windows item is
added by this rewrite.

Nothing here is ticked yet: the display layer is where every one of these
becomes reachable from the command line, and it is the layer still landing. The
parenthetical on each item is what *is* done and tested underneath it.

- [ ] Interactive install multi-select in TTY (menu, state-aware pre-checking
      and id validation land and are tested; blocked on `configure.rs`)
- [ ] Dry-run uninstall preview by default (`Artifact::remove` honors `dry_run`
      under test; blocked on `uninstall.rs`)
- [ ] Status shows every harness with its state, its caveat notes, and a residue
      section — reachable as both `topos status` and `topos install status`,
      `--json` for agents. ("Configured vs detected" lives in the interactive
      menu, where `detected` is the hint for an unconfigured harness that is
      present on disk; status reports state, not detection.) Blocked on
      `status.rs` and `residue.rs`.
- [ ] Install is idempotent: re-running on a correct entry performs no write
      (true and tested at the artifact layer, where a second `apply` returns
      "already correct"; blocked on `configure.rs`)
- [ ] Uninstall is idempotent and leaves no trace, including created directories
      (blocked on `state.rs` and `uninstall.rs`)
- [ ] Non-TTY path works without prompts (target resolution and the error for
      "no ids and no `--all`" land and are tested; blocked on `configure.rs`)
- [ ] Both commands work on Windows (`paths.rs` and `binary.rs` have their
      Windows branches; unexercised in CI)

## Out of scope

- **Skill distribution.** ClawHub / Hermes / openclaw own it, via
  [`.github/workflows/clawhub-publish.yml`](../../.github/workflows/clawhub-publish.yml)
  and `openclaw skills install @Krv-Labs/topos`. `topos install` writes no
  `SKILL.md` anywhere and the draft's `include_str!` of it is gone.
- **Prose instruction blocks and `@import` lines.** Detected and reported, never
  written or deleted.
- **Project-scope and workspace-scope MCP config.** User scope only.
- **Auto-installing the `topos` binary.** It is assumed already installed; this
  command only records where it is.
- **Publishing to ClawHub from this command.**

Windows is *not* out of scope any more — see the last acceptance item.

## Changes from the draft

This is the part worth reading. Each entry is a defect the draft shipped, not a
preference.

- **Skills left the command entirely.** The draft `include_str!`'d
  `skills/topos/SKILL.md` into the binary and wrote it into each harness's skill
  directory. That duplicates a distribution channel that already exists and
  works: ClawHub publishes the canonical skill and `openclaw` installs it into
  `~/.agents/skills/topos/`, with the harness skill dirs as symlink farms into
  that namespace and a `~/.agents/.skill-lock.json` tracking it. Two writers for
  one file is a conflict generator with no upside. Pre-existing draft-written
  skill files are now reported by `residue.rs` with advice pointing at
  `openclaw skills uninstall`.
- **Copilot moved from a prose block to an MCP entry.** The draft wrote a
  `<!-- topos:start -->` marker block into `~/.copilot/copilot-instructions.md`.
  That file is shared with other tools and holds no MCP configuration at all —
  it never registered a server. Copilot CLI reads MCP servers from
  `~/.copilot/mcp-config.json`, which is what topos writes now. The instruction
  file is detected and reported, never modified.
- **Antigravity moved from `GEMINI.md` to `~/.gemini/config/mcp_config.json`.**
  The draft appended an `@import` line to `~/.gemini/GEMINI.md` pointing at a
  written-out skill copy — a file shared with Gemini CLI, and again not an MCP
  config. Antigravity reads `~/.gemini/config/mcp_config.json`; verified by a
  scratch-`HOME` probe that seeded distinct marker servers in all five candidate
  directories and observed which one the client loaded. The probe also turned up
  the migration hazard and the back-compat symlink that make Antigravity the one
  harness carrying a `note`.
- **Cursor and VS Code are two harnesses, not one row.** The draft's combined
  row wrote a skill file plus `~/.cursor/mcp.json`, leaving VS Code with nothing.
  VS Code has its own config at `Code/User/mcp.json`, its own container key
  (`servers`, not `mcpServers`), its own `"type": "stdio"` requirement, and its
  own JSONC parsing problem. Sharing a row with Cursor could only ever be wrong
  for one of them.
- **The recorded command is an absolute path.** The draft wrote the literal
  `{"command": "topos", "args": ["mcp"]}`, copied from the manual
  `claude mcp add` example in `skills/topos/SKILL.md`. That is fine for a shell
  but cannot spawn from a GUI-launched macOS app, which inherits only
  `/usr/bin:/bin:/usr/sbin:/sbin` — so Claude Desktop, Cursor and Antigravity
  could never have worked, on any install channel. Getting the absolute path
  right then required the whole of `binary.rs`: the `$PATH`-alias preference so
  Homebrew upgrades do not break the entry, identity comparison so one file's
  several spellings do not trigger a rewrite loop, and the `fs::metadata` rule so
  the alias search can match a symlink to its target at all.
- **The backup rule moved to the caller.** `atomic_write` deciding from
  `path.is_file()` meant that any second write — which self-healing makes
  routine — overwrote the pristine pre-install snapshot with content topos had
  already written. The snapshot is the entire point of the backup.
- **"Stale" split into `Incomplete` and `Conflict`.** One state for "we can fix
  this" and "a human must look at this" produced advice that was wrong half the
  time; the draft's status also had no way to say *why*. `Incomplete` now always
  means "run `topos install`" and `Conflict` always means "topos wrote nothing,
  here is the path", each with a reason attached.
- **Entry comparison is field-wise.** Whole-value equality against a client that
  normalizes its own config reports the entry as stale forever and rewrites the
  file on every run.
- **Uninstall tracks directories, not just files.** The draft recorded created
  *files* only, so a `~/.copilot` or `~/.codex` that install brought into
  existence stayed behind. Pruning needed the `.DS_Store` allowance and the
  never-prune list to be safe, and the state file needed to outlive
  `clear_created_files` so `createdDirs` is still readable when pruning runs.
- **Writes follow symlinks.** A temp-file rename over a symlinked
  `~/.claude.json` silently converts it to a regular file and orphans the user's
  dotfile repo. Common enough (stow, chezmoi) to be a correctness bug rather
  than an edge case.
- **`integrations.rs` is gone.** A 140-cyclomatic file scoring 0.0 on Topos's own
  SIMPLE gate is not a good look for the tool that computes the score. The
  replacement is a data table plus per-format modules, with the gate itself
  (≥ 0.40 per file) as a definition-of-done criterion.
- **`serde_json` gained `preserve_order`.** Without it, installing reorders every
  key in the user's `~/.claude.json` alphabetically — a diff that looks like
  topos rewrote their whole config, because it did.
- **Windows is in scope.** Both commands previously hard-errored there. `paths.rs`
  now has `%APPDATA%` branches and `binary.rs` a Windows identity comparison.
- **`topos status` exists as a top-level command.** `topos install status` is
  where it belongs structurally; `topos status` is what people type.
