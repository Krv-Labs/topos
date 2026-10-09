# `topos update`

`topos update` reports which distribution channel a `topos` binary came from
and offers to upgrade it. It supersedes the position in
`installation.rst` that updating "remains a package-manager responsibility" —
the package manager is still the one that does the work, but topos now says
which one to reach for.

## The problem

`install.sh` already answered part of this. When it found another `topos` on
`PATH`, it printed a channel-correct hint (`install.sh:152-251`) and warned
that `PATH` order decides which binary runs. But that is a shell script run
from a pipe, it only fires during an install, and it is invisible to the MCP
server. The two questions a person actually has — *which channel is this, and
what upgrades it?* — had no answer in the binary itself.

A Python-era `topos update` existed and was deleted in the Rust rewrite
(`CHANGELOG.md:365-392`). Its design notes are worth keeping: channel-aware
upgrade paths, a 24-hour notice throttle, `TOPOS_NO_UPDATE_NOTICES=1`, and an
install-layout notice for conflicting executables. This restores all of it
rather than inventing a second set of names.

## Decisions

### Classify by path shape, never by asking the package manager

`Channel` is derived from where the file sits — `/Cellar/`, `~/.cargo/bin`,
`/target/release/`, `site-packages`. This is the same rule `install.sh:152-184`
already uses, and it is deliberate that `brew` is never invoked: a read-only
report should not require the package manager to be installed and working, and
should not be slow because of it. `install.sh` avoided calling `brew` for
exactly this reason ("preflight must stay fast and offline-friendly").

### Upgrade by channel

| Channel | How it is upgraded |
| --- | --- |
| binary install | Download the release asset, verify SHA-256, and replace the selected executable |
| homebrew | Run brew upgrade topos with inherited stdio |
| cargo / source / python / unknown | Print a manual command |

The binary downloader uses native progress bars and a unique staging file in
the target directory. It checks the release checksum before renaming the file
over the selected executable. Concurrent updates cannot share a staging file.
Homebrew retains ownership of its files; Topos runs its upgrade command.

Classification resolves symlinks before inspecting known install locations.
Python virtual environments are recognized by pyvenv.cfg. Unrecognized
locations are reported as unknown and cannot receive an automatic download.
Probing an installed executable for its version has a two-second deadline.
A failed or timed-out probe reports an unknown version.

### Nothing runs unasked

`topos update` never runs a package manager because it was typed. It reports,
shows the command, and waits for a confirmation. `--yes` is the explicit
version of that confirmation. A non-terminal run without --yes **reports and exits 0 without
replacing binaries** — an agent or CI job has nobody to answer a prompt, and
downloading a binary nobody agreed to is not a safe default. `update_e2e.rs`
asserts the prompt glyphs are absent from every redirected run.

### The card reuses the existing TTY grammar

`menu.rs` is the only interactive selector in the codebase, and a new renderer
would drift from it. So `update/report.rs` composes `SelectStep`,
`run_select`, `run_menu` and `run_confirm`, which means `topos update` is
graphically indistinguishable from `topos install`:

```
┌  Topos update available
│  0.7.1 is published for macos-arm64.
│
│  Current   0.7.0
│  Target    0.7.1
│  Source    binary install · /home/dev/.local/bin/topos
│  Command   TOPOS_UPDATE=1 curl -fsSL https://docs.krv.ai/topos/install.sh | bash
│
│  ↑↓ move · enter confirm · esc skip
│
│ ❯ ● Install update now (0.7.1)   (0.7.0 → 0.7.1)
│   ○ Continue with current version (re-run `topos update` later)
└
```

One change to shared code was needed: `SelectOption.label`, `SelectStep.title`
and `SelectStep.keys` were `&'static str`, which cannot hold a discovered path
or a version. They are now `Cow<'static, str>`, so the static tables in
`config.rs` still allocate nothing while a caller with runtime strings can pass
one.

The `Command` row wraps with a hanging indent rather than truncating.
`render::wrap_text` hard-truncates any single word longer than the budget, which
would silently shorten a URL and make the command uncopyable, so the budget is
widened to the longest token first.

