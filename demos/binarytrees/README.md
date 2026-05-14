# Binary Trees AST Comparison

This demo generates Abstract Syntax Trees (ASTs) for the Binary Trees benchmark implemented in multiple languages.

## Supported Languages
- Python
- JavaScript (Node.js)
- Rust
- C++

## Setup

Ensure you have installed the required tree-sitter parsers:

```bash
uv add tree-sitter-python tree-sitter-rust tree-sitter-javascript tree-sitter-cpp
```

## Run AST Extraction

From the repository root:

```bash
uv run python demos/binarytrees/get_asts.py
```

This will:
1. Load the source files from `demos/binarytrees/src/`.
2. Parse them through the multi-backend AST dispatch pipeline.
3. Save conformance artifacts under `demos/binarytrees/asts/`:
   - `treesitter/*.ast.txt` (CST S-expressions)
   - `uast/*.uast.json` (normalized UAST)
   - `native/*.native.txt` (native AST dump when available)

## Comparing Implementations

After extracting the ASTs, run the cross-language comparison:

```bash
uv run python demos/binarytrees/compare_asts.py
```

This builds a pairwise 4x4 structural comparison over the UAST representations and writes:
- `results/comparison.json` — full machine-readable report (kind histograms, edit distance, control-flow deltas, summary deltas).
- `results/comparison.md` — markdown distance matrices plus per-pair control-flow deltas.

### Interpreting the output

- **Kind-histogram distance** (L1, `[0, 1]`): measures how differently each language uses UAST node kinds. `0.0` means identical kind mix, `1.0` means disjoint vocabularies.
- **UAST edit distance** (normalized, `[0, 1]`): Wagner-Fischer edit distance over DFS-ordered UAST kinds. Captures structural shape, not just kind frequency.
- **Control-flow delta**: signed counts (`target - source`) for control-flow-relevant UAST kinds (`IfStmt`, `ForStmt`, `WhileStmt`, `MatchStmt`, `CallExpr`, `ReturnStmt`, ...). Empty delta vs non-empty delta is the strongest "did we detect a difference?" signal.

## Purpose

This is an experimental setup for **Issue #12: Compare AST from different languages**. The goal is to determine if structural or logic differences can be detected between implementations of the same algorithm across different programming languages. The comparison runner answers that question quantitatively via the UAST profunctor module ([src/topos/functors/profunctors/uast/](../../src/topos/functors/profunctors/uast/)).
