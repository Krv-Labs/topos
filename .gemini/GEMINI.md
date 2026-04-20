# Topos - Project Context

## Project Overview

**Topos** is a Python-based static analysis tool and MCP (Model Context Protocol) server. It provides structural code quality metrics that agents and developers can act upon. Topos models programs as morphisms (maps) on graphs and classifies them using a four-valued diamond lattice (Heyting algebra) to evaluate design properties beyond standard input-output behavior.

### Key Concepts
*   **Evaluation Lattice:** 
    *   `⊥ BROKEN`: Fails quality targets.
    *   `◐ SELF_CONTAINED`: Good internal structure (Complexity, Entropy).
    *   `◑ COMPOSABLE`: Good external coupling (Dependency graph via GitNexus).
    *   `⊤ SOUND`: Meets both structural and coupling targets.
*   **Pillars / Dimensions:**
    *   **Structural Pillar:** Analyzes Abstract Syntax Tree (AST) using Tree-sitter. Measures Cyclomatic Complexity and Entropy.
    *   **Coupling Pillar:** Analyzes the repository's dependency graph. Uses LadybugDB and requires `GitNexus` (`npm install -g gitnexus`).

### Architecture & Stack
*   **Language:** Python 3.11+
*   **CLI:** Built with `click` (entry point `topos`).
*   **MCP Server:** Built with `fastmcp` (entry point `topos-mcp`).
*   **Parsing:** `tree-sitter` for AST construction.
*   **Package Management:** `uv` is heavily used in CI for dependency management and execution.

---

## Building, Running, and Testing

This project uses `hatchling` as its build backend and `uv` for local environment management, testing, and linting.

### Environment Setup
To install dependencies for development:
```bash
uv sync --group dev --group docs
```

### Running the CLI
You can execute the CLI commands locally via `uv run`:
```bash
# Evaluate a directory recursively
uv run topos evaluate src/ -r --priority self_contained

# Inspect a specific module
uv run topos inspect module.py

# Run the MCP Server
uv run topos-mcp
```

### Testing
Tests are located in the `tests/` directory and use `pytest` with `pytest-cov`.
```bash
# Run all tests
uv run pytest -v

# The CI automatically runs coverage reports:
uv run pytest --cov=src/topos --cov-report=term-missing
```

---

## Development Conventions

*   **Linting and Formatting:** The project strictly uses `ruff`.
    *   Line length: `88` characters.
    *   Run lint checks: `uv run ruff check src tests`
    *   Run formatting: `uv run ruff format src tests`
*   **Type Hinting:** Code should utilize standard Python 3.11+ type hints (e.g., `|` for Unions, `list[str]`). Follow strict typing guidelines where possible.
*   **Documentation:** Documentation is built using Sphinx (`docs/` directory). Any new features or metric updates should be documented in the corresponding `.rst` files.
*   **GitNexus Integration:** Any changes to the Coupling/Dependency graph dimension must be tested against the output format of GitNexus. Use `topos depgraph generate` locally to populate `.gitnexus/` for testing.
*   **Spelling Conventions:** Opt for US spelling conventions (e.g., "modeling", "optimizing") across both code and documentation.