### One install is a single-select; several are a checkbox

The question is genuinely binary for one install ("upgrade now?") and genuinely
plural for several ("which of these?"), so the two cases use the two selectors
the codebase already has: `run_select` and `run_menu`, the latter matching
`topos install`'s multi-harness checkbox.

When more than one install is found, all are listed with path, version and
channel, and the running one is marked. This is the case the passive
install-layout notice exists for: the user upgraded one binary while a
different one still shadowed it.

### Notices: two surfaces, one daily budget

A new release is surfaced twice, to the two audiences that exist:

- **CLI** — one line on stderr after a successful command, so it lands below the
  output rather than scrolling away above it. Stderr keeps `--json` on stdout
  machine-readable, which `composable.rs:92-97` already relies on.
- **MCP** — the markdown channel of `to_tool_result`, which all 59 tool results
  funnel through. This is the channel that reaches both the agent and the user;
  stderr from a stdio server goes to a log pane nobody opens.

`instructions` was rejected for the reason `formatting.rs:753-756` already
documents for the staleness banner: instructions are built during the handshake,
when a just-started process has just read the current version. A verdict
computed then is always false or permanently baked in.

The MCP banner is latched with an `AtomicBool`, the same once-per-process
idiom as `NOTED_FILE_HASH` in `tools/assess.rs`, so one server prepends it to
one result rather than all fifty-nine.

Three brakes govern the unprompted traffic:

| Brake | Effect |
| --- | --- |
| `TOPOS_NO_UPDATE_NOTICES`, `CI`, non-terminal | no check, no notice |
| `checked_at` ≥ 24h | at most one `curl` a day; the common path is one `stat` |
| `notified_at` ≥ 24h | at most one notice a day, across both surfaces |

Fetch and display are budgeted separately because they buy different things.
Fetching daily but showing hourly would nag; fetching hourly but showing daily
would hammer a URL nobody asked for.

**The MCP server reads the cache and never fetches.** Blocking a tool call on a
network round-trip to decide whether to *mention* an update is a bad trade for
a server that promises to answer fast. A missed notice is a much better failure
than a hung one. The CLI does the fetching, so anyone who also uses the CLI
still hears about releases.

`topos update` and `topos mcp` are excluded from the passive notice — the first
*is* the notice. So is `topos uninstall`: the cache lives in
`~/.local/state/topos`, the directory uninstall prunes, so offering a notice
after a teardown would recreate the state the user just removed.

### One state directory, resolved once

`install.json`, `install.sh`'s provenance file and the update cache all live in
`~/.local/state/topos`. The shell honoured `XDG_STATE_HOME` and the Rust ledger
did not, so a user who set the variable got two directories and `topos uninstall`
pruned only one. The resolver now lives in `topos_mcp::paths` and both callers
delegate to it, which also fixes the relative-path case the XDG spec calls
invalid.

The cache is written through a unique staging file and rename rather than in place, because the other surface
reads it from another process concurrently and must never see half a JSON file.
A corrupt cache reads as "never checked", which is the correct response.

### Removal is not offered

`topos update` reports several installs and updates whichever you pick; it does
not remove one. Removing a binary is destructive, and `rm`-ing a file or
uninstalling a formula is a worse default than an extra sentence of advice. If
the stray turns out to matter, `PATH` order and `topos status` already say
where things are.

### Placement

The shared logic lives in the `topos-mcp` crate, next to `build_info.rs`, which
is already the home of self-identity concerns. `topos/cli` depends on
`topos-mcp` (`topos/cli/Cargo.toml:18`), so both surfaces share one
implementation with no new crate wiring.

`topos-engine` would be the more honest home for the release logic — it is a
shared concern rather than an MCP one — but it needs new dependency edges in
both directions. The smaller diff won; the layering compromise is recorded here
rather than hidden.

`semver` is the only new dependency, and it has none of its own. The
hand-rolled comparator in `gitnexus.rs:136-143` drops pre-release suffixes, so
it cannot order `0.7.1-rc.1` against `0.7.0` — exactly the question this asks.

