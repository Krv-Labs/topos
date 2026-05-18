# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.0] - 2026-05-18

### Added

- **Rust Backend (`topos-functors`)**: Migrated performance-critical graph construction and metric probes to a high-performance Rust core using PyO3 and Maturin.
- **Benchmarking Suite**: Added `benchmarks/` directory with side-by-side comparison scripts between Python and Rust implementations.
- **Parity Tests**: Added `tests/parity/` to continuously monitor implementation equivalence between the Rust core and the v1.0.0 Python baseline.
- **CLI Reference docs**: Added `docs/source/cli.rst` with command-by-command reference for evaluate, inspect, compare, structural test coverage, dependency graphs, and MCP; linked from the docs index.

### Changed

- **Hybrid Architecture**: Reorganized project structure into a hybrid Rust/Python package. Performance-heavy logic (CFG, AST entropy, edit distance) now runs at native speed while maintaining readable Python wrappers.
- **Directory Restructuring**: Moved Python source from `src/topos` to the repository root as `topos/` and repurposed `src/` for the Rust backend.
- **Build System**: Switched from `hatchling` to `maturin` to support native extension compilation.
- **Performance**: Achieved significant speedups (approx. 6-8x) for evaluation tasks on standard codebases.
- **Calibration Experiments**: Moved calibration suite and threshold findings infrastructure from `evaluations/calibration/` into the `benchmarks/` folder as `benchmarks/calibration/`.
- **Categorical Documentation**: Updated `topos.graphs` to explicitly define graph construction as a **Functor** $R: \text{Lang} \to \mathcal{E}$.

## [1.0.0] - 2026-05-16

v1.0.0 changes live on the `refactor/3-pillars` branch and will land on `main` via [PR #39](https://github.com/Krv-Labs/topos/pull/39).

Topos is not published to PyPI. Install the **Topos CLI** and start the **MCP server** from release binaries (see `install.sh` in the README and [installation docs](docs/source/installation.rst)).

### Added

- Introduced the 3-pillar code quality evaluation model (Simple, Composable, Secure).
- Added Heyting algebra support for partial-confidence code evaluation on the 8-element lattice (SLOP → IDEAL).
- Added evaluation types: `CharacteristicMorphism`, `ClassificationResult`, and preference-driven `UserPreferences` with induced relaxation walk on Ω.
- Added representation models: `ControlFlowGraph`, `CodePropertyGraph`, `ModuleDependencyGraph`.
- Added structural test coverage: CLI `topos structural-test-coverage` and MCP `topos_calculate_coverage` (declaration-level bipartite UAST matching).
- Added MCP `topos_preference_walk` and preferences-aware evaluate/assess tools.
- Added calibration suite under `evaluations/calibration/` with v1.0.0 threshold findings documented in `docs/calibration.md`.
- Added `verbose` option on `topos_evaluate_project` to include raw probe metric floats in the response.

### Changed

- Major architectural overhaul transitioning from experimental 0.x releases.
- Consolidated evaluation logic to operate on the new structural code quality metrics; policies use independent binary thresholds per pillar.
- Project rollup (`combine_dimensions`) uses calibrated per-generator score floors (defaults aligned with calibration findings).
- Migrated UAST structural test coverage implementation under `topos.functors.profunctors.uast`.
- Structural test coverage docs and CLI output aligned with declaration-level bipartite matching only.
- Refactored CLI into `topos.cli` submodules; entry point is `topos` (including `topos mcp`).
- Updated documentation and README to reflect the 3-pillar approach, medal-podium lattice framing, and calibrated score floors.

### Removed

- **Breaking:** Previous experimental APIs and CLI commands from 0.x are no longer compatible and have been removed.
- Legacy structural test coverage v0/v1 paths (pooled histogram and earlier recall variants). Only declaration-level bipartite coverage remains (`declaration_coverage` / `DeclarationCoverageReport`).
- Obsolete pooled-coverage probe module after profunctor migration.
