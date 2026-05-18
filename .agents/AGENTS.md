# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Writing Style

Always use **American English spelling** — "optimize" not "optimise", "analyze" not "analyse", "modeling" not "modelling", etc.

## Overview

**Topos** is a code quality evaluation framework that applies category theory (specifically Heyting Algebra) to classify Python code. Programs are evaluated across three independent *generators* (SIMPLE, COMPOSABLE, SECURE) and mapped to an 8-element lattice (free Heyting algebra on 3 generators) rather than a single numeric score.

The project consists of:
- A CLI tool (`topos`) for evaluating and comparing code
- An MCP server (`topos-mcp`) exposing code analysis as tools for Claude and other MCP clients
- Category-theoretic abstractions in `topos.core`: `ProgramObject`, `ProgramMorphism`, `ProgramCategory`, `Omega`
- The decision layer in `topos.evaluation`: `CharacteristicMorphism` (χ_S : P → Ω) plus the per-generator policy translators
- Graph representations in `topos.graphs`: `ASTRepresentation`, `ControlFlowGraph`, `ModuleDependencyGraph`, `ProgramDependenceGraph`, `CodePropertyGraph`, and the UAST substrate

## Development Commands

```bash
uv pip install -e ".[dev]"              # Install with dev dependencies
uv run maturin develop                  # Build Rust backend in development mode

pytest                                   # Run all tests
pytest tests/parity/                    # Run implementation parity tests
pytest tests/test_file.py::test_name    # Run a specific test

ruff check topos/ --fix && ruff format topos/   # Lint and format
```

## CLI Usage

```bash
topos evaluate path/to/code.py
topos evaluate topos/ -r --priority simple
topos evaluate topos/ -r --gitnexus-dir .gitnexus --priority composable
topos evaluate topos/ -r --gitnexus-dir .gitnexus --priority secure
...
```

## Architecture

### Hybrid Rust/Python Model
Topos uses a high-performance hybrid architecture. Performance-critical logic is implemented in Rust (`topos-functors`), while orchestration and evaluation policies remain in Python for readability.

- **`topos/core/`** — the program topos's defining structure.
- **`topos/graphs/`** — translational functors `R : Lang → E`. Documented Python wrappers that delegate graph construction to the Rust backend.
- **`topos/evaluation/`** — the decision layer: how raw measurements become Ω verdicts.
- **`topos/functors/`** — probes and profunctors. Documented wrappers delegating heavy metrics (CFG, entropy, edit distance) to Rust.
- **`src/`** — houses the Rust `topos-functors` core.

The `Representation` protocol (`graphs/base.py`) requires:
- `name: str` — identifies the representation type (e.g. `"ast"`, `"cfg"`, `"mdg"`, `"cpg"`).
- `dimension: str` — the generator this representation feeds (`"simple"`, `"composable"`, or `"secure"`).
- `metrics() -> dict[str, float]` — namespaced metric values.

New representations go in `topos.graphs/`; new probes in `topos.functors.probes/`; new pairwise comparisons in `topos.functors.profunctors/`.

### Evaluation Flow

```
ProgramMorphism.from_file(path)
  → ASTRepresentation(morphism.ast)   [always built by the classifier; entropy → SIMPLE]
  → morphism.build_cfg()              [always built; feeds SIMPLE]
  → morphism.build_pdg()              [always built; diagnostic]
  → morphism.build_cpg()              [always built; feeds SECURE]
  → ModuleDependencyGraph.from_gitnexus_dir(...)  [optional; requires .gitnexus/; feeds COMPOSABLE]

CharacteristicMorphism.classify_detailed(morphism, representations=[cfg, pdg, cpg, mdg], priority=...)
  → Group representations by their `dimension` (= generator)
  → Call rep.metrics() → raw floats
  → score_simple(cfg.cyclomatic, ast.entropy, ...) → ScoredDecision  [SIMPLE]
  → score_coupling(mdg.coupling, mdg.instability, ...) → ScoredDecision  [COMPOSABLE]
  → score_secure(cpg.dangerous_calls, cpg.taint_flows, ...) → ScoredDecision  [SECURE]
  → score ≥ 0.6 → generator satisfied
  → verdict = verdict_from_generators(simple, composable, secure) → Ω element
  → Return ClassificationResult
```

