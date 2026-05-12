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
uv run demos/binarytrees/get_asts.py
```

This will:
1. Load the source files from `demos/binarytrees/src/`.
2. Parse them through the multi-backend AST dispatch pipeline.
3. Save conformance artifacts under `demos/binarytrees/asts/`:
   - `treesitter/*.ast.txt` (CST S-expressions)
   - `uast/*.uast.json` (normalized UAST)
   - `native/*.native.txt` (native AST dump when available)

## Purpose

This is an experimental setup for **Issue #12: Compare AST from different languages**. The goal is to determine if structural or logic differences can be detected between implementations of the same algorithm across different programming languages.
