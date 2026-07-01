# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.6] - 2026-07-01

### Added

- **Glama release**: containerized MCP server build (`Dockerfile`, `.dockerignore`) and a `glama.json` maintainer manifest so the stdio server can be built, security-scanned, and published on Glama. MCP tool definitions were sharpened for the Tool Definition Quality Score (TDQS), and `topos_evaluate_project` now autodetects every supported language (Python, Rust, JavaScript, TypeScript, C++) in one walk with per-language rollups.
- **MCP `topos_assess_changeset`**: multi-file / module-split assessment with per-file before/after verdicts, a project rollup, and complexity-relocation / project-regression flags (read-only unless `refresh_depgraph` is set). (closes [#68](https://github.com/Krv-Labs/topos/issues/68))
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
