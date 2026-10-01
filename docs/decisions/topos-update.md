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

### Delegate the upgrade; never download over a package manager

| Channel | How it is upgraded |
| --- | --- |
| binary install | `TOPOS_UPDATE=1 curl … install.sh \| bash`, inherited stdio |
| homebrew | `brew upgrade topos`, inherited stdio |
| cargo / source / python | printed, not run |

Two reasons this is a delegation and not a downloader:

1. **Homebrew.** Writing over `/opt/homebrew/Cellar/topos/<version>/bin/topos`
   produces a binary `brew` does not know about. The next `brew upgrade`
   reverts it silently, and the user concludes the update did nothing.
2. **Checksums and atomicity already exist.** `install.sh:557-585` downloads to
   a temp file, verifies the SHA-256 against the release's `checksums.txt`, and
   only then `mv`s into place. Reimplementing that in Rust would duplicate
   ~60 lines of reviewed shell and add a second implementation to keep correct
   about atomic replacement. One source of truth is worth more than
   in-process control here.

`TOPOS_UPDATE=1` is the installer's existing "this is an upgrade, don't stop to
ask" switch (`install.sh:321-325`). Without it the script would prompt about
the other installs it discovers — including the one just checked — asking a
question the user already answered by choosing "update now".

`cargo`, `source` and `python` are advised rather than acted on. A source
checkout's working tree is not something topos can locate or safely pull, and
a pip install belongs to `uv`.

### Nothing runs unasked

`topos update` never runs a package manager because it was typed. It reports,
shows the command, and waits for a confirmation. `--yes` is the explicit
version of that confirmation. A non-terminal run **reports and exits 0 without
touching anything** — an agent or CI job has nobody to answer a prompt, and
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

The cache is written by rename rather than in place, because the other surface
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