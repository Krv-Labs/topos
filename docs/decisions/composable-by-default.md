# COMPOSABLE scored by default — CLI and MCP standardized

Status: **SHIPPED** (v0.4.0, unreleased). `topos evaluate` (CLI) and
`topos_evaluate_file`/`topos_evaluate_project` (MCP) all now detect
GitNexus, check `.gitnexus` freshness, and generate/refresh it when
missing or stale, before scoring — so COMPOSABLE is reachable with no
extra step in the common case. This document records why, and the
shape of the change.

## Context

Before this change, SIMPLE and SECURE always ran, but COMPOSABLE was
opt-in everywhere:

- The CLI's `evaluate` command never attached a `ModuleDependencyGraph`
  at all — its own doc comment called this out explicitly as
  unfinished follow-up, "not a bug," scoped out of issue #147.
- The MCP tools (`topos_evaluate_file`/`topos_evaluate_project`) would
  attach an MDG *if* `.gitnexus` happened to already exist, but never
  generated one themselves. An agent had to notice `coupling_available:
  false`, read the `blocked_by`/`next_tool` guidance, call
  `topos_generate_depgraph` in a separate round trip, then re-evaluate.

That two-step dance was correct but easy to skip — an agent (or a
human running the CLI) could go a whole session only ever seeing
SIMPLE/SECURE, never realizing a third of Topos's signal was sitting
unscored. Given COMPOSABLE is one of Topos's three pillars and a stated
differentiator over single-axis linters, defaulting to "try to score
all three" rather than "score one axis unless asked" is the right
default.

## Decision

**Make GitNexus detection/generation the default everywhere `evaluate`
runs**, with an explicit opt-out for anyone who wants the old behavior
back (CI without network access, very large repos where a human wants
to control exactly when generation happens, etc.).

The resolve-or-generate decision is **one function**, not duplicated
per surface: `topos_mcp::evaluation::ensure_gitnexus_dir`
(`crates/topos-mcp/src/evaluation.rs:107`). Given an override path (or
none), a project root, a `skip` flag, and a `capture` flag, it:

1. Returns the existing `.gitnexus` dir unchanged when `skip=true`
   (reproducing the pre-change read-only behavior exactly).
2. Otherwise classifies state via the already-existing
   `depgraph_status` staleness check (content hash → commit SHA →
   mtime walk).
3. On `present`, or on a state generation can't fix (`invalid_dir`,
   `schema_mismatch`, `branch_not_indexed`, `load_error`), does
   nothing further — the existing `gitnexus_warnings`/
   `composable_contract_signals` machinery already explains those
   cases well; no need to duplicate that.
4. On `missing`/`stale`: checks `gitnexus_available()` ($PATH scan);
   if absent, returns a `generation_note` explaining that and telling
   the caller how to install it. If present, calls
   `generate_depgraph(project_root, capture, None)` (bounded by
   `TOPOS_DEPGRAPH_TIMEOUT`, default 300s) and reports failure via the
   same `generation_note` mechanism if the run itself fails.

Both the CLI (`crates/topos-cli/src/commands/evaluate.rs`'s
`resolve_composable_mdg`, line 137) and the MCP tools
(`crates/topos-mcp/src/tools/evaluate.rs`'s `evaluate_file_sync`/
`evaluate_project_sync`, lines 188/294) call this same function. The
CLI and MCP crates cannot drift apart on *whether* to generate; they
only differ in what they do with the resolved dir afterward:

- The CLI parses the MDG **once per run** and mutates its public
  `target_file` field per file in a directory walk — cheap, and
  avoids reparsing the whole graph N times for N files.
