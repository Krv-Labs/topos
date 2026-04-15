# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

**Topos** is a code quality evaluation framework that applies category theory (specifically Heyting Algebra) to classify Python code. Instead of a single numeric score, every program is mapped to one of six evaluation stages in a lattice, combining cyclomatic complexity and entropy metrics.

The project consists of:
- A CLI tool for evaluating and comparing code
- An MCP (Model Context Protocol) server exposing code analysis as tools
- Category-theoretic abstractions: ProgramMorphism, ProgramObject, SubobjectClassifier

## Development Commands

**Installation & Setup:**
```bash
uv pip install -e .                    # Install in editable mode
uv pip install -e ".[dev]"             # Install with dev dependencies (pytest, ruff)
```

**Running Tests:**
```bash
pytest                                  # Run all tests
pytest tests/test_file.py              # Run a specific test file
pytest tests/test_file.py::test_name   # Run a specific test
pytest -v                               # Verbose output
pytest --tb=short                       # Shorter traceback format
```

**Linting & Code Quality:**
```bash
ruff check src/                         # Check code style
ruff check src/ --fix                   # Auto-fix style issues
ruff format src/                        # Format code
```

**CLI Usage:**
```bash
topos evaluate path/to/code.py          # Classify a single file
topos evaluate src/ -r                  # Recursively classify a directory
topos evaluate *.py -v                  # Verbose output with metrics
topos evaluate src/ --json              # JSON output
topos compare file1.py file2.py         # Compare AST structure (edit distance)
topos compare file1.py file2.py -v      # Show operation counts
topos inspect module.py                 # Detailed metrics and classifications
```

**MCP Server:**
```bash
topos-mcp                               # Run the FastMCP server (tools available for Claude)
```

## Project Architecture

### Directory Structure
```
src/topos/
├── core/              # Core categorical abstractions
│   ├── morphism.py    # ProgramMorphism: programs as transformations
│   ├── object.py      # ProgramObject: AST representation
│   └── __init__.py
├── logic/             # Lattice and classification logic
│   ├── lattice.py     # EvaluationLattice: Heyting Algebra (6-valued logic)
│   ├── omega.py       # SubobjectClassifier: classification engine
│   ├── policies.py    # Metric-to-lattice evaluation rules
│   └── __init__.py
├── metrics/           # Code quality metrics
│   ├── complexity.py  # Cyclomatic complexity calculation
│   ├── entropy.py     # Kolmogorov proxy via compression
│   ├── distance.py    # AST edit distance (topological drift)
│   └── __init__.py
├── utils/             # Utilities
│   ├── tree_sitter.py # Tree-sitter AST parsing wrapper
│   └── __init__.py
├── main.py            # CLI entry point (Click)
└── server.py          # MCP server (FastMCP)
```

### Core Concepts

**ProgramMorphism** (`core/morphism.py`)
- Central abstraction: represents a program as a transformation between computational states
- Wraps source code and its parsed AST (ProgramObject)
- Can be created from source string or file
- Property `is_valid` checks syntactic validity

**ProgramObject** (`core/object.py`)
- Encapsulates the AST from tree-sitter
- Provides methods to calculate AST metrics (depth, node count, etc.)
- Used by metrics calculators

**EvaluationLattice** (`logic/lattice.py`)
- Implements a Heyting Algebra with 6 values: BROKEN (⊥) → ENTANGLED → COUPLED | COMPLEX → STABLE → SOUND (⊤)
- Supports lattice operations: `meet()`, `join()`, `implies()`, `complement()`
- Each label describes a structural observation (what metrics detect), not an abstract quality judgment

**SubobjectClassifier** (`logic/omega.py`)
- The classification engine; groups representations by *dimension* and aggregates within each
- `classify_detailed()` returns a `ClassificationResult` with `dimensions: dict[str, EvaluationValue]`
- `classify()` returns `result.summary()` — the worst value across all dimensions
- `combine_dimensions()` aggregates per-dimension verdicts across multiple files

