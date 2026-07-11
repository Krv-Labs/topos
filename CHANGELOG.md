# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **MCP refactor targets in `topos_evaluate_file`**: `refactor_targets: int = 0` (0 = off, N = cap) returns up to N ranked edit targets — concrete spans with the failing metric, current value vs. threshold, and `recommended_operations` tokens — without a new MCP tool. The agent contract routes targets natively (`next_tool = topos_assess_worktree_change` plus an `edit target …` action) and, when targets were not requested and the verdict is below IDEAL, advertises the option in `next_actions`.
- **Canonical gate specs** (`topos/evaluation/policies/gates.py`): one structured table (pillar, band, granularity, exemption predicates, operation tokens, interpretation prose) now drives the scorers' gate decisions, the suggestion engine, interpretation strings, and refactor targets. Verdict-preserving by construction (characterization grid in `tests/evaluation/test_gate_parity.py`); the entrypoint-module carve-outs are expressed once, so suggestions and targets no longer fire on gates the scorer passes.
- **Consolidated security guidance** (`topos/evaluation/security_guidance.py`): a single dangerous-API → (prose, operations) table, suffix-matched with the danger probe's own matcher, shared by suggestions and refactor targets. A registry-coverage test guarantees every `DANGEROUS_APIS` entry resolves to specific guidance.
- **Ensure-style `topos_generate_depgraph(force=False)`**: no-ops when the graph is current, regenerates when missing/stale/unloadable, and blocks on schema mismatch; `force=true` always regenerates. Results carry `generated` and `state_before`.
- **Go language support**: Added parsing, mapping, and evaluation support for Go across all three quality dimensions (SIMPLE, SECURE, COMPOSABLE). Introduces `tree-sitter-go` parsing, `GoParser`, a dedicated Go UAST mapper (`mapper_go.py`), and central provider registry dispatching. Registers Go entries in the CPG dangerous-API (`exec.Command`, `syscall.Exec`, etc.) and taint-source (`os.Getenv`, `os.Args`, etc.) registries, and integrates cross-package boundary `IMPORTS` and `CALLS` edge mapping via GitNexus. ([#123](https://github.com/Krv-Labs/topos/pull/123), closes [#72](https://github.com/Krv-Labs/topos/issues/72), [#73](https://github.com/Krv-Labs/topos/issues/73), [#74](https://github.com/Krv-Labs/topos/issues/74))

### Changed

- **Depgraph freshness now sees the working tree** (fingerprint v2): generation records `{head_sha, generated_at}`, and staleness also triggers when any discovered source file was modified after generation — so the evaluate → edit-in-place → assess loop no longer scores COMPOSABLE against a pre-edit graph, and the ensure default regenerates instead of no-opping. v1 fingerprints keep the old SHA-only behavior; non-git dirs now get a sha-less marker so mtime freshness works there too.
- **`SCHEMA_MISMATCH` guidance no longer routes to plain regeneration**: the store was written by a newer GitNexus than the embedded ladybug reads, so regenerating cannot fix it. `topos_depgraph_status` now sets `next_tool = None` with upgrade-Topos / downgrade-GitNexus guidance, matching the generate tool's block message.
- **Suggestion/remediation matching**: longest-key suffix matching fixes `subprocess.Popen` resolving to `os.popen` advice; deserialization (`pickle.loads`, `yaml.load`, `marshal.loads`) and JS timer APIs gain specific operation tokens.
- `RefactorTarget.verify_with` removed — verification guidance lives once on `agent_contract.verification_gates`; per-target `constraints` slimmed to kind-specific lines.
- Agent-contract invariant documented and enforced: `next_tool`/`next_actions` never contradict `blocked_by`; when a target coexists with a setup blocker, `next_actions` carries both the edit step and the setup remedy (regression-tested in `tests/mcp/test_contract_invariant.py`).
- **`include_security_findings` is now a payload gate, never a routing gate**: the security overlay always carries the true active findings, and redaction happens only where results are shaped (`to_evaluation_result`, project file entries). Hiding findings no longer suppresses security refactor targets, secure suggestions, or the `active_security_findings` risk flag — assess and project contracts derive that flag from the allowlist-adjusted verdict (`secure_adjusted is False`) instead of the redactable payload list.

### Fixed

- **CFG parser `if` branch locating**: Fixed a bug where `_if_branches` used a fixed position to locate the `then` block, causing it to break on Go's `if x := f(); cond {}` init-clause statement, and independently, on any Python, C++, or Rust `if` condition containing a same-line trailing comment. The `then` block is now correctly located by node kind. ([#123](https://github.com/Krv-Labs/topos/pull/123))
- **CFG parser loop body locating**: Fixed a bug where `_loop_body` unconditionally sliced off the first child as a loop condition/iterator, which silently dropped the entire body of Go's condition-less `for {}` loop. The loop body is now correctly located by node kind. ([#123](https://github.com/Krv-Labs/topos/pull/123))
- **CLI language detection for non-Python files**: Fixed a bug where `topos inspect` and `topos evaluate` CPG building and entropy calculations defaulted non-Python files to Python parsing due to a default parameter in `ProgramMorphism.from_file`. Correctly threads `detect_language(path)` through the affected CLI paths. ([#123](https://github.com/Krv-Labs/topos/pull/123))
- **Rust `#[cfg(test)]` modules leaked into the UAST**: the filter checked a node's own children for a `cfg(test)` attribute, but tree-sitter-rust places that attribute as a *preceding sibling* of the item it annotates — the check could match the wrong node entirely, including the file root itself, which then dropped the whole file (not just the test module) from the AST. Attribute-to-sibling correlation now scopes the filter to the correct item. ([#126](https://github.com/Krv-Labs/topos/pull/126))
- **Go entries missing from consolidated security guidance**: merging Go language support (#123) into this branch added `exec.Command`, `exec.CommandContext`, `os.StartProcess`, `syscall.Exec`, and `syscall.ForkExec` to the CPG dangerous-API registry, but the new canonical `security_guidance.py` table (above) predates that merge and had no matching entries — those callees fell through to generic default guidance instead of Go-specific advice. The registry-coverage test caught the gap as designed; added the five missing entries.

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
