# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Merged PRs add their entries to `[Unreleased]` as they land; the release PR renames
that section. See the Git History & Release Convention in [`.agents/AGENTS.md`](.agents/AGENTS.md).

## [Unreleased]

### Added

- **`topos install pi`** ([#345](https://github.com/Krv-Labs/topos/issues/345)) — ninth supported harness, registering `mcpServers.topos` in `~/.pi/agent/mcp.json` and detected by `~/.pi`. pi has no MCP client of its own, so the entry is read by the [`pi-mcp-adapter`](https://github.com/nicobailon/pi-mcp-adapter) extension; install output and `topos status` both carry a note saying so, because a `✓` on an entry no client reads is a silent failure. See [`docs/decisions/cli-harness-install.md`](docs/decisions/cli-harness-install.md) § Harness matrix.

### Fixed

- **`topos evaluate --json` and every MCP structured payload are byte-deterministic** ([#332](https://github.com/Krv-Labs/topos/issues/332)) — map keys now come out in sorted order instead of `RandomState` order, so two runs over unchanged code produce identical bytes. The CLI builds `serde_json` with `preserve_order` and links the MCP crate, so every `HashMap` reaching the wire serialized in a different order on every process, defeating diffs and caches downstream. Every serialized map — `scores`, `dimensions`, `pillars`, `raw_metrics`, `interpretation`, `metric_locations`, `score_deltas`, `metric_deltas`, and the project rollups — is a `BTreeMap`. No shape change: JSON objects carry no ordering contract and the emitted JSON Schema is identical.

### Changed

- **Passing pillars with a near-zero score render `! WARN`, not `✓ PASS`** ([#350](https://github.com/Krv-Labs/topos/issues/350)) — `✓ PASS 0%` read as a contradiction. In the multi-file card the status keys off the weakest file, since that row prints both AVG and MIN and either one at near-zero next to a `✓` is the same contradiction. Policy is untouched: gates alone decide pass/fail, the threshold feeds no verdict or exit code, and `--json` is unchanged.

### Breaking

- **Graphify integration removed** (~1,680 production lines). Every signal it provided was already available from the GitNexus/MDG graph Topos loads for COMPOSABLE, usually at better fidelity — full `startLine`/`endLine` spans instead of point anchors, a first-class `Community` node label with `MEMBER_OF` edges instead of a bare Louvain field, and a continuous `confidence: f64` + `reason` instead of a three-value enum. It had no production consumer, no CI coverage, and no agent-facing recommendation, and it wrapped a pre-1.0 external tool with a documented history of schema breaks. See [`docs/decisions/refactor-suite.md`](docs/decisions/refactor-suite.md) § Removed target and issue [#325](https://github.com/Krv-Labs/topos/issues/325).

  Removed public surface — **clients calling any of these will break**:

  - **MCP tool `topos_generate_graphify_graph`** is gone. There is no replacement; nothing else generated that graph.
  - **`topos_refactor(target="graphify")`** is gone, along with the `graphify_dir` parameter on `RefactorInput` and the `graphify_orphan` / `graphify_fragile_edge` hotspot kinds. `topos_refactor` now accepts `cycles`, `dependencies`, and `process` only; a `graphify` target is rejected as an invalid enum value.
  - **CLI `topos graphify generate` and `topos graphify orphans`** are gone. `topos graphify` now errors as an unknown subcommand.
  - The `TOPOS_GRAPHIFY_TIMEOUT` and `GRAPHIFY_OUT` environment variables are no longer read.

- **MCP tool count drops from 18 to 17**, and the tool-definition **context budget shrinks accordingly**: the wire surface every agent session pays for on every request went from 39,584 to 38,668 characters (~9,896 → ~9,667 approximate tokens). The ratchet in `topos/mcp/src/context_budget.rs` was moved **down** from 40,500 to 39,500. Resource count (7) and prompt count (1) are unchanged.

### Changed

- **MCP SDK upgraded to `rmcp` 3.x.** ([#264](https://github.com/Krv-Labs/topos/issues/264), [#324](https://github.com/Krv-Labs/topos/pull/324), [#337](https://github.com/Krv-Labs/topos/pull/337)) The server now advertises and serves the MCP `2026-07-28` revision: a client may open the session with `server/discover` as its first frame instead of `initialize`, and gets back supported protocol versions, capabilities, and instructions. The legacy `initialize` handshake is unchanged, as are all 18 tools, 7 resources, and 1 prompt — same names, same schemas, same results. Every revision the SDK knows (`2024-11-05` through `2026-07-28`) still negotiates as itself: rmcp 3.1.0 made the server's supported-version list bound `initialize` negotiation rather than only feed `server/discover`, so that list is now pinned explicitly in the handler to keep an SDK upgrade from moving which revisions Topos will speak.
- **The security overlay decides before it parses** ([#322](https://github.com/Krv-Labs/topos/issues/322), [#331](https://github.com/Krv-Labs/topos/pull/331)) — `overlay_for_file` / `overlay_for_source` built a `ProgramMorphism` before checking the two conditions that make an overlay possible at all (parseable, and SECURE actually failing). Every SECURE-passing file therefore paid a full tree-sitter parse whose only consumer was an early `return None`, and `overlay_for_file` re-read from disk a file `classify_file` had just read. The guards now run first. Most visible in the project loop, which calls `overlay_for_file` once per file: on a clean codebase that was one wasted read + parse per file. `topos_inspect_code` on a SECURE-clean 1820-line Rust file drops from 118.5 ms to 99.8 ms (~16%). Verdicts, findings, and acknowledged risks are unchanged — verified by diffing `topos evaluate --json` across both binaries on corpora exercising both the firing and passing paths.

## [0.5.1] - 2026-08-08

### Fixed

- **`cfg.cyclomatic` remediation no longer advises an action that worsens the metric it cites** ([#286](https://github.com/Krv-Labs/topos/issues/286), [#318](https://github.com/Krv-Labs/topos/pull/318)) — `cfg.cyclomatic` is a whole-file sum, so extracting a helper preserves the decisions and adds a function, moving the number up. The guidance now names the actual levers for a whole-file sum (collapse redundant decisions, or split the file) and surfaces the trade-off against `ast.max_function_complexity` rather than implying the two align.

### Changed

- **Ranking-list sorts evaluate each row's gates once** ([#305](https://github.com/Krv-Labs/topos/issues/305), [#320](https://github.com/Krv-Labs/topos/pull/320)) — project ranking comparators called `gate_metrics_for` per comparison, cloning each row's full `raw_metrics` map and reclassifying, for O(n log n) gate evaluations. Rows are now decorated with precomputed keys once, sorted, and projected back. Ordering and list membership are unchanged (verified by comparing `topos_evaluate_project` payloads across both binaries).
- **Metric-location generation parses once per request** ([#306](https://github.com/Krv-Labs/topos/issues/306), [#319](https://github.com/Krv-Labs/topos/pull/319)) — SIMPLE and NAVIGABLE location collectors each built their own `ProgramMorphism` for the same single-file request. One morphism is now built and shared. Response shape and span ordering unchanged.
- **CFG builder tracks continue targets directly** ([#307](https://github.com/Krv-Labs/topos/issues/307), [#317](https://github.com/Krv-Labs/topos/pull/317)) — after switch `break` handling moved to `break_stack`, `LoopContext` held a single field. Replaced with `continue_stack: Vec<usize>`. No edge-contract change.

## [0.5.0] - 2026-08-07

### Added

- **MCP Registry publishing** now runs after a successful PyPI release using GitHub OIDC; the publisher binary is version-pinned and checksum-verified before receiving publication credentials.
- **Gate-first project ranking lists** add `hard_fails`, `leaf_composable_zeros`, and `maintainability_giants`; `worst_files` remains deprecated for one release. Agent guidance starts from the first actual gate failure rather than a score-only ordering.
- **Full LadybugDB ingestion** discovers every node-label table, loads real node properties, and preserves `confidence` and `reason` on `CodeRelation` edges.
- **NAVIGABLE — a fourth evaluation pillar for agentic cognitive load.** Measures how expensive a file is for an LLM agent to read, reason over, and safely change, via **Semantic Compositional Divergence**: `Σ depth(u)·ln(1 + fanout(u))` over the scope-forming nodes inside each function, gated on the per-function maximum (`nav.max_function_divergence`). Computed from the same UAST as SIMPLE, so it needs no external tooling and is always available across all six supported languages.

  NAVIGABLE is deliberately orthogonal to SIMPLE: SIMPLE counts *branches*, NAVIGABLE measures *nesting*. A flat function scores `0.0` no matter how many branches it has, while the same branches nested four deep score badly. Nesting is what keeps predicting LLM task failure once code length is controlled for. Ternaries and short-circuit boolean operators are excluded — expression-level branching opens no block, and counting it would re-measure SIMPLE.

  A failing gate resolves to the offending functions worst-first with real line spans, so it becomes an actionable `refactor_target` (`extract_helper`) rather than an un-fixable verdict.

- **🏆 PLATINUM medal tier** — awarded when all four pillars pass.

- **`navigable` accepted everywhere a pillar is named** — `--priority navigable`, `.topos.toml` rankings, the `topos config` interactive selector, and MCP `preferences.ranking`.

### Fixed

- **CFG metric correctness** restores nesting depth at branch joins, excludes module preambles such as Go `package` clauses from implicit callables, and aligns equivalent fixtures across supported languages. `cfg.nesting_depth` remains diagnostic and `cfg.cyclomatic` remains advisory.
- **CFG switch and try/catch/finally semantics** route switch `break` to the match join, model handlers as exception targets, and ensure normal, return, and throw paths traverse nested `finally` blocks correctly.
- **C++ declaration and GitNexus fingerprint parity** maps header prototypes and pure-virtual declarations as functions and writes fingerprints beside the resolved branch-scoped graph store.
- **CLI four-pillar ranking compatibility** uses `RANKING_LEN` throughout the info browser so NAVIGABLE rankings compile and display consistently.
- **File-level COMPOSABLE now gates only outward dependency burden:** `mdg.fan_out <= 10`. Fan-in measures responsibility/change-impact radius, so a popular interface or shared utility no longer fails merely for having many callers. All dependency readings remain in `inspect`; instability, main-sequence distance, and fan-in also remain scored advisories that drive refactor guidance. The cap was calibrated on 2,979 production files across Python, Rust, TypeScript, and MCP cohorts (4.3% failures with equal ecosystem weight); the literature supports file/program-unit coupling as a risk signal, not a universal numeric cutoff. See [`docs/decisions/file-level-composable.md`](docs/decisions/file-level-composable.md).
- **COMPOSABLE distance mode reads the right graph.** `coupling_gate_input` decides whether an instability reading is real from module-level `mdg.coupling` (`Ca + Ce`), not from symbol-level `mdg.fan_in`/`mdg.fan_out`. A pure trait/interface module makes no calls, so its fan is legitimately zero while it carries real `IMPORTS` edges — `topos/engine/src/graphs/base.rs` sat exactly on the main sequence (`A = 1.0`, `I = 0.0`, `D = 0.0`) and still scored COMPOSABLE at 0.0.
- **`ProjectEvaluationResult.leaf_composable_zeros` is deprecated** — always empty, kept one release for wire compatibility. The structural-leaf carve-out existed to keep instability failures out of `hard_fails`; with instability advisory there is no hard fail to suppress.
- **Martin instability is no longer a file-level gate.** `I = Ce / (Ca + Ce)` has resolution `1 / (Ca + Ce)`; with a single coupling edge the balanced band `[0.3, 0.7]` is unreachable by construction. The reading remains visible and scored for architectural diagnosis, while any future package-stability verdict will use package-scoped measurements rather than projecting a package model onto files.

### Changed

- **Agent contracts share one prelude** for composability, parse, grade-cap, and security signals across evaluate, assess, project, and depgraph paths. The project contract keeps verification gates even when no `worst_files` rows exist.
- **`Ω` extended from 8 to 16 elements.** `G_qual` gains a fourth generator, so the subobject classifier is now a 4-cube. Medal tiers band on the count of satisfied pillars: 4 → PLATINUM, 3 → GOLD, 2 → SILVER, 1 → BRONZE, 0 → SLOP. `Omega`'s lattice operations were already generic over the cover relation and needed no change.
- **Preference weights are now `8 / 4 / 2 / 1`** down the ranking, preserving the strict lexicographic order.
- **Default ranking is now `SIMPLE ≻ NAVIGABLE ≻ SECURE ≻ COMPOSABLE`.** The two pillars an agent can always compute and always fix inside one file rank highest — both are UAST-derived and need no external tooling. `SECURE` follows (local, but zero-tolerance, so a blocker rather than a gradient). `COMPOSABLE` ranks **last** because it requires an external GitNexus graph, degrades to unavailable without one, and measures the file's interactions with the wider dependency graph — so it is the right thing for a relaxation walk to concede first when `coupling_available` is `false`. Consequences: the default `fallback_target` is `SIMPLE_NAVIGABLE` (was `SIMPLE_COMPOSABLE`), one step below `IDEAL` is `SIMPLE_SECURE_NAVIGABLE`, and `next_step` from `SLOP` is `COMPOSABLE`. The published order table in `topos://docs/preferences` changed with it.
- **`Priority::default()` is now `Simple`, not `Secure`** — it previously disagreed with MCP's `resolve_priority(None)`, which already returned `Simple`, and now matches the head of the default ranking. All three defaults agree.
- **`--priority <pillar>` with nothing configured now bases the ranking on the default preferences** rather than `Generator::ALL` declaration order. `Generator::ALL` remains display/column order, which is not a preference statement.
- Deleted the unused `WeightProfile` rather than growing it a fourth field.

### Breaking

- **Parse-blocker code normalized to `parse_failures`.** Single-file `agent_contract.blocked_by` and `risk_flags` previously emitted singular `parse_failure`; field names are unchanged.
- **Project evaluation adds three ranking-list fields** (`hard_fails`, `leaf_composable_zeros`, and `maintainability_giants`) and deprecates `worst_files` for one release.
- **`IDEAL` now requires all four pillars.** The verdict formerly named `IDEAL` — the top of the three-generator algebra — is now `SIMPLE_COMPOSABLE_SECURE`, and it bands as GOLD rather than the top tier. **CI pinned to `IDEAL` will start failing** on files that pass the original three pillars but fail NAVIGABLE.
- **Medal tiers re-grade in both directions**, since the banding is now over four pillars.
- **`LatticeElement` gains 8 variants** on the MCP wire — a JSON-schema change for clients.
- **A `priority` / `preferences` ranking must list all four pillars.** A three-element ranking in `.topos.toml` is no longer a permutation of `G_qual`; it is dropped and the default order applies, matching the best-effort contract every other malformed key already gets.
- The MCP tool-definition context budget grew from 33,736 to 39,584 chars, from the larger `LatticeElement` and `GeneratorInput` enums appearing in every schema that accepts a verdict or ranking.

### Notes

> **NAVIGABLE thresholds calibrated** on a balanced 6,390-file leaderboard corpus (2026-08-07): `max_function_divergence = 10.0` (p95 `10.37`; ~5.2% gate failure rate) and `divergence_cap = 12.0` (spans p99 across Rust `10.40`, Go `13.64`, and Python `12.31` so scores decay linearly without early flooring). Topos's 176 Rust sources remain a reference ECDF (`p95` `5.65`, p99 `8.62`, max `12.19`).

Docs figures under `docs/source/_static/figures/topos-lattice*.svg` have been redrawn for the full sixteen-element 4-cube (PLATINUM down to SLOP). The benchmark figures (`topos-medal-mix*`, `topos-typical-profiles*`, `topos-library-profiles*`, `topos-bronze-ready*`) still plot the three-pillar corpus run and will be regenerated when the benchmark is re-run under NAVIGABLE.

## [0.4.4] - 2026-08-04

### Added

- **`topos install` / `topos uninstall` / `topos status` for agent harnesses** ([#256](https://github.com/Krv-Labs/topos/issues/256), [#271](https://github.com/Krv-Labs/topos/pull/271), [#278](https://github.com/Krv-Labs/topos/pull/278)) — registers a resolvable MCP server entry across supported harnesses with preview, confirmation, dry-run, backup, and leave-no-trace uninstall behavior. See [`docs/decisions/cli-harness-install.md`](docs/decisions/cli-harness-install.md).
- **`--gitnexus-dir` / `gitnexus_dir` as COMPOSABLE project root** ([#258](https://github.com/Krv-Labs/topos/issues/258), [#270](https://github.com/Krv-Labs/topos/pull/270)) — derives freshness and generation from the graph store's parent so nested or absolute overrides analyze the intended project.

### Fixed

- **Evaluate summary floor and COMPOSABLE notices** — keeps setup output out of JSON, presents recoverable graph misses consistently, and reports a clean SLOP floor.
- **`topos evaluate` language inference for named files** ([#289](https://github.com/Krv-Labs/topos/issues/289)) — infers every supported suffix unless `--language` is explicitly supplied as a filter.
- **`cfg.nesting_depth` loop handling** ([#288](https://github.com/Krv-Labs/topos/issues/288)) — computes static depth on the forward CFG without loop back-edge inflation.
- **First-run COMPOSABLE generation with `--gitnexus-dir`** ([#287](https://github.com/Krv-Labs/topos/issues/287)) — treats an in-root store that does not exist yet as missing and eligible for generation.
- **Graphify edge loading and bounded reads** ([#214](https://github.com/Krv-Labs/topos/issues/214), [#268](https://github.com/Krv-Labs/topos/pull/268)) — supports `links` or `edges` and rejects oversized graph files before parsing.
- **Root containment for symlinks and `..`** ([#215](https://github.com/Krv-Labs/topos/issues/215), [#269](https://github.com/Krv-Labs/topos/pull/269)) — canonicalizes existing path prefixes before accepting missing leaves.
- **MCP registry metadata no longer ships a broken PyPI index URL** ([#276](https://github.com/Krv-Labs/topos/pull/276), [#277](https://github.com/Krv-Labs/topos/pull/277)) — drops duplicate index arguments and adds a CI invariant.
- **Rust locals named `raw` parse again** ([#285](https://github.com/Krv-Labs/topos/issues/285)) by upgrading `tree-sitter-rust`.

### Changed

- **Document JS/TS `switch_statement` → `MatchStmt` as intentional** ([#213](https://github.com/Krv-Labs/topos/issues/213), [#266](https://github.com/Krv-Labs/topos/pull/266)).
- **Release tags must match Cargo and wheel versions** ([#217](https://github.com/Krv-Labs/topos/issues/217), [#267](https://github.com/Krv-Labs/topos/pull/267)).

### Breaking

- **Harness install targets MCP registration only** ([#278](https://github.com/Krv-Labs/topos/pull/278)); combined skill installation is replaced by separate Cursor and VS Code MCP targets.

## [0.4.3] - 2026-07-28

### Added

- **First-class CLI terminal UX restored** ([#251](https://github.com/Krv-Labs/topos/pull/251)) — `topos evaluate` prints one stable pillar summary instead of streaming every file; TTY progress stays on stderr. `--info` adds a bounded weak-file drill-down; `--failures <pillar>` lists failing paths. New `topos config` interactively sets evaluation priority and writes `.topos.toml`.
- **Unified `--priority` flag** ([#255](https://github.com/Krv-Labs/topos/pull/255)) — `topos evaluate` and `topos config set` accept either a single pillar or a full comma-separated ranking; on-disk schema collapses to one `priority` key (legacy `preferences` still loads for one release).

### Changed

- **`topos inspect` and supporting CLI commands** — actionable single-file detail, medal tier in the evaluate footer, clearer composable-unavailable and diagnostic-score guidance; Rust CLI docs refreshed and Python-era references removed.

## [0.4.2] - 2026-07-26

### Fixed

- **Rebuild release artifacts without spurious OpenSSL linkage** — v0.4.1 macOS CLI and PyPI wheels linked Homebrew `openssl@3` paths and aborted under library validation; source fixes landed in PR [#246](https://github.com/Krv-Labs/topos/pull/246) but users still received the broken v0.4.1 binaries until this release.

### Changed

- **Drop stale OpenSSL from PyPI wheel and Glama Docker builds** — remove unused `openssl@3` / `libssl-dev` install steps from the maturin wheel job; CI already rejects macOS wheels with non-system dylib linkage.

## [0.4.1] - 2026-07-25

### Fixed

- **Project-row SECURE score now matches its pillar score under an active security overlay** — `evaluate_project`'s per-file `scores.secure` and `pillars.secure.score` could disagree when an allowlisted/acknowledged risk was in effect, because the overlay-adjusted classification used for `pillars` hard-coded the score to 1.0/0.0 instead of keeping it raw. `pillars.secure.achieved` and `lattice_element` still reflect the overlay-adjusted verdict; only the score is now always raw, matching the existing single-file `evaluate` behavior. (Closes [#232](https://github.com/Krv-Labs/topos/issues/232))

## [0.4.0] - 2026-07-25

Baseline: **`topos-mcp==0.3.12`** (last Python release on PyPI). v0.4.0 is a Rust rewrite aimed at drop-in parity for scoring semantics and agent workflows on the same inputs, with documented exceptions called out below. Items not listed under **Intentional changes** are intended to match 0.3.12 behavior; where the Rust port regressed during migration, **Fixed** entries restore Python semantics.

### Parity with `topos-mcp==0.3.12`

- **Three-pillar scoring** — same gate thresholds, lattice medals, preference walk, and assess/evaluate/compare/coverage/depgraph/refactor tool semantics when given the same file, flags, and `.gitnexus` state. All 18 MCP tools, 6 `topos://docs/*` resources, and the `topos_refactor_until_ideal` prompt are preserved (argument shape is flat top-level JSON, not a FastMCP `params` wrapper).
- **CLI surface** — `topos evaluate`, `inspect`, `compare`, structural test coverage, `depgraph generate`, and advisory `refactor`/`graphify` subcommands. Pass **`--no-composable`** (CLI) or **`no_composable: true`** (MCP) to reproduce 0.3.12's opt-in COMPOSABLE behavior (no auto-generation, read existing `.gitnexus` only).
- **SECURE allowlisting** — #168/#174 consistency ported to Rust: taint findings resolve via `sink_info.sink_type`; allowlist matching is per-finding; one-off `--allow` acknowledges risk without stripping the grade cap.
- **SIMPLE complexity** — decision-form counting restored (#142): short-circuit booleans, ternaries, comprehensions, `with`/`assert`, per-handler `except`/`catch`. Gate threshold unchanged (`<= 10.0`).
- **Rollups and gates** — multi-file verdict is the lattice meet (∧) of per-file Ω verdicts; files missing a dimension representation (e.g. no MDG) no longer drag the project rollup down; refactor suggestions use the same gate inputs as the scorer; gates fail closed on `NaN`; `taint_flow_paths` is deterministic.
- **CLI inspect JSON** — parseable on first run in a fresh repo; text mode exits `1` on parse failure; `--json` scores are 0–100 (Python scale). `TOPOS_DEPGRAPH_TIMEOUT` / `TOPOS_GRAPHIFY_TIMEOUT` disable the deadline instead of panicking on non-finite values.

### Intentional changes from `topos-mcp==0.3.12`

- **SECURE gate stays CPG-native; Sighthound is advisory-only** — 0.3.12 let a `sighthound` binary on `$PATH` *replace* `cpg.dangerous_calls`/`cpg.taint_flows` for the gate when present. v0.4.0 always gates on native CPG probes; embedded Sighthound only enriches `security_findings`. See [`docs/decisions/refactor-suite.md`](docs/decisions/refactor-suite.md) and the SECURE section of `README.md`. Force CPG-only detail with `TOPOS_DISABLE_SIGHTHOUND=1`.
- **`match`/`switch` counted per case arm** (#151/#153) — 0.3.12 counted a whole `match`/`switch` as one decision; v0.4.0 counts one branch per arm (stricter SIMPLE gate, aligns `ast.max_function_complexity` with `cfg.cyclomatic`).
- **COMPOSABLE scored by default** — CLI and MCP detect GitNexus, generate/refresh `.gitnexus` when missing or stale, then score COMPOSABLE in the same call. 0.3.12 required a separate depgraph step (CLI never attached an MDG; MCP read an existing graph only). See [`docs/decisions/composable-by-default.md`](docs/decisions/composable-by-default.md). Opt out: `--no-composable` / `no_composable: true`.
- **Sighthound embedded in-process** — no `$PATH` discovery; compiled into `topos-mcp` for Python/JS/TS/Go rulesets (Rust/C++ still use CPG probes). Deployment change; SECURE gate behavior is covered above.
- **MCP wire shape (breaking for structured-content consumers)** — `structured_content` defaults compact: `raw_metrics` behind `verbose: true`; null/empty fields omitted unconditionally; `topos_assess_*` always compact; `refactor_targets` defaults to `3` (was `0`); `worst_files` is `{filepath, lattice_element}` not full row clones; new `binding_constraint` field; gating targets rank ahead of advisory `cfg.cyclomatic` when no pillar preference is given. Restore partial payload with `verbose: true`; disable targets with `refactor_targets: 0`.

### Added

- **Server build identity, and self-reported stale-process detection.** Determining which of several registered Topos servers a host was actually running — and whether it held the binary you just built — previously meant `stat`, `pgrep`, `ps`, and `strings` on the executable. The server now answers it:
  - `serverInfo.version` carries semver build metadata (`0.4.0+build.<epoch>`), so two builds of the same branch are distinguishable. It rides on the existing field: no new surface and no agent-context cost, and it renders in the host's server list where a local build is told apart from a registry-installed one.
  - A new `topos://build` resource reports version and build time, executable path, resolved file root, pid, and staleness. It is a resource rather than a tool deliberately — resources are read on demand, so it costs nothing in the always-listed tool surface.
  - **Every tool response is prefixed with a stale-server warning when, and only when, the binary on disk has been rebuilt since the process started.** A rebuild does not replace a running server, since the MCP host owns the process; this makes that visible instead of leaving two timestamps to compare. Healthy responses are unchanged, so the compact payload is unaffected. It cannot be advertised in `instructions` instead: those are built at `initialize`, when a just-started process is never stale.
  - `topos-mcp --version` (and `--help`) now work. Previously the binary answered a version query by waiting for an `initialize` frame that never arrived and then reporting a closed connection, which reads like a broken install.

  The build timestamp is the executable's own mtime read at runtime, not a value embedded by a `build.rs`: a build script stamping a timestamp must rerun on every build to stay accurate, forcing a relink each time, and a cached value can report a build that never happened. No new dependency and no build script were added.
- **Sighthound SAST engine embedded directly**: the [Corgea/Sighthound](https://github.com/Corgea/Sighthound) pattern-matching + taint-flow scanner is now a compiled-in library dependency of `topos-mcp` (not an external CLI discovered on `$PATH`). SECURE findings for Python/JavaScript/TypeScript/Go come from Sighthound's embedded rulesets in-process; Rust/C++ fall back to the local CPG probes. Set `TOPOS_DISABLE_SIGHTHOUND=1` to force the CPG-probe path.
- **`topos mcp` subcommand**: the `topos` CLI binary now launches the in-process Rust MCP server, so the single `topos` binary is both the CLI and the MCP server (the VS Code extension invokes `topos mcp`). The standalone `topos-mcp` binary remains the PyPI-wheel entry point.
- **`topos depgraph generate` CLI subcommand restored.** Thin wrapper over the same `generate_depgraph` / `depgraph_status` paths the MCP `topos_generate_depgraph` tool uses (closes [#206](https://github.com/Krv-Labs/topos/issues/206)).
- **Graphify knowledge-graph integration (issue #150, Phase 1)**: a subprocess adapter, a from-scratch `graph.json` parser, and an orphan/fragile-edge detection probe, wired into `topos_refactor(target="graphify")`, a new `topos_generate_graphify_graph` MCP tool, and a new `topos graphify generate|orphans` CLI subcommand. Purely advisory — never feeds SIMPLE/COMPOSABLE/SECURE.
- **OpenClaw / Hermes agent skill:** Canonical skill at `skills/topos/SKILL.md`, ClawHub publish workflow, and `scripts/check_skill.py` version sync. ([#185](https://github.com/Krv-Labs/topos/pull/185))
- **`EvaluationResult.binding_constraint`.** The single gating metric costing a pillar its `achieved`: pillar, metric, value, threshold, and span. It is the top-ranked `"fix"` refactor target projected down to those fields, so it cannot disagree with `refactor_targets`, and it is omitted when only advisory metrics are out of band — which is what makes the common `achieved: true` / `score: 0.0` combination readable.

### Changed

- **MCP server rewritten in Rust (`topos-mcp` crate)**: the entire `topos/mcp/**` Python package is reimplemented as a Rust `rmcp` stdio server — every tool calls directly into `topos-core`. The `topos-mcp` PyPI package is now a thin maturin `bin` wheel with zero Python runtime dependencies.
- **All computation centralized in `topos-core`**: persistent-homology cycle basis, Forman-Ricci curvature engines, process graph, and MDG/process curvature probes moved out of `topos-pyo3` into `topos-core` (`functors::curvature`, `functors::probes::{cfg::homology,mdg::curvature,process::curvature}`, `graphs::process`). The `topos-pyo3` crate is removed.
- **Characteristic morphism χ_S moved to `core/`**: `characteristic_morphism.rs` now sits alongside `omega`, `morphism`, `object`, and `category` in `topos-core/src/core/`, not under `evaluation/`.
- **Tree-sitter is the sole AST engine**: no alternative parser backends are carried forward.
- **COMPOSABLE is scored by default, everywhere** — see **Intentional changes** above. Shared resolver: `topos_mcp::evaluation::ensure_gitnexus_dir`. MCP evaluate tools move to `read_only_hint: false` / `open_world_hint: true`; `topos_evaluate_file` is `async` with blocking offload. Flags: CLI `--no-composable` / `--gitnexus-dir <dir>`; MCP `no_composable` on evaluate tools.
- **Per-function complexity entries no longer skip `pub fn`s (wire-visible).** Rust migration bug: gate metric and `metric_locations`/`refactor_targets`/`binding_constraint`/`topos_inspect_code` entries disagreed when name resolution failed on `pub fn`s. Entries now cover those functions; gate metric unchanged but Rust files gain targets they never surfaced before.
- **`topos_inspect_code` now scores COMPOSABLE (closes #216).** Resolves the graph through the same helpers as `topos_evaluate_file`; takes `gitnexus_dir` and `no_composable`. The `code`-string form unchanged (no file → no module → SIMPLE/SECURE only).
- **`refactor_targets[0]` is the top gating target when no pillar preference is given.** Gating tier leads over advisory `cfg.cyclomatic` (#193); agent docs relabel `cfg.cyclomatic` as advisory. With `preferences.ranking` supplied, stated pillar order still wins.
- **`topos_generate_depgraph` caps GitNexus child output** at 200 bytes with an elision marker on `message`, `error`, and markdown. CLI still prints GitNexus output in full on stderr.
- **MCP evaluation responses default to a compact shape** — see **Intentional changes** above for the full wire contract and restore paths.

### Removed

- **The legacy Python implementation is deleted**: `topos/` (MCP, functors, graphs, core, evaluation, CLI, utils) and the Python `tests/` suite; PyInstaller onefile build (`scripts/build-binary.sh`, `scripts/lazy_exports.py`, `packaging/macos-entitlements.plist`); Sphinx `docs/source/api/` autodoc pages. CI and release are Rust-only (cargo test/clippy/fmt + stdio smoke test; binaries via `cargo build`, PyPI `bin` wheels via maturin). See `docs/source/architecture.rst`.

### Fixed

- **Deeply nested source no longer overflows the stack.** UAST clone/drop/equality, CFG construction, callable discovery, and PDG dataflow are iterative; CFG builder is an explicit task machine; CPG node collection is O(n). Golden edge-shape contracts for all six languages lock pre-rewrite CFG output (closes [#226](https://github.com/Krv-Labs/topos/issues/226)–[#229](https://github.com/Krv-Labs/topos/issues/229)).
- **`topos inspect --json` emits parseable JSON on the first run in a repository.** COMPOSABLE-by-default subprocess output no longer lands inside the JSON document; captured when `--json` is set.
- **The MCP agent docs and `topos_refactor_until_ideal` prompt taught a call shape the server rejects.** Flat top-level args (no FastMCP `params` wrapper); regression tests pin docs to the registered router.
- **`ast.max_function_complexity` restores 0.3.12 decision-form counting via UAST** (closes [#142](https://github.com/Krv-Labs/topos/issues/142)): forms the Rust port initially dropped — see **Parity with 0.3.12**.
- **`cfg.cyclomatic` / `ast.max_function_complexity` now count `match`/`switch` per case arm** (completes #151/#153) — **intentional break** from 0.3.12; see **Intentional changes** above. Also fixes discriminant-less Go `switch` over-count.
- **Multi-file rollup is now the true lattice meet** — restores consistency 0.3.12 should have had: `combine_dimensions` now takes ∧ of per-file Ω verdicts, not `min(score) ≥ score_floor`.
- **Project rollup no longer penalized by files that never evaluated a dimension** — missing pillar key (e.g. no MDG → no `composable`) leaves that pillar out of the meet.
- **Refactor suggestions can no longer fire on a gate the scorer passed** — both paths use shared `coupling_gate_input`.
- **Gates fail closed on `NaN`.**
- **`taint_flow_paths` is deterministic** — ties break by `(width, start_byte, id)`.
- **One-off `--allow` acknowledges risk instead of stripping it** — grade cap fires correctly on acknowledged SIMPLE+COMPOSABLE files.
- **`TOPOS_DEPGRAPH_TIMEOUT` / `TOPOS_GRAPHIFY_TIMEOUT` no longer panic** on non-finite or out-of-range values.
- **Version is 0.4.0** across `Cargo.toml`, `.mcp/server.json`, and the VS Code extension.

## [0.3.12] - 2026-07-20

### Added

- **Sighthound/Corgea SECURE Outsourcing:** Deprecates Python-based pattern matching and BFS taint-path tracing on the Code Property Graph (CPG) by outsourcing security analysis directly to Corgea/Sighthound when available on system `PATH`. When `sighthound` is present, Topos invokes it to parse JSON-formatted findings and maps them into standard `cpg.dangerous_calls` and `cpg.taint_flows`. If `sighthound` is absent, Topos gracefully falls back to local CPG danger and taint probes. (Closes [#130](https://github.com/Krv-Labs/topos/issues/130))
- **OpenWiki engineering wiki:** Regenerated repository documentation under `openwiki/` (architecture, workflows, domain concepts, operations, integrations) with CI refresh on merges to `main`. ([#169](https://github.com/Krv-Labs/topos/pull/169))

### Changed

- **VS Code extension package manager:** Migrated `extensions/vscode` from npm to pnpm (`packageManager: pnpm@11.8.0`), updated CI/release workflows to `pnpm/action-setup` with frozen lockfile, and publish with `vsce --no-dependencies`. GitNexus install hints now prefer `pnpm add -g` with npm fallback. ([#175](https://github.com/Krv-Labs/topos/pull/175), closes [#71](https://github.com/Krv-Labs/topos/issues/71))

### Fixed

- **Consistent allowlisting for Sighthound SECURE metrics:** Standardized `cpg.security_metrics` to filter Sighthound findings and local CPG probes through the same active engine, resolving discrepancies where Sighthound-only findings stayed active while `secure_adjusted` passed. Sighthound taint findings now correctly resolve to the actionable sink operation (`sink_type`) instead of the containing function name, ensuring consistent suffix-aware allowlist mapping. (Closes [#168](https://github.com/Krv-Labs/topos/issues/168), [#174](https://github.com/Krv-Labs/topos/pull/174))
- **VS Code extension failed binary downloads:** Deferred creation of the download destination stream until the HTTP response is confirmed as 200, preventing redirects and non-200 responses from leaving an empty file on disk. (Closes [#173](https://github.com/Krv-Labs/topos/issues/173), [#172](https://github.com/Krv-Labs/topos/pull/172))

## [0.3.11] - 2026-07-13

### Changed

- **COMPOSABLE no longer gates raw instability alone for languages with Abstractness support**: the `mdg.instability` gate (fixed `[0.3, 0.7]` band) flagged well-structured layered modules as failing — stable leaves (constants, error types, I≈0) and unstable orchestrators (`main.rs`/bootstrap wiring, I≈1) both got penalized even when architecturally intentional, because a raw-instability band ignores Robert Martin's own second axis. `Φ_COMPOSABLE` now pairs instability with a new `mdg.abstractness` metric (fraction of a module's type declarations that are abstract — trait/interface/protocol vs. concrete struct/class/enum) and gates on Distance from the Main Sequence (`mdg.main_sequence_distance = |A + I - 1|`, threshold `≤ 0.5`) instead, whenever abstractness is available. A concrete, unstable orchestrator now sits on the main sequence (D≈0) and is not penalized. Added a symmetric role-based exemption (`is_stable_leaf_module`: a declarations-only module with no branching control flow) for the "stable concrete leaf" case, which distance alone doesn't resolve — mirroring Martin's own accepted "Zone of Pain" exception. Scoped to Python, Rust, Go, TypeScript, and C++ files with countable type declarations; JavaScript (which has no abstract-type concept in the language) keeps the original instability-band gate unchanged. `main_sequence_distance_max` (0.5) and `stable_leaf_instability_max` (0.05) are first-pass provisional thresholds, not yet run through the PyPI corpus ECDF calibration the other COMPOSABLE constants received. Closes [#124](https://github.com/Krv-Labs/topos/issues/124).
- **`is_stable_leaf_module` no longer exempts modules with executable code**: the predicate only checked for absent branching control flow, so a declarations-only-*looking* module that still contained top-level calls or function/method definitions could wrongly claim the "Zone of Pain" distance exemption. `CallExpr`, `FunctionDecl`, and `MethodDecl` now also disqualify the leaf exemption.

### Fixed

- **C++ UAST mapper declaration node names now match the `tree-sitter-cpp` grammar**: `_DECLARATION_TYPES` was copy-pasted from the Python/Rust mappers and referenced node kinds that don't exist in C++'s grammar, so every C++ class/struct/enum/union mapped to `Unknown` with no `TypeDecl`/`typeKind` to hang Abstractness off of. C++ now has a working `extract_type_attributes` (pure-virtual-method to `abstractClass`) and is included in `_ABSTRACTNESS_SUPPORTED_LANGUAGES`, which is what unlocks C++ in the Distance-from-Main-Sequence gate above. Closes [#158](https://github.com/Krv-Labs/topos/issues/158).
- **COMPOSABLE scored 0% for isolated files once Abstractness was available**: `calculate_coupling`'s "no signal" fallback (`mdg.instability = 0.5` when a file has zero measured fan-in/fan-out) sat in the optimal band under the old raw-instability gate (quality 1.0), but the same fallback value combined with the common `mdg.abstractness = 0.0` case (no type declarations) put `mdg.main_sequence_distance` exactly at its calibrated ceiling -- passing the gate at the boundary but scoring 0% on the quality curve, and showing `FAIL` in the CLI table despite the file still counting toward an `IDEAL` badge. `Phi_COMPOSABLE` now only switches to distance mode when fan-in/fan-out indicate a real measured signal; files with no coupling data keep gating on raw instability, matching pre-#124 behavior.

## [0.3.10] - 2026-07-11

### Added

- **MCP refactor targets in `topos_evaluate_file`**: `refactor_targets: int = 0` (0 = off, N = cap) returns up to N ranked edit targets — concrete spans with the failing metric, current value vs. threshold, and `recommended_operations` tokens — without a new MCP tool. The agent contract routes targets natively (`next_tool = topos_assess_worktree_change` plus an `edit target …` action) and, when targets were not requested and the verdict is below IDEAL, advertises the option in `next_actions`.
- **Canonical gate specs** (`topos/evaluation/policies/gates.py`): one structured table (pillar, band, granularity, exemption predicates, operation tokens, interpretation prose) now drives the scorers' gate decisions, the suggestion engine, interpretation strings, and refactor targets. Verdict-preserving by construction (characterization grid in `tests/evaluation/test_gate_parity.py`); the entrypoint-module carve-outs are expressed once, so suggestions and targets no longer fire on gates the scorer passes.
- **Consolidated security guidance** (`topos/evaluation/security_guidance.py`): a single dangerous-API → (prose, operations) table, suffix-matched with the danger probe's own matcher, shared by suggestions and refactor targets. A registry-coverage test guarantees every `DANGEROUS_APIS` entry resolves to specific guidance.
- **Ensure-style `topos_generate_depgraph(force=False)`**: no-ops when the graph is current, regenerates when missing/stale/unloadable, and blocks on schema mismatch; `force=true` always regenerates. Results carry `generated` and `state_before`.
- **Unified refactoring suite (Methods Upgrade milestone)**: three new advisory `topos refactor` CLI subcommands and one new MCP tool, `topos_refactor(target="cycles"|"dependencies"|"process", ...)`, none of which affect SIMPLE/COMPOSABLE/SECURE scoring — distinct from this release's `RefactorTarget`/`refactor_targets` (gate-failure edit targets surfaced *inside* `topos_evaluate_file`); these are standalone tools applying new structural-analysis engines. The MCP surface is one tool rather than three specifically to stay under the tool-definition wire-size ratchet (`tests/mcp/test_context_budget.py`); see `openwiki/workflows/agent-and-cli.md` (Advisory refactoring) and `topos_get_doc(topic="workflows")` for the design orientation (three separate tools would each carry a self-contained `outputSchema`, tripling the embedded hotspot schema on the wire). `target=cycles` extracts a fundamental cycle basis on the CFG (new `src/ph.rs` functor) and maps each cycle generator to the source line range it covers, so cyclomatic complexity's count points at actual loops/branches instead of just a number. `target=dependencies` applies balanced Forman curvature (Topping et al., ICLR 2022) to the MDG to name concrete dependency edges worth strengthening. `target=process` applies directed Forman-Ricci curvature (Samal et al.) to GitNexus process graphs (new `topos/graphs/process/`) to find execution "choke points" where many independent call paths funnel through one transition. Both curvature variants share a new `src/frc.rs` Rust engine. (closes [#83](https://github.com/Krv-Labs/topos/issues/83), [#84](https://github.com/Krv-Labs/topos/issues/84), [#86](https://github.com/Krv-Labs/topos/issues/86))
- **Go language support**: Added parsing, mapping, and evaluation support for Go across all three quality dimensions (SIMPLE, SECURE, COMPOSABLE). Introduces `tree-sitter-go` parsing, `GoParser`, a dedicated Go UAST mapper (`mapper_go.py`), and central provider registry dispatching. Registers Go entries in the CPG dangerous-API (`exec.Command`, `syscall.Exec`, etc.) and taint-source (`os.Getenv`, `os.Args`, etc.) registries, and integrates cross-package boundary `IMPORTS` and `CALLS` edge mapping via GitNexus. ([#123](https://github.com/Krv-Labs/topos/pull/123), closes [#72](https://github.com/Krv-Labs/topos/issues/72), [#73](https://github.com/Krv-Labs/topos/issues/73), [#74](https://github.com/Krv-Labs/topos/issues/74))

### Changed

- **Depgraph freshness now sees the working tree** (fingerprint v2): generation records `{head_sha, generated_at}`, and staleness also triggers when any discovered source file was modified after generation — so the evaluate → edit-in-place → assess loop no longer scores COMPOSABLE against a pre-edit graph, and the ensure default regenerates instead of no-opping. v1 fingerprints keep the old SHA-only behavior; non-git dirs now get a sha-less marker so mtime freshness works there too.
- **`SCHEMA_MISMATCH` guidance no longer routes to plain regeneration**: the store was written by a newer GitNexus than the embedded ladybug reads, so regenerating cannot fix it. `topos_depgraph_status` now sets `next_tool = None` with upgrade-Topos / downgrade-GitNexus guidance, matching the generate tool's block message.
- **Suggestion/remediation matching**: longest-key suffix matching fixes `subprocess.Popen` resolving to `os.popen` advice; deserialization (`pickle.loads`, `yaml.load`, `marshal.loads`) and JS timer APIs gain specific operation tokens.
- `RefactorTarget.verify_with` removed — verification guidance lives once on `agent_contract.verification_gates`; per-target `constraints` slimmed to kind-specific lines.
- Agent-contract invariant documented and enforced: `next_tool`/`next_actions` never contradict `blocked_by`; when a target coexists with a setup blocker, `next_actions` carries both the edit step and the setup remedy (regression-tested in `tests/mcp/test_contract_invariant.py`).
- **`include_security_findings` is now a payload gate, never a routing gate**: the security overlay always carries the true active findings, and redaction happens only where results are shaped (`to_evaluation_result`, project file entries). Hiding findings no longer suppresses security refactor targets, secure suggestions, or the `active_security_findings` risk flag — assess and project contracts derive that flag from the allowlist-adjusted verdict (`secure_adjusted is False`) instead of the redactable payload list.

### Fixed

- **Depgraph mtime-drift calibration could trust a corrupted fingerprint**: `_newer_source_file`'s clock-skew calibration derived a threshold from a single `(finished_at, fingerprint_mtime)` sample with no sanity check; a negative or implausibly large `finished_at - generated_at` duration (backward/forward clock jump, or a corrupted fingerprint field) could extrapolate a bogus threshold that silently missed a real in-place edit. The duration is now clamped to `[0, 3600]` seconds, falling back to the flat skew tolerance when out of range, and debug-level logging now tags which freshness method (`content_hash` / `sha_anchor` / `mtime_calibrated` / `sha_only_no_signal` / `legacy_dir_mtime`) decided each verdict. ([#120](https://github.com/Krv-Labs/topos/pull/120))

- **CFG parser `if` branch locating**: Fixed a bug where `_if_branches` used a fixed position to locate the `then` block, causing it to break on Go's `if x := f(); cond {}` init-clause statement, and independently, on any Python, C++, or Rust `if` condition containing a same-line trailing comment. The `then` block is now correctly located by node kind. ([#123](https://github.com/Krv-Labs/topos/pull/123))
- **CFG parser loop body locating**: Fixed a bug where `_loop_body` unconditionally sliced off the first child as a loop condition/iterator, which silently dropped the entire body of Go's condition-less `for {}` loop. The loop body is now correctly located by node kind. ([#123](https://github.com/Krv-Labs/topos/pull/123))
- **CLI language detection for non-Python files**: Fixed a bug where `topos inspect` and `topos evaluate` CPG building and entropy calculations defaulted non-Python files to Python parsing due to a default parameter in `ProgramMorphism.from_file`. Correctly threads `detect_language(path)` through the affected CLI paths. ([#123](https://github.com/Krv-Labs/topos/pull/123))
- **Rust `#[cfg(test)]` modules leaked into the UAST**: the filter checked a node's own children for a `cfg(test)` attribute, but tree-sitter-rust places that attribute as a *preceding sibling* of the item it annotates — the check could match the wrong node entirely, including the file root itself, which then dropped the whole file (not just the test module) from the AST. Attribute-to-sibling correlation now scopes the filter to the correct item. ([#126](https://github.com/Krv-Labs/topos/pull/126))
- **Go entries missing from consolidated security guidance**: merging Go language support (#123) into this branch added `exec.Command`, `exec.CommandContext`, `os.StartProcess`, `syscall.Exec`, and `syscall.ForkExec` to the CPG dangerous-API registry, but the new canonical `security_guidance.py` table (above) predates that merge and had no matching entries — those callees fell through to generic default guidance instead of Go-specific advice. The registry-coverage test caught the gap as designed; added the five missing entries.
- **`evaluate --gitnexus-dir` crashed on a LadybugDB store with pending shadow pages**: Topos always opens `.gitnexus/lbug` read-only, but Ladybug refuses to replay pending shadow pages (left behind by an incremental `gitnexus analyze` without a full wipe) unless opened read-write, raising an unhandled `RuntimeError`. `_from_ladybugdb` now retries with a read-write handle when the read-only open fails specifically because of shadow-page replay. Also broadened `_handle_dep_graph_error`'s catch-all so any other unrecognized Ladybug `RuntimeError` (e.g. a corrupted WAL) degrades COMPOSABLE gracefully instead of crashing the CLI/MCP invocation — the previous check only tolerated "different version" / "storage version" messages. ([#136](https://github.com/Krv-Labs/topos/issues/136))

## [0.3.9] - 2026-07-06

### Changed

- **CLI startup latency**: `--version` and root `--help` exit before Click and heavy imports load; subcommands register lazily and `import topos` exposes only `__version__` eagerly. Standalone binary warm `--version` drops from ~854ms to ~586ms on macOS arm64. ([#109](https://github.com/Krv-Labs/topos/pull/109), closes [#108](https://github.com/Krv-Labs/topos/issues/108))
- **Single release binary**: retired the ECT semantic-coverage variant and slim-vs-ect packaging split; one `topos-{platform}` binary (~39 MB, down from ~72 MB). Semantic (ECT) coverage was removed from CLI, MCP, and policies. ([#109](https://github.com/Krv-Labs/topos/pull/109), [#116](https://github.com/Krv-Labs/topos/pull/116))
- **Release CI dogfoods binaries**: packaging smoke tests run against the built PyInstaller artifact so a broken frozen binary fails CI instead of shipping. (closes [#110](https://github.com/Krv-Labs/topos/issues/110), via [#109](https://github.com/Krv-Labs/topos/pull/109))

### Fixed

- **MCP invalid `gitnexus_dir` routing**: centralized COMPOSABLE setup contract routing for invalid, missing, and stale GitNexus states; `invalid_gitnexus_dir` now propagates across evaluate, assess, worktree, and changeset tool contracts instead of suggesting `topos_generate_depgraph` for a bad override path. ([#112](https://github.com/Krv-Labs/topos/pull/112), closes [#98](https://github.com/Krv-Labs/topos/issues/98))

## [0.3.8] - 2026-07-04

### Fixed

- **`cfg.longest_path` hung on functions with many sequential if/else branches**: `ControlFlowGraph::longest_acyclic_path` used backtracking-DFS path enumeration, which is O(2^k) for `k` sequential branches — real-world files (`typing_extensions`, `pycparser`'s `ply/yacc.py`) hung indefinitely. Replaced with a topological-sort + DP longest-path (O(V+E)). `CONTINUE` edges are now stripped alongside `LOOPBACK` before building the graph (a `continue`'s back-edge to its loop header also breaks the DAG invariant), and the implementation panics loudly if that invariant is ever violated instead of silently falling back to the algorithm that caused the hang. (closes [#113](https://github.com/Krv-Labs/topos/issues/113), [#114](https://github.com/Krv-Labs/topos/pull/114))

## [0.3.7] - 2026-07-02

### Fixed

- **Standalone binary crashed on every command** with `FileNotFoundError: .../\_MEIxxxx/Cargo.toml`. Version lookup fell back to reading `Cargo.toml`, which isn't bundled in the PyInstaller binary. `_version.py` now also searches `sys._MEIPASS` and never raises (falls back to `0.0.0+unknown`), and the release build bundles `Cargo.toml`. ([#105](https://github.com/Krv-Labs/topos/pull/105))
- **MCP `topos_depgraph_status`**: `risk_flags` now carries the state-specific code (`stale` / `load_error` / `schema_mismatch` / `invalid_dir`) alongside `composable_unavailable`, so clients branching on `risk_flags` alone can tell non-`PRESENT` states apart. ([#99](https://github.com/Krv-Labs/topos/pull/99))

## [0.3.6] - 2026-07-01

### Added

- **Glama release**: containerized MCP server build (`Dockerfile`, `.dockerignore`) and a `glama.json` maintainer manifest so the stdio server can be built, security-scanned, and published on Glama. MCP tool definitions were sharpened for the Tool Definition Quality Score (TDQS), and `topos_evaluate_project` now autodetects every supported language (Python, Rust, JavaScript, TypeScript, C++) in one walk with per-language rollups.
- **MCP `topos_assess_changeset`**: multi-file / module-split assessment with per-file before/after verdicts, a project rollup, and complexity-relocation / project-regression flags (read-only). (closes [#68](https://github.com/Krv-Labs/topos/issues/68))
- **MCP dependency-graph tools**: `topos_depgraph_status` (read-only `.gitnexus` state, including mtime-based staleness) and `topos_generate_depgraph` (approval-gated generation). The agent contract now blocks on missing/stale GitNexus stores and points `next_tool` at the depgraph tool; the CLI shares the same generation helper. (closes [#70](https://github.com/Krv-Labs/topos/issues/70))
- **Metric source locations**: failing `ast.max_function_complexity` / `cfg.cyclomatic` gates now map to concrete source spans, and `FunctionEntry` carries `qualified_name`, `kind`, line span, `metric_source`, and nesting info so `topos_inspect_code` and `topos_evaluate_file` report consistent locations. (closes [#67](https://github.com/Krv-Labs/topos/issues/67))
- Cross-language **entrypoint-module** handling: import/export-only modules (`__init__.py`, `mod.rs`/`lib.rs`, `index.ts`/`index.tsx`, `index.js`/`index.mjs`/`index.cjs`, C++ headers) are recognized via the new `topos/evaluation/file_roles.py` and receive relaxed SIMPLE (low-entropy) and COMPOSABLE (high-instability with zero fan-in) gates, so trivial re-export hubs are not penalized. `file_roles` is a general home for file-role predicates (generated/vendored/test files can follow). ([#87](https://github.com/Krv-Labs/topos/pull/87), closes [#77](https://github.com/Krv-Labs/topos/issues/77))
- **`topos update`** system command: channel-aware upgrades for binary installs (re-runs `install.sh` with checksum verification), PyPI installs (`uv pip` / `pip install -U topos-mcp`), and source checkouts (prints `git pull && uv pip install -e .`). Supports `--check` (exit 0 if current, 1 if outdated) and `--version` to pin a binary release. (closes [#78](https://github.com/Krv-Labs/topos/issues/78))
- Passive update notices on interactive CLI use (at most once per 24h; skipped for `topos mcp`, CI, non-TTY, and when `TOPOS_NO_UPDATE_NOTICES=1` is set).
- MCP edit-in-place assessment workflow for agents: snapshot and worktree-based assessment without pasting full source into tool calls. ([#76](https://github.com/Krv-Labs/topos/pull/76))
- Documentation quickstart guide, Sphinx autodoc API reference (`docs/source/api/`), and branded docs assets (Geist fonts, lattice/medal figures, Krv logos). ([#75](https://github.com/Krv-Labs/topos/pull/75))
- Preferences guide (`docs/source/preferences.rst`) and expanded agent workflow documentation.

### Changed

- Bumped the `fastmcp` floor from `>=3.0.0` to `>=3.4.2`. The 3.3.0 release has a circular import between `fastmcp.tools` and `fastmcp.server` that surfaces as a misleading `ImportError: FastMCP server support is not installed` whenever a tool module is imported before the server (e.g. during MCP test collection). The running MCP server was unaffected — it instantiates `FastMCP` (loading `fastmcp.server`) before any tool module — but the unpinned floor allowed the broken release into test/CI environments. 3.4.2 resolves the import order.
- **`install.sh`**: `TOPOS_UPDATE=1` fast path for in-place binary upgrades (skips banner, GitNexus prompt, and PATH setup while preserving download/checksum verification).
- MCP assess/evaluate tools refactored into `topos/mcp/tools/assess/` and `topos/mcp/tools/evaluate/` subpackages (`core`, `render`, `snapshot`, `worktree`, `project`) to improve structure and metric scores on the Topos codebase itself. ([#76](https://github.com/Krv-Labs/topos/pull/76))
- Updated MCP agent contract, workflow, and refactor prompt guidance for edit-in-place and preference-walk usage. ([#76](https://github.com/Krv-Labs/topos/pull/76))
- Documentation index, installation, agents, and README aligned with current CLI/MCP behavior; copy-paste code blocks cleaned up. ([#75](https://github.com/Krv-Labs/topos/pull/75))

### Fixed

- **Install detection priority** (closes [#82](https://github.com/Krv-Labs/topos/issues/82)): `detect_install_info()` now checks live Python metadata first; binary provenance is a fallback only when no Python package is found. Fixes `topos update` running the binary upgrade path for editable/pip installs that have a stale provenance record.
- `detect_install_method()` now resolves the **`topos-mcp`** PyPI distribution (was `topos`) and detects editable/source installs via `direct_url.json`.
- Duplicate binary path in install layout notice output (PATH-default binary was listed twice when it also appeared in `other_bins`).

### Added

- **Install layout notices**: detects conflicting `topos` executables on PATH and warns on stderr (throttled to once per 24h; always shown during `topos update` and `topos uninstall`; skipped in CI, non-TTY, and `TOPOS_NO_UPDATE_NOTICES=1`).

### Changed

- **`topos uninstall`**: shell rc cleanup (`--prune-path-hints`) now happens by default; pass `--keep-path-hints` to skip.
- **`topos uninstall`**: removes the full `~/.local/state/topos/` state directory (provenance file, update-check cache, install-layout cache) instead of only the provenance file. Removal is dry-run aware.

## [0.3.4] - 2026-06-12

### Fixed

- GitNexus ``.gitnexus`` stores from gitnexus 1.6.x (LadybugDB storage v41) no longer crash MDG loading; evaluation degrades gracefully when the store cannot be read. (closes [#59](https://github.com/Krv-Labs/topos/issues/59))

### Changed

- Replaced the frozen ``real-ladybug`` dependency with ``ladybug>=0.17.0,<0.18`` to match GitNexus 1.6.x (``@ladybugdb/core ^0.17.0``).

## [0.3.2] - 2026-06-04

### Fixed

- macOS onefile CLI: sign embedded dylibs (including `libpython3.12.dylib`) during PyInstaller collect with the same Developer ID as the outer binary, fixing `topos --version` failures after curl install (`PYI-82977` / different Team IDs). ([#54](https://github.com/Krv-Labs/topos/pull/54), closes [#55](https://github.com/Krv-Labs/topos/issues/55))

### Security

- Bumped `pyo3` from `0.22` to `0.24.1` to remediate [GHSA-pph8-gcv7-4qj5](https://github.com/advisories/GHSA-pph8-gcv7-4qj5) (`PyString::from_object` buffer overflow). Contributed via [#53](https://github.com/Krv-Labs/topos/pull/53).

## [0.3.0] - 2026-06-03

Consolidates the work previously published under the mis-tagged releases v1.0.0–v1.1.1.
Topos is still in initial development (0.x), so these are folded into a single 0.x
milestone; the v1.x tags were created in error and have been removed. Benchmark and
calibration tooling now lives in the separate
[topos-leaderboard](https://github.com/Krv-Labs/topos-leaderboard) repository.

Topos is not published to PyPI. Install the **Topos CLI** and start the **MCP server** from
release binaries (see `install.sh` in the README and [installation docs](docs/source/installation.rst)).

### Added

- 3-pillar code quality evaluation model (Simple, Composable, Secure).
- Heyting algebra support for partial-confidence code evaluation on the 8-element lattice (SLOP → IDEAL).
- Evaluation types: `CharacteristicMorphism`, `ClassificationResult`, and preference-driven `UserPreferences` with induced relaxation walk on Ω.
- Representation models: `ControlFlowGraph`, `CodePropertyGraph`, `ModuleDependencyGraph`.
- Structural test coverage: CLI `topos structural-test-coverage` and MCP `topos_calculate_coverage` (declaration-level bipartite UAST matching).
- MCP `topos_preference_walk` and preferences-aware evaluate/assess tools.
- `verbose` option on `topos_evaluate_project` to include raw probe metric floats in the response.
- **Rust Backend (`topos-functors`)**: performance-critical graph construction and metric probes on a Rust core via PyO3 and Maturin.
- **Parity Tests** (`tests/parity/`) monitoring equivalence between the Rust core and the Python baseline.
- **CLI Reference docs** (`docs/source/cli.rst`) for evaluate, inspect, compare, structural test coverage, dependency graphs, and MCP.
- **CLI Progress Bar** for `topos eval`, **MCP Diagnostics** in tool responses, and **Language Detection** in `classify_file` from file suffixes.

### Changed

- **Hybrid Architecture**: hybrid Rust/Python package — performance-heavy logic (CFG, AST entropy, edit distance) runs at native speed (~6–8x speedup) behind readable Python wrappers.
- **Directory Restructuring**: moved Python source from `src/topos` to the repository root as `topos/`; repurposed `src/` for the Rust backend.
- **Build System**: switched from `hatchling` to `maturin` to support native extension compilation.
- Consolidated evaluation logic onto the structural code quality metrics; policies use independent binary thresholds per pillar.
- Project rollup (`combine_dimensions`) uses calibrated per-generator score floors.
- Migrated UAST structural test coverage implementation under `topos.functors.profunctors.uast`; aligned to declaration-level bipartite matching only.
- Refactored CLI into `topos.cli` submodules; entry point is `topos` (including `topos mcp`).
- **Categorical Documentation**: `topos.graphs` now explicitly defines graph construction as a **Functor** $R: \text{Lang} \to \mathcal{E}$.
- Updated documentation and README to reflect the 3-pillar approach, medal-podium lattice framing, and calibrated score floors.

### Removed

- Earlier experimental 0.x APIs and CLI commands that are no longer compatible.
- Legacy structural test coverage paths (pooled histogram and earlier recall variants); only declaration-level bipartite coverage remains (`declaration_coverage` / `DeclarationCoverageReport`).
- Obsolete pooled-coverage probe module after the profunctor migration.
