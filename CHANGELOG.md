# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-07-20

### Changed

- **MCP server rewritten in Rust (`topos-mcp` crate)**: the entire `topos/mcp/**` Python package is reimplemented as a Rust `rmcp` stdio server, so no computation is marshalled through Python anymore — every tool (all 17: the `topos_evaluate_*`, `topos_assess_*`, `topos_compare_*`, `topos_inspect_code`, `topos_calculate_coverage`, `topos_preference_walk`, `topos_depgraph_*`, `topos_refactor`, `topos_get_doc` family), all 6 `topos://docs/*` resources, and the `topos_refactor_until_ideal` prompt call directly into `topos-core`. The `topos-mcp` PyPI package is now a thin maturin `bin` wheel that ships this self-contained server binary; `pip install topos-mcp` puts the `topos-mcp` command on `PATH` with zero Python runtime dependencies.
- **All computation centralized in `topos-core`**: the persistent-homology cycle basis, Forman-Ricci curvature engines, process graph, and their MDG/process curvature probes moved out of the former `topos-pyo3` extension crate into `topos-core` (`functors::curvature`, `functors::probes::{cfg::homology,mdg::curvature,process::curvature}`, `graphs::process`). The `topos-pyo3` crate is removed — its functionality lives in `topos-core` as functors, per the PR #159 review.
- **Characteristic morphism χ_S moved to `core/`**: `characteristic_morphism.rs` now sits alongside the other category-theory definitions (`omega`, `morphism`, `object`, `category`) in `topos-core/src/core/`, not under `evaluation/`.
- **Tree-sitter is the sole AST engine**: the AST dispatch layer commits fully to tree-sitter; no alternative parser backends are carried forward.
- **SECURE scoring stays CPG-native; Sighthound is advisory-only** — this supersedes the Python `topos/utils/sighthound.py` integration merged separately to `main` (issues #130/#134), which let Sighthound's counts *replace* the CPG probes for the SECURE gate itself when the CLI was on `PATH`. In this Rust port, `cpg.dangerous_calls`/`cpg.taint_flows` (the SECURE gate's inputs) always come from the native CPG probes; the embedded Sighthound engine only supplies supplementary, per-finding `security_findings` detail. This is an intentional behavior change, not an oversight — see `docs/refactor-suite.md` and the SECURE section of `README.md`.

### Added

- **Sighthound SAST engine embedded directly**: the [Corgea/Sighthound](https://github.com/Corgea/Sighthound) pattern-matching + taint-flow scanner is now a compiled-in library dependency of `topos-mcp` (not an external CLI discovered on `$PATH`). SECURE findings for Python/JavaScript/TypeScript/Go come from Sighthound's embedded rulesets in-process; Rust/C++ fall back to the local CPG probes. Set `TOPOS_DISABLE_SIGHTHOUND=1` to force the CPG-probe path. Finding→callee/sink mapping and allowlist matching incorporate the same consistency fix `main` shipped in Python (#168/#174): taint findings resolve their actionable sink via `sink_info.sink_type` before falling back to the containing function name, and allowlist matching is resolved per-finding rather than through a pre-filtered registry substitution.
- **`topos mcp` subcommand**: the `topos` CLI binary now launches the in-process Rust MCP server, so the single `topos` binary is both the CLI and the MCP server (the VS Code extension invokes `topos mcp`). The standalone `topos-mcp` binary remains the PyPI-wheel entry point.
- **Graphify knowledge-graph integration (issue #150, Phase 1)**: a subprocess adapter, a from-scratch `graph.json` parser, and an orphan/fragile-edge detection probe, wired into `topos_refactor(target="graphify")`, a new `topos_generate_graphify_graph` MCP tool, and a new `topos graphify generate|orphans` CLI subcommand (the first CLI entry point for the refactor-suite family). Purely advisory — never feeds SIMPLE/COMPOSABLE/SECURE.

### Removed

- **The legacy Python implementation is deleted**: `topos/` (the Python package — MCP, functors, graphs, core, evaluation, CLI, utils) and the entire Python `tests/` suite are gone now that computation lives in `topos-core` and the server is `topos-mcp`. Also removed: the Rust-vs-Python parity/benchmark scripts, the PyInstaller onefile build (`scripts/build-binary.sh`, `scripts/lazy_exports.py`, `packaging/macos-entitlements.plist`), and the Sphinx `docs/source/api/` autodoc pages (which introspected the removed Python API — there's no Rust equivalent; see the new `docs/source/architecture.rst`). The rest of the Sphinx site (`docs.krv.ai`) stays and is rewritten for the new Rust CLI/MCP surface and crate structure — see `pyproject.toml`'s `docs` dependency group and `.github/workflows/docs.yml`. CI and the release workflow are rewritten Rust-only (cargo test/clippy/fmt + a stdio smoke test; binaries via `cargo build`, PyPI `bin` wheels via maturin).

### Fixed

- **`cfg.cyclomatic` / `ast.max_function_complexity` now count `match`/`switch` in every language** (completes #151/#153): Python `match`, JavaScript/TypeScript `switch`, and C++ `switch` were never mapped to the UAST `MatchStmt` kind, so their case dispatch added nothing to complexity and a match-heavy function could pass the SIMPLE gate it should fail. Every language mapper now emits `MatchStmt`, and the CFG builder's arm extractor was unified so each case is exactly one branch — also fixing a discriminant-less Go `switch` over-count where each case *statement* became its own branch. Both metrics now count one branch per case arm, so `ast.max_function_complexity` agrees with `cfg.cyclomatic`; this intentionally diverges from `topos-mcp==0.3.11` (which counted a whole match/switch as a single decision) and is allowlisted in `scripts/parity_check.py`.
- **Multi-file rollup is now the true lattice meet.** `combine_dimensions` derived the codebase verdict from `min(score) ≥ score_floor`, a criterion that diverged from the per-file `achieved` gates and could make a one-file "codebase" contradict that file's own verdict. It now takes the meet (`∧`) of the per-file Ω verdicts, so single- and multi-file classification agree by construction; continuous scores stay advisory.
- **Refactor suggestions can no longer fire on a gate the scorer passed.** `suggest_refactors` re-evaluated gates against raw `mdg.instability` with hard-coded exemption flags, disagreeing with `Φ_COMPOSABLE` (which gates `mdg.main_sequence_distance` in distance mode). Both paths now build gate inputs through the shared `coupling_gate_input`, and the real stable-leaf / instability context is threaded through.
- **Gates fail closed on `NaN`.** A `NaN` metric compared false against both bounds and silently `Pass`ed every gate, including the zero-tolerance SECURE gates; it is now an out-of-band failure.
- **`taint_flow_paths` is deterministic.** Enclosing-statement resolution broke equal-width ties by `HashMap` iteration order, so the taint count could vary across runs; ties now break by `(width, start_byte, id)`.
- **Version is 0.4.0** across `Cargo.toml`, `.mcp/server.json`, and the VS Code extension, so the release publishes as `topos-mcp 0.4.0` instead of colliding with the already-published `0.3.11` on PyPI.
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