### The 8-Element Lattice (Ω)

`Omega` (in `topos.core.omega`) is the free Heyting algebra on the three generators `{SIMPLE, COMPOSABLE, SECURE}` — one element per subset of generators a program satisfies:

```
                          IDEAL  (⊤ — all three generators satisfied)
                         /  |  \
        SIMPLE_COMPOSABLE  SIMPLE_SECURE  COMPOSABLE_SECURE
              |  \  /             \  /  |
            SIMPLE   COMPOSABLE        SECURE
                       \    |    /
                          SLOP  (⊥ — no generator satisfied)
```

Key property: **The three generators are pairwise incomparable.** Each can be achieved independently; the algebraic meet of two incomparable atoms is `SLOP`. Multi-file rollup is the pointwise lattice meet `⋀_f χ_S(f)`: a generator is satisfied across the codebase iff it is satisfied on every file (minimum per-generator score ≥ threshold).

### The Three Pillars

| Generator | Representation | Metrics | Scoring |
|-----------|---------------|---------|---------|
| SIMPLE | `ControlFlowGraph` (+ `ASTRepresentation` entropy) | `cfg.cyclomatic`, `cfg.essential`, `cfg.nesting_depth`, `cfg.longest_path`, `ast.entropy` | Weighted: `1 - cyclomatic/40` + entropy bell-curve (peak at 0.5). Threshold: **0.60**. |
| COMPOSABLE | `ModuleDependencyGraph` | `mdg.coupling`, `mdg.instability`, `mdg.fan_in`, `mdg.fan_out`, `mdg.dep_depth` | Weighted: `1 - coupling/35` + instability flat-top tent over [0.3, 0.7]. Threshold: **0.60**. |
| SECURE | `CodePropertyGraph` | `cpg.dangerous_calls`, `cpg.taint_flows` | Weighted exp-decay: `exp(-count / scale)` for each metric. Threshold: **0.70** (higher — security false-negatives are asymmetrically costly). |

**Diagnostic (not counted toward verdict):**
- `ProgramDependenceGraph`: `pdg.data_deps`, `pdg.control_deps`, `pdg.density` — intra-procedural dependence analysis.

