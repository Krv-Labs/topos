# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

**Topos** is a code quality evaluation framework that applies category theory (specifically Heyting Algebra) to classify Python code. Programs are evaluated across two independent *dimensions* and mapped to a 4-element diamond lattice rather than a single numeric score.

The project consists of:
- A CLI tool (`topos`) for evaluating and comparing code
- An MCP server (`topos-mcp`) exposing code analysis as tools for Claude and other MCP clients
- Category-theoretic abstractions: `ProgramMorphism`, `ProgramObject`, `SubobjectClassifier`

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
topos evaluate src/ -r --priority self_contained
topos evaluate src/ -r --gitnexus-dir .gitnexus --priority composable
topos compare file1.py file2.py -v
topos inspect module.py --gitnexus-dir .gitnexus
topos depgraph generate                 # Wraps 'gitnexus analyze' (requires npm install -g gitnexus)
topos-mcp                               # Run the FastMCP server
```

## Architecture

### Two-Layer Representation Model

The codebase separates *representations* (structural views of a program) from *raw metric functions*:

- **`graphs/`** — `Representation` protocol + concrete implementations (`ASTRepresentation`, `DependencyGraph`). These are the objects passed to the classifier.
- **`metrics/`** — Pure functions that compute raw floats from ASTs or dep-graph data. Called by representations via their `metrics()` method.

The `Representation` protocol (`graphs/base.py`) requires:
- `name: str` — identifies the representation type (`"ast"`, `"depgraph"`)
- `dimension: str` — the quality axis it measures (`"structural"`, `"coupling"`)
- `metrics() -> dict[str, float]` — computes namespaced metric values

New representations go in `graphs/`; new measurement functions go in `metrics/`.

### Evaluation Flow

```
ProgramMorphism.from_file(path)
  → ASTRepresentation(morphism.ast)   [always built by classifier]
  → DependencyGraph.from_gitnexus_dir(...)  [optional; requires .gitnexus/]

SubobjectClassifier.classify_detailed(morphism, representations=[depgraph], priority=...)
  → Group representations by dimension
  → Call rep.metrics() → raw floats
  → score_structural(complexity, entropy) → ScoredDecision [structural dim]
  → score_coupling(coupling, instability) → ScoredDecision [coupling dim]
  → score ≥ 0.6 → dimension achieved → mapped to lattice target
  → Return ClassificationResult
```

### The Diamond Lattice

`EvaluationLattice` implements a Heyting Algebra with four values:

```
        ⊤ SOUND         (both targets achieved)
       /  \
  COMPOSABLE  SELF_CONTAINED    (incomparable — neither ≤ the other)
       \  /
        ⊥ BROKEN        (neither achieved)
```

Key property: **COMPOSABLE and SELF_CONTAINED are incomparable**. `meet(COMPOSABLE, SELF_CONTAINED) = BROKEN`. In multi-file rollup, `combine_dimensions()` uses minimum score per dimension and re-applies the threshold — it is not a direct lattice meet.

### The Two Dimensions

| Dimension | Representation | Target | Metrics |
|-----------|---------------|--------|---------|
| structural | `ASTRepresentation` | `SELF_CONTAINED` | `ast.complexity`, `ast.entropy` |
| coupling | `DependencyGraph` | `COMPOSABLE` | `depgraph.coupling`, `depgraph.instability`, `depgraph.fan_in`, `depgraph.fan_out`, `depgraph.dep_depth` |

**Scoring thresholds:**
- Structural: score = weighted average of `1 - complexity/40` and bell-curve entropy (ideal = 0.5). Threshold: 0.6.
- Coupling: score = weighted average of `1 - coupling/35` and instability quality (sweet spot [0.3, 0.7]). Threshold: 0.6.

**`--priority`** shifts metric weights *within* each dimension (via `Priority` enum: `BALANCED`, `COMPOSABLE`, `SELF_CONTAINED`). It does not change the lattice structure.

### Key Non-Obvious Behaviors

- **COMPOSABLE is unreachable without a DependencyGraph.** Coupling evaluation only runs when a `DependencyGraph` representation is provided (via `--gitnexus-dir`). Without it, only structural runs.
- **GitNexus is an external npm tool.** Run `topos depgraph generate` (which calls `gitnexus analyze`) to produce the `.gitnexus/` directory consumed by `--gitnexus-dir`.
- **Parse failures are always structural failures.** `is_parseable=False` → `BROKEN`; in `combine_dimensions()` they inject a 0.0 structural score, pulling multi-file aggregation down.
- **Mixed representations within a dimension** are scored independently and combined via `min()` (conservative).
- **`metrics/depgraph/fan.py`** computes fan-in/fan-out and dependency depth — separate from coupling/instability in `metrics/depgraph/coupling.py`.

## Classification Result

```python
from topos import ProgramMorphism, SubobjectClassifier
from topos.graphs.depgraph.graph import DependencyGraph

morphism = ProgramMorphism.from_file("my_code.py")
depgraph = DependencyGraph.from_gitnexus_dir(".gitnexus", "my_code.py")  # optional

classifier = SubobjectClassifier()
result = classifier.classify_detailed(morphism, representations=[depgraph])

result.dimensions    # {"structural": SELF_CONTAINED, "coupling": COMPOSABLE}
result.scores        # {"structural": 0.72, "coupling": 0.65}
result.summary()     # EvaluationValue.SOUND (both achieved)
result.raw_metrics   # {"ast.complexity": 8.0, "ast.entropy": 0.52, ...}
```

## Adding a New Representation

1. Create `graphs/<name>/object.py` implementing the `Representation` protocol (`name`, `dimension`, `metrics()`)
2. Add raw metric functions to `metrics/<name>/`
3. Add a score dispatcher to `omega._REPRESENTATION_SCORE_DISPATCHERS`
4. Add the dimension target to `omega._DIMENSION_TARGET` if introducing a new dimension

## MCP Server Tools

- `evaluate_code(code, language)` — Classify code from a string
- `evaluate_file(filepath)` — Classify code from a file
- `compare_code(source_code, target_code, language)` — Compare AST edit distance
- `compare_files(source, target)` — Compare two files
- `assess_improvement(current_code, proposed_code, language)` — Check if proposed code improves current
- `inspect_code(code, language)` — Detailed metrics breakdown