## Verification

- `cargo test -p topos-mcp --lib update` — channel classification, semver
  precedence, redirect parsing, both throttles, the notice claim, cache
  round-trip and corrupt-cache recovery.
- `cargo test -p topos --test update_e2e` — the real binary against a fake
  `curl` on `PATH`, so both the update-available and up-to-date branches run
  without a release ever being cut. Asserts no prompt glyph in any redirected
  run, and that CI writes no cache.
- `install_e2e` and `main.rs`'s root-help test cover the shared-code changes:
  the `Cow` widening, the state-directory resolver, and the uninstall cleanup.

The once-per-24-hour notice is unit-tested rather than e2e: the notice requires
a terminal, and the e2e suite has no pty, so an end-to-end assertion would pass
whether or not the throttle worked.
---

# MCP file resolution

Recorded separately from the update command, because the next person to hit
`topos_begin_refactor` failing with "an absolute path is required" will ask
where the boundary came from, and the answer is not obvious from the code.

## The problem

An MCP server is spawned by its host, so its working directory is the host's
choice, not the user's. With no `TOPOS_MCP_FILE_ROOT` configured, the server
derived its boundary by walking up from that directory looking for a project
marker. On the machine this was diagnosed against, four `topos mcp` processes
were running with four different working directories, three of them pointing at
a repository other than the one being edited.

Meanwhile `topos install` writes only `command` and `args` — it never writes
`TOPOS_MCP_FILE_ROOT` — so the resulting "an absolute file or directory path is
required" error was the *default* for every unconfigured host, not an edge case.

## The decision

A relative path resolves against `TOPOS_MCP_FILE_ROOT`, else the project the
server was started in, else fails with an error naming the directory it walked up
from.

This is stricter than the behaviour it replaces. An absolute path with no
configured root previously only had to have a project marker somewhere above it;
a relative path is now resolved against a known root and held inside it, with
the check applied after `canonicalize()` so a symlink out of the root fails too.
Absolute paths still take their project from the path itself and are never
re-rooted — silently re-pinning a server to the host's cwd would be worse than
the error it replaces.

## Why not MCP `roots`

`roots` is the obvious answer: ask the host which directories it is working in.
It is deliberately not used.

SEP-2577 (merged 2026-05-15) deprecated `roots`, `sampling` and `logging`,
citing low adoption and that roots overlap with "tool parameters and server
configuration". The spec's direction is to pass paths in tool parameters — which
is what absolute-path-first already does — with `roots` retained as a bridge
for hosts that still advertise it. Building on a deprecated capability, on an
SDK (rmcp 3.1.2) that exposes no server→client `roots/list` API at all, would
be building on something scheduled to disappear.

So the remaining ambiguity — a relative path resolved against a startup
directory that is not the intended repository — is handled by disclosure rather
than prevention, because it is not decidable from inside the server: nothing in
the request says which project was meant. `resolution_note` reports the absolute
file that was read, on **both** the markdown and `structuredContent.warnings`
channels, and fires only when the base was genuinely ambiguous: never for an
absolute path, and never when `TOPOS_MCP_FILE_ROOT` chose the base explicitly.
A note that fired on every call would be noise an agent learns to ignore.

`topos://build` also reports *where* its root came from, so a mis-pinned server
is visible rather than silent.

## Verification

- `cargo test -p topos-mcp --lib security` — resolution rules, containment,
  symlink escapes, and that a configured root still bounds every path.
- `cargo test -p topos-mcp --test path_resolution` — six protocol-level tests
  that start the built binary in a controlled cwd and drive it over real
  JSON-RPC. The unit tests cannot catch a tool that passes its raw parameter
  around the guard, and there are 46 such call sites across six tool modules.

Both were checked as regression coverage rather than assumed:

- reverting the resolver to its pre-fix behaviour fails 3 of the 6
- reverting only `append_path_note` fails exactly 1 — the markdown assertion,
  and nothing else