**`--priority`** shifts metric weights *within* each generator (via `Priority` enum: `SIMPLE`, `COMPOSABLE`, `SECURE`). It does not change the lattice structure or which generators score. `Priority` is the single-knob shorthand; for a full agent loop use `UserPreferences` (see [Priority & Preferences](#priority--preferences)).

### Key Non-Obvious Behaviors

- **SIMPLE and SECURE always run.** CFG and CPG are derived from the UAST built during parsing — no external tooling required.  Parse failures collapse the whole verdict to `SLOP`.
- **COMPOSABLE is unreachable without `.gitnexus/`.**  Module dependency evaluation only runs when a `ModuleDependencyGraph` is loaded from a `.gitnexus/` directory (`--gitnexus-dir`).  Any verdict containing COMPOSABLE (including `IDEAL`) is then unreachable.
- **GitNexus is an external npm tool.**  Run `topos depgraph generate` (which calls `gitnexus analyze`) to produce the `.gitnexus/` directory consumed by `--gitnexus-dir`.
- **Parse failures kill the verdict.** `is_parseable=False` → `lattice_element = SLOP`; in `combine_dimensions()` they inject a `0.0` score on SIMPLE, pulling multi-file aggregation down.
- **Mixed representations within a generator** are scored independently and combined via `min()` (conservative).  E.g. AST entropy and CFG cyclomatic both feed SIMPLE; the generator passes iff both individual decisions pass.
- **Three generators are orthogonal.** A file can be SIMPLE without being COMPOSABLE, COMPOSABLE without being SECURE, etc.  The 8-element Ω encodes every combination.

## Priority & Preferences

There are **two complementary knobs** for controlling how the scoring pipeline weights quality axes. Every evaluation call must supply at least one.

### `Priority` — single-knob (top-generator emphasis)

`Priority` is a `StrEnum` with three members: `SIMPLE`, `COMPOSABLE`, `SECURE`. It selects a `WeightProfile` that upweights the primary metric for that generator inside the matching `Φᵢ`:

| Priority | `w_complexity` (Φ_SIMPLE) | `w_coupling` (Φ_COMPOSABLE) | `w_taint` (Φ_SECURE) |
|---|---|---|---|
| `simple` | 0.7 | 0.3 | 0.3 |
| `composable` | 0.3 | 0.7 | 0.3 |
| `secure` (default) | 0.3 | 0.3 | 0.7 |

`Priority` is the CLI shorthand — use it when you only need to name the *top-ranked* generator. It does **not** produce a relaxation walk on Ω.

**There is no `balanced` mode.** Every evaluation pins a priority; the codebase default is `Priority.SECURE` (most conservative).

### `UserPreferences` — full strict ordering (agent loop)

`UserPreferences` captures a **strict total order** over all three generators, e.g. `[COMPOSABLE, SECURE, SIMPLE]`. This induces a total order on all 8 Ω elements, enables two-stage targeting, and drives the relaxation walk.

```python
from topos.evaluation.preferences import UserPreferences, Generator

prefs = UserPreferences(ranking=(Generator.COMPOSABLE, Generator.SECURE, Generator.SIMPLE))
```

#### How the induced order works

Each verdict is scored by its satisfied-generator bitmask weighted 4 / 2 / 1 in preference order:

```
score(v) = 4·⟦g₁ satisfied⟧ + 2·⟦g₂ satisfied⟧ + 1·⟦g₃ satisfied⟧
```

For ranking `[COMPOSABLE, SECURE, SIMPLE]` this yields:
`IDEAL (7) > COMPOSABLE_SECURE (6) > COMPOSABLE_SIMPLE (5) > COMPOSABLE (4) > SECURE_SIMPLE (3) > SECURE (2) > SIMPLE (1) > SLOP (0)`.

#### Two-stage targeting

| Stage | Target | Trigger to advance |
|---|---|---|
| 1 | `IDEAL` (aspirational) | Attempt for all iterations first |
| 2 | `fallback_target` — meet of the top-two generators | When IDEAL plateaus (no lattice movement) |

For ranking `[COMPOSABLE, SECURE, SIMPLE]` the fallback is `COMPOSABLE_SECURE`.

#### Relaxation walk & next_step

`prefs.relaxation_walk(current)` returns the descending verdict sequence from the aspirational target down to (but not including) `current`. `prefs.next_step(current)` is the **smallest** improvement above the current verdict — the safest immediate goal for the agent.

#### `WeightProfile` from a full ranking

When `UserPreferences` is supplied to the classifier, `WeightProfile.from_ranking(ranking)` is called to derive intra-policy weights. The top-ranked generator's `Φᵢ` is the most decisive (0.7), the middle is balanced (0.5), the bottom is conservative (0.3):

```
ranking[0] (top)    → primary-metric weight 0.7
ranking[1] (middle) → primary-metric weight 0.5
ranking[2] (bottom) → primary-metric weight 0.3
```

This means supplying a full `UserPreferences` ranking is strictly more informative than a bare `Priority`: it both linearizes Ω for the relaxation walk *and* sets a richer weight profile for all three policy translators simultaneously.

#### MCP usage

Pass `preferences` alongside `priority` to any evaluate or assess tool:

```json
{
  "filepath": "src/server.py",
  "priority": "composable",
  "preferences": { "ranking": ["composable", "secure", "simple"] }
}
```

The response includes a `preference_walk` block with `target`, `fallback_target`, `walk`, `next_step`, and `progress` (fraction from SLOP to IDEAL in [0, 1]). Use `topos_preference_walk` to get this walk without re-evaluating the file.

## Classification Result

```python
from topos import CharacteristicMorphism, ModuleDependencyGraph, ProgramMorphism

morphism = ProgramMorphism.from_file("my_code.py")
mdg = ModuleDependencyGraph.from_gitnexus_dir(".gitnexus", "my_code.py")  # optional; enables COMPOSABLE

# CFG / PDG / CPG are derived intrinsically from the morphism's UAST:
cfg = morphism.build_cfg()
cpg = morphism.build_cpg()

chi = CharacteristicMorphism()
result = chi.classify_detailed(morphism, representations=[cfg, cpg, mdg])

result.dimensions    # {"simple": EvaluationValue.SIMPLE, "composable": SLOP, "secure": SECURE}
result.scores        # {"simple": 0.72, "composable": 0.45, "secure": 0.99}
result.summary()     # EvaluationValue.SIMPLE_SECURE  (bits: 0b101)
result.raw_metrics   # {"cfg.cyclomatic": 8.0, "ast.entropy": 0.52, "cpg.dangerous_calls": 2, ...}
```

`ProgramCategory.classify_detailed(morphism)` is a one-line wrapper around the above for callers that don't want to construct representations manually.

## Adding a New Representation

1. Create `graphs/<name>/object.py` implementing the `Representation` protocol:
   - `name: str` — representation key (e.g., `"cfg"`, `"mdg"`).
   - `dimension: str` — the generator it feeds (`"simple"`, `"composable"`, or `"secure"`).
   - `metrics() -> dict[str, float]` — namespaced metric values (e.g., `{"cfg.cyclomatic": 8.0, ...}`).
2. Add raw metric probes in `topos/functors/probes/<name>/` (`P : E → ℝ`).
3. Register a score dispatcher in `topos/evaluation/characteristic_morphism.py`:
   - Add `_score_<name>(raw, priority)` returning a `ScoredDecision`.
   - Add to `_REPRESENTATION_SCORE_DISPATCHERS` keyed by `representation.name`.
4. (Optional) Add pairwise comparison in `topos/functors/profunctors/<name>/compare.py` (`D : E × E^op → ℝ`).
5. To introduce a new generator:
   - Extend `EvaluationValue` in `topos/core/omega.py` (the enum is a bitmask; widening it changes Ω's cardinality from `2^n`).
   - Extend `verdict_from_generators()` and add the new `Generator` member to `topos/evaluation/preferences.py`.
   - Add a `WeightProfile` entry for the new `Priority` member in `policies/base.py::WEIGHT_PROFILES`, and extend `WeightProfile.from_ranking()` if it uses hard-coded positions.
   - Add a policy translator `Φ_NEW` in `topos/evaluation/policies/`.
   - Update MCP `LatticeElement` enum and docs.

## MCP Server (`topos-mcp`)

Run with `topos-mcp` (stdio transport). Requires `TOPOS_MCP_FILE_ROOT` env var, or a project marker (`.git`/`pyproject.toml`) walking up from cwd — otherwise the server fails closed.

### Tools

- `topos_evaluate_code(code, language, priority, preferences, response_format)` — classify a string.  SIMPLE and SECURE are always scored; COMPOSABLE is unreachable from a bare string (no dependency graph). Pass `preferences` to receive a `preference_walk` block in the result.
- `topos_evaluate_file(filepath, priority, gitnexus_dir, preferences, response_format)` — classify a file. **Pass `gitnexus_dir` to enable the COMPOSABLE generator.** Pass `preferences` to receive a `preference_walk` block.
- `topos_evaluate_project(path, priority, gitnexus_dir, preferences, limit, offset, response_format)` — project-wide rollup with `ctx.report_progress`. Returns worst-scoring files first. Scores are per-generator; lattice value is the meet across files.
- `topos_compare_code(source_code, target_code, language, response_format)` — AST edit distance between two strings.
- `topos_compare_files(source, target, response_format)` — AST edit distance between two files.
- `topos_assess_improvement(proposed_code, filepath | current_code, priority, gitnexus_dir, preferences, response_format)` — agent refactor loop tool. Prefer `filepath` to enable COMPOSABLE scoring. Anti-gaming guardrail: returns `SUSPICIOUS_NO_STRUCTURAL_CHANGE` when scores move but AST edit distance is near zero. Pass `preferences` to drive the walk.
- `topos_inspect_code(code, language, priority, top_n_functions, response_format)` — detailed breakdown: top-N functions by CFG complexity, entropy details, full metric table.
- `topos_preference_walk(ranking, current, target, response_format)` — compute the induced total order on Ω and the relaxation walk for a given ranking, **without** re-evaluating any file. Pass `current` to get `next_step` and `progress` relative to a known verdict.

### Resources

- `topos://docs/lattice` — the 8-element lattice (SLOP / SIMPLE / COMPOSABLE / SECURE / SIMPLE_COMPOSABLE / SIMPLE_SECURE / COMPOSABLE_SECURE / IDEAL).
- `topos://docs/metrics` — every metric key, generator, threshold, and priority weight table.
- `topos://docs/priority` — when to use `simple` vs `composable` vs `secure` priority.
- `topos://docs/preferences` — full strict-ordering preferences: induced Ω order, two-stage targeting, relaxation walk, `UserPreferences` vs `Priority`. **Read before building an agent loop.**
- `topos://docs/workflows` — canonical review→plan→refactor→re-measure agent loop. Verdict stop condition is `IDEAL`. Read first.

### Prompts

- `topos_refactor_until_ideal(filepath, priority, max_iterations)` — scaffolds the full refactor loop.

### Package layout

Code lives under `topos/mcp/`:
- `server.py` — FastMCP instance + stdio entry point.
- `schemas.py` — Pydantic input + structured return models.
- `security.py` — fail-closed file-root resolution.
- `cache.py` — LRU cache for `ModuleDependencyGraph` keyed on `.gitnexus` mtime.
- `evaluation.py` — shared classifier pipeline (attaches dep graph when available).
- `formatting.py` — response builders.
- `tools/` — one module per tool category: `evaluate.py`, `compare.py`, `assess.py`, `inspect.py`.
- `resources/docs.py` + `resources/content/*.md` — static documentation.
- `prompts/refactor.py` — the refactor prompt template.

### Calibration & Benchmarking

- **Calibration Suite**: Located in `benchmarks/calibration/`. Contains infrastructure and data for validating Topos metric thresholds against real-world codebases. See `docs/calibration.md` for methodology.
- **Performance Benchmarks**: Located in `benchmarks/`. Side-by-side comparison scripts between Python and Rust implementations to verify speedups and algorithmic parity.

# Topos Agent Workflows

Per CodeScene's 2026 best-practice research ("agent-first tools need
AGENTS.md-style orchestration"), this document is the canonical recipe for
using Topos tools in a closed-loop refactor. Agents should read this on
first encounter with the server.

## The canonical loop: review → plan → refactor → re-measure

```
┌────────────┐      ┌────────────┐      ┌──────────────┐      ┌────────────┐
│ 1. MEASURE │ ───► │ 2. PLAN    │ ───► │ 3. PROPOSE   │ ───► │ 4. VERIFY  │
│  (evaluate)│      │ (identify  │      │  (refactor)  │      │  (assess)  │
│            │      │  weakest)  │      │              │      │            │
└────────────┘      └────────────┘      └──────────────┘      └─────┬──────┘
                                                                    │
                          ┌─────────────────────────────────────────┘
                          ▼
                    ┌──────────────┐
                    │ 5. DECIDE    │
                    │ accept / try │
                    │ again / stop │
                    └──────────────┘
```

### 1. Measure

- Single file: `topos_evaluate_file(filepath, gitnexus_dir)` — `gitnexus_dir`
  is required for the COMPOSABLE generator.  Without it, any verdict
  containing COMPOSABLE (including 🥇 **GOLD**) is unreachable.
- Whole project: `topos_evaluate_project(path, gitnexus_dir)` — rollup +
  worst-N file list. Start here to pick a target and "Go for Gold".

### 2. Plan

Read the `guidance` field of the evaluation result. It's priority-aware and
tells you which dimension to work on. If `guidance` says "provide
gitnexus_dir" you must run `topos depgraph generate` first.

For deep analysis of a specific file, call `topos_inspect_code` — it returns
top-N functions by complexity, entropy details, and the full metric table.

### 3. Propose

Write a refactor. Keep the change focused on one dimension at a time.
Submit via `topos_assess_improvement(filepath=..., proposed_code=...)`.

### 4. Verify

`topos_assess_improvement` returns one of:

- `IMPROVEMENT` — lattice moved up (e.g. ❌ SLOP → 🥉 BRONZE, or 🥉 BRONZE → 🥈 SILVER). Commit.
- `IMPROVEMENT_SCORE` — lattice unchanged but per-dim scores improved.
  Progress, but not a medal jump yet.
- `LATERAL_MOVE` — neither improved nor regressed. Try a different angle.
- `REGRESSION` / `REGRESSION_SCORE` — revert and re-plan.
- **`SUSPICIOUS_NO_STRUCTURAL_CHANGE`** — ⚠️ scores moved but AST barely
  changed. The refactor is probably cosmetic (whitespace / comments /
  renames). Make a structural change, not a textual one. **Do not commit.**

### 5. Decide

Stop when:
- Verdict = 🥇 **GOLD** (all three generators satisfied), OR
- Priority-specific generator satisfied (`simple` → SIMPLE bit set,
  `composable` → COMPOSABLE bit set, `secure` → SECURE bit set), OR
- `max_iterations` exhausted — report partial progress honestly rather than
  gaming one more iteration.

## Escape hatches — when the loop stalls

### Stall #1: Every generator score plateaus below 60%

Often a sign the file needs to be **split**, not refactored. Use
`topos_inspect_code` to find the top-complexity functions; consider
extracting them into a separate module. Re-run `topos_evaluate_project` to
check the rollup doesn't regress as a result.

### Stall #2: `SUSPICIOUS_NO_STRUCTURAL_CHANGE` repeatedly

You're iterating on presentation. Step back: what is the *structural*
problem? Rename → not a refactor. Whitespace → not a refactor. Loop
unrolling, extracted helpers, collapsed conditionals → real refactors.

### Stall #3: SIMPLE improves, COMPOSABLE regresses

Classic "moved complexity elsewhere" anti-pattern. Re-run
`topos_evaluate_project` — did the other file's score drop? If so, the
refactor didn't reduce total system complexity, it just relocated it.
Consider if the abstraction is actually an improvement or just a shuffle.

## Priority selection cheat sheet

- Leaf module (few callers) → `simple`
- Library surface (many importers) → `composable`
- File handling untrusted input → `secure`
- Unknown / general cleanup → `secure` (default scorer emphasis)

See `topos://docs/priority` for more.

## Preference-driven targeting

For agent loops that need a concrete *next-best* verdict to aim for —
not just an upweighted generator — pass `preferences` alongside
`priority`. A `preferences.ranking` like `["composable", "secure",
"simple"]` induces a total order on Ω and produces a **two-stage**
target:

1. **`target`** — aspirational, default 🥇 **GOLD**. Try to beat the
   thresholds for all three generators first.
2. **`fallback_target`** — the **"ideal intersection"**, i.e. the meet
   of the top-two ranked generators (🥈 **SILVER**). When 🥇 **GOLD** plateaus, divert here.

The result also returns a **`walk`** (descending verdicts from GOLD
down) and a **`next_step`** (the smallest improvement above the
current verdict).

Concretely: aim for 🥇 **GOLD** for the first few iterations; if the lattice
verdict won't move, switch to `fallback_target` (🥈 **SILVER**) and try to satisfy
only the top-two generators. See `topos://docs/preferences`.

## What Topos does NOT measure

- **Test coverage.** A refactor that improves the score but breaks tests
  is a regression. Topos cannot see this; run the test suite separately.
- **Functional correctness.** AST edit distance measures *change*, not
  *preservation of behavior*. Always verify behavior with tests.
- **Runtime performance.** Orthogonal to all Topos metrics.
- **Beyond-syntactic security.** The SECURE generator catches obvious
  footguns (dangerous-API call sites, source→sink taint paths) via
  textual / structural pattern matching on the CPG.  It is not a full
  SAST / pen-test — pair with dedicated security tooling for high-stakes
  code.

Topos is one signal in a multi-signal loop. Pair it with test coverage and
type checks for the full picture.