- MCP's existing `load_dep_graph` caches per `(dir, target_file,
  branch, mtime)` — a good fit for a long-lived server handling
  arbitrary single-file calls, but the wrong shape for a CLI's bulk
  walk (each new `target_file` would be a fresh cache miss). This
  divergence is intentional, not an oversight: it's the one place the
  two surfaces still specialize, because their call patterns are
  genuinely different, not because the underlying policy differs.
- `capture` differs by surface: `false` for the CLI (streams
  GitNexus's own subprocess output live — a human is watching an
  interactive command), `true` for MCP (collects it into the result
  instead, since there's no terminal to stream to over a stdio
  transport already carrying the protocol).

### Fail-open, always

Every failure mode — GitNexus not installed, the `gitnexus analyze`
subprocess failing, a schema mismatch, an invalid override — degrades
to "COMPOSABLE not scored, here's why" rather than failing the whole
evaluate call. This was true before this change (a missing
`.gitnexus` was never an error) and stays true after it: adding
generation to the default path could not be allowed to turn "COMPOSABLE
unavailable" into "evaluate is now broken" for anyone without GitNexus
installed. `warnings` (CLI: `stderr`) always explains the specific
reason.

### Tool annotations

`topos_evaluate_file`/`topos_evaluate_project` moved their MCP tool
annotations from `read_only_hint: true` / `open_world_hint: false` to
`false` / `true` — they now genuinely write `.gitnexus/` to disk and
shell out to an external process by default, which the old annotations
promised callers would never happen. The new annotations match
`topos_generate_depgraph`'s existing ones exactly, since the two tools
now do the same category of thing under the hood.
`topos_evaluate_file` also became `async` + `spawn_blocking` (mirroring
`topos_evaluate_project`, which already did this for its CPU-bound
walk) so a slow first-time generation on a large repo can't stall the
MCP transport for other concurrent calls.

### Opt-out

- CLI: `--no-composable` (skip entirely) / `--gitnexus-dir <dir>`
  (override the default `<cwd>/.gitnexus`).
- MCP: `no_composable: bool` / `gitnexus_dir: string` on both
  `EvaluateFileInput` and `EvaluateProjectInput`
  (`crates/topos-mcp/src/schemas.rs:242,289`).

## Alternatives considered

**Keep MCP read-only and just sharpen the guidance.** The existing
`blocked_by`/`next_tool`/`risk_flags` agent-contract machinery
(`composable_contract_signals`, `state_guidance`) already gives an
agent a precise, structured "call `topos_generate_depgraph` next"
signal — this was seriously considered as sufficient on its own. Ruled
out because it still requires two tool calls for the common case, and
because GitNexus is explicitly designed to run against large repos
(its own analyze step is the expensive part, not Topos's use of it) —
the latency concern that would normally argue for staying read-only is
smaller in practice than it looks. Standardizing on one behavior
across CLI and MCP was judged more valuable than saving one round trip
of latency risk. The sharpened guidance from this alternative wasn't
wasted — it's still exactly what fires on the (now rarer) cases where
generation genuinely can't happen.

**Scope to `topos_refactor`/`topos_assess_*` too.** Out of scope for
this change. Those tools (`crates/topos-mcp/src/tools/{refactor,
assess}.rs`) still require an explicit/pre-existing `gitnexus_dir` for
their COMPOSABLE-dependent targets (`dependencies`/`process`). The ask
that drove this change was specifically about `evaluate` parity between
CLI and MCP; extending the same default to the refactor-suite and
assess tools is a reasonable, cheap follow-up (they'd call the same
`ensure_gitnexus_dir`) but a separate decision, since those tools have
different risk/latency profiles (e.g. `topos_assess_*` runs on every
edit-verify cycle, where repeatedly re-checking freshness has a
different cost/benefit than a one-shot `evaluate` call).

## Testing

- `crates/topos-mcp/src/evaluation.rs`'s `ensure_gitnexus_dir_*` tests:
  `skip=true` reproduces the old plain-resolve behavior; an override
  outside the project root is rejected before any availability check
  or subprocess call (deterministic regardless of whether GitNexus
  happens to be installed on the test machine); GitNexus absent from
  `$PATH` degrades to a `generation_note` without shelling out (guarded
  to skip itself if the test machine happens to have GitNexus
  installed, rather than asserting a false negative).
- `crates/topos-cli/src/commands/evaluate.rs`'s
  `classify_with_representations_scores_composable_when_mdg_present`
  test: an in-memory MDG changes the classified dimensions to include
  `composable`; absent, it doesn't.
- Manual end-to-end check (no GitNexus installed): `topos evaluate`
  prints the install notice and still reports SIMPLE/SECURE;
  `--no-composable` suppresses the notice entirely; a hand-built fresh
  `.gitnexus` fixture (skipping the real subprocess) produces a scored
  `composable` row with no warnings.
- Full workspace suite (`cargo test --workspace`) and `cargo clippy
  --workspace --all-targets -- -D warnings` stay green throughout —
  no existing test stubs a fake `gitnexus` on `$PATH`, so none of the
  new default-generation logic changes existing test outcomes on a
  machine without GitNexus installed (true of CI).