**Metrics**
- `complexity.py`: Cyclomatic complexity (branches, loops, conditions)
- `entropy.py`: Kolmogorov complexity proxy using compression ratios
- `distance.py`: Tree-sitter AST edit distance for structural comparison

### Evaluation Rules

Classification is determined per *dimension*:
1. **Syntax validity** → if parsing fails, `is_parseable=False` and `dimensions` is empty
2. **Structural dimension** (always present):
   - Complexity → cyclomatic complexity mapped via bins in `policies.py`
   - Entropy → Kolmogorov-proxy compression ratio mapped via bins in `policies.py`
   - Combined via `meet()` in the non-total lattice
3. **Coupling dimension** (present when a `DependencyGraph` is passed):
   - Coupling + instability mapped via bins in `dep_policies.py`

See `logic/policies.py` and `logic/dep_policies.py` for threshold bins.

## Common Workflows

**Adding a New Metric:**
1. Add calculation function to `metrics/` (e.g., `metrics/my_metric.py`)
2. Update the relevant evaluation section in `policies.py` or `dep_policies.py`
3. Register a verdict dispatcher in `omega._REPRESENTATION_VERDICT_DISPATCHERS`
4. Add the metric key to the appropriate representation's `metrics()` method

**Testing a Code Evaluation:**
```python
from topos import ProgramMorphism, SubobjectClassifier

morphism = ProgramMorphism.from_file("my_code.py")
classifier = SubobjectClassifier()
result = classifier.classify_detailed(morphism)

print(result.dimensions)          # {"structural": <EvaluationValue.STABLE: 4>}
print(result.summary())           # ◐ STABLE
print(result.raw_metrics)         # {"ast.complexity": 12.0, "ast.entropy": 0.44}
```

**Comparing Two Files:**
```python
from topos.metrics.distance import calculate_ast_distance
from topos import ProgramMorphism

morph1 = ProgramMorphism.from_file("original.py")
morph2 = ProgramMorphism.from_file("refactored.py")

result = calculate_ast_distance(morph1.ast, morph2.ast)
print(f"Similarity: {1 - result.normalized_distance:.1%}")
```

## Dependencies

**Core:**
- `tree-sitter` (>=0.23): AST parsing
- `tree-sitter-python` (>=0.23): Python language support
- `click` (>=8.1): CLI framework
- `fastmcp`: Model Context Protocol server

**Development:**
- `pytest` (>=9.0.2): Testing framework
- `ruff` (>=0.15.8): Linting & formatting

## Configuration

**Ruff (`pyproject.toml`):**
- Line length: 88
- Active rules: E, F, I, UP, B, SIM (style, format, imports, upgrades, bugbear, simplify)

**Pytest (`pyproject.toml`):**
- Test directory: `tests/`
- Python path: `src/` (for imports)

**Build System:**
- Backend: `hatchling`
- Package source: `src/topos`

## Key Files to Understand First

1. **README.md** — Overview of the evaluation lattice and CLI examples
2. **src/topos/__init__.py** — Public API exports
3. **src/topos/main.py** — CLI commands (evaluate, compare, inspect)
4. **src/topos/logic/lattice.py** — EvaluationValue enum and lattice operations
5. **src/topos/logic/omega.py** — Classification logic and result structure
6. **src/topos/server.py** — MCP tools for code evaluation

## MCP Server Tools

The `topos-mcp` server exposes these tools for use by Claude and other MCP clients:

- **evaluate_code(code, language)** — Classify code from a string
- **evaluate_file(filepath)** — Classify code from a file
- **compare_code(source_code, target_code, language)** — Compare AST distance
- **compare_files(source, target)** — Compare two files
- **assess_improvement(current_code, proposed_code, language)** — Check if proposed code improves current
- **inspect_code(code, language)** — Detailed metrics breakdown
