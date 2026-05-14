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

pytest                                   # Run all tests
pytest tests/test_file.py::test_name    # Run a specific test
pytest -v --tb=short                    # Verbose with short tracebacks

ruff check src/ --fix && ruff format src/   # Lint and format
```

## CLI Usage

```bash
topos evaluate path/to/code.py
topos evaluate src/ -r --priority simple
topos evaluate src/ -r --gitnexus-dir .gitnexus --priority composable
topos evaluate src/ -r --gitnexus-dir .gitnexus --priority secure
topos compare file1.py file2.py -v
topos inspect module.py --gitnexus-dir .gitnexus
topos structural-test-coverage path/to/code.py --language python
topos depgraph generate                 # Wraps 'gitnexus analyze' (requires npm install -g gitnexus)
topos-mcp                               # Run the FastMCP server
```

## Architecture

### Three-layer model: topos primitives, representations, decision layer

The codebase mirrors the math spec:

- **`topos.core/`** — the program topos's defining structure.
  - `ProgramObject`, `ProgramMorphism`, `ProgramCategory` — objects and morphisms.
  - `Omega` (in `core/omega.py`) — the subobject classifier and value Heyting algebra.  Holds `EvaluationValue` (8 elements: `SLOP`, `SIMPLE`, `COMPOSABLE`, `SECURE`, the three pair meets, and `IDEAL`) plus `meet`/`join`/`implies`/`negation`.
- **`topos.graphs/`** — translational functors `R : Lang → E`.  One subpackage per representation, each conforming to the `Representation` protocol:
  - `ast` — concrete syntax via tree-sitter.
  - `uast` — language-independent normalized AST (substrate for the rest).
  - `cfg` — intra-procedural control flow graph (feeds SIMPLE).
  - `pdg` — academic Program Dependence Graph (intra-procedural; consumed by CPG).
  - `mdg` — module dependency graph parsed from GitNexus (feeds COMPOSABLE).
  - `cpg` — Code Property Graph fusing AST ∪ CFG ∪ DDG ∪ CDG (feeds SECURE).
- **`topos.evaluation/`** — the decision layer: how raw measurements become Ω verdicts.
  - `characteristic_morphism.py` — `CharacteristicMorphism` (χ_S : P → Ω) and `ClassificationResult`.
  - `policies/` — policy translators `Φᵢ : ℝ → Ω`, one per generator (`score_simple`, `score_coupling`, `score_secure`).
- **`topos.functors/`** — probes and profunctors over representations.
  - `probes/<rep>/` — single-program measurements `P : E → ℝ` (e.g. `probes.cfg.cyclomatic_complexity`).
  - `profunctors/<rep>/` — pairwise comparisons `D : E × E^op → ℝ` (e.g. `profunctors.cpg.compare_cpg`).

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
| SIMPLE | `ControlFlowGraph` (+ `ASTRepresentation` entropy) | `cfg.cyclomatic`, `cfg.essential`, `cfg.nesting_depth`, `cfg.longest_path`, `ast.entropy` | Weighted: `1 - cyclomatic/40` + entropy bell-curve (peak at 0.5). Threshold: 0.6. |
| COMPOSABLE | `ModuleDependencyGraph` | `mdg.coupling`, `mdg.instability`, `mdg.fan_in`, `mdg.fan_out`, `mdg.dep_depth` | Weighted: `1 - coupling/35` + instability flat-top tent over [0.3, 0.7]. Threshold: 0.6. |
| SECURE | `CodePropertyGraph` | `cpg.dangerous_calls`, `cpg.taint_flows` | Weighted exp-decay: `exp(-count / scale)` for each metric. Threshold: 0.6. |

**Diagnostic (not counted toward verdict):**
- `ProgramDependenceGraph`: `pdg.data_deps`, `pdg.control_deps`, `pdg.density` — intra-procedural dependence analysis.

**`--priority`** shifts metric weights *within* each generator (via `Priority` enum: `BALANCED`, `SIMPLE`, `COMPOSABLE`, `SECURE`). It does not change the lattice structure or which generators score.

### Key Non-Obvious Behaviors

- **SIMPLE and SECURE always run.** CFG and CPG are derived from the UAST built during parsing — no external tooling required.  Parse failures collapse the whole verdict to `SLOP`.
- **COMPOSABLE is unreachable without `.gitnexus/`.**  Module dependency evaluation only runs when a `ModuleDependencyGraph` is loaded from a `.gitnexus/` directory (`--gitnexus-dir`).  Any verdict containing COMPOSABLE (including `IDEAL`) is then unreachable.
- **GitNexus is an external npm tool.**  Run `topos depgraph generate` (which calls `gitnexus analyze`) to produce the `.gitnexus/` directory consumed by `--gitnexus-dir`.
- **Parse failures kill the verdict.** `is_parseable=False` → `lattice_element = SLOP`; in `combine_dimensions()` they inject a `0.0` score on SIMPLE, pulling multi-file aggregation down.
- **Mixed representations within a generator** are scored independently and combined via `min()` (conservative).  E.g. AST entropy and CFG cyclomatic both feed SIMPLE; the generator passes iff both individual decisions pass.
- **Three generators are orthogonal.** A file can be SIMPLE without being COMPOSABLE, COMPOSABLE without being SECURE, etc.  The 8-element Ω encodes every combination.

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
   - Extend `verdict_from_generators()` and add a `WeightProfile` entry for the new priority.
   - Add a policy translator `Φ_NEW` in `topos/evaluation/policies/`.
   - Update MCP `LatticeElement` enum and docs.

## MCP Server (`topos-mcp`)

Run with `topos-mcp` (stdio transport). Requires `TOPOS_MCP_FILE_ROOT` env var, or a project marker (`.git`/`pyproject.toml`) walking up from cwd — otherwise the server fails closed.

### Tools

- `topos_evaluate_code(code, language, priority, response_format)` — classify a string.  SIMPLE and SECURE are always scored; COMPOSABLE is unreachable from a bare string (no dependency graph).
- `topos_evaluate_file(filepath, priority, gitnexus_dir, response_format)` — classify a file. **Pass `gitnexus_dir` to enable the COMPOSABLE generator.**
- `topos_evaluate_project(path, priority, gitnexus_dir, limit, offset, response_format)` — project-wide rollup with `ctx.report_progress`. Returns worst-scoring files first. Scores are per-generator; lattice value is the meet across files.
- `topos_compare_code(source_code, target_code, language, response_format)` — AST edit distance between two strings.
- `topos_compare_files(source, target, response_format)` — AST edit distance between two files.
- `topos_assess_improvement(proposed_code, filepath | current_code, priority, gitnexus_dir, response_format)` — agent refactor loop tool. Prefer `filepath` to enable COMPOSABLE scoring. Anti-gaming guardrail: returns `SUSPICIOUS_NO_STRUCTURAL_CHANGE` when scores move but AST edit distance is near zero.
- `topos_inspect_code(code, language, priority, top_n_functions, response_format)` — detailed breakdown: top-N functions by CFG complexity, entropy details, full metric table.

### Resources

- `topos://docs/lattice` — the 8-element lattice (SLOP / SIMPLE / COMPOSABLE / SECURE / SIMPLE_COMPOSABLE / SIMPLE_SECURE / COMPOSABLE_SECURE / IDEAL).
- `topos://docs/metrics` — every metric key, generator, and threshold.
- `topos://docs/priority` — priority profiles (balanced / simple / composable / secure).
- `topos://docs/workflows` — canonical review→plan→refactor→re-measure agent loop. Verdict stop condition is `IDEAL`. Read first.

### Prompts

- `topos_refactor_until_ideal(filepath, priority, max_iterations)` — scaffolds the full refactor loop.

### Package layout

Code lives under `src/topos/mcp/`:
- `server.py` — FastMCP instance + stdio entry point.
- `schemas.py` — Pydantic input + structured return models.
- `security.py` — fail-closed file-root resolution.
- `cache.py` — LRU cache for `ModuleDependencyGraph` keyed on `.gitnexus` mtime.
- `evaluation.py` — shared classifier pipeline (attaches dep graph when available).
- `formatting.py` — response builders.
- `tools/` — one module per tool category: `evaluate.py`, `compare.py`, `assess.py`, `inspect.py`.
- `resources/docs.py` + `resources/content/*.md` — static documentation.
- `prompts/refactor.py` — the refactor prompt template.

### Evaluation harness

`evaluations/topos_mcp.xml` — 10 Q/A pairs per the mcp-builder skill Phase 4.
