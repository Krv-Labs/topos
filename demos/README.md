# Topos Demos

This directory contains demonstrations and experimental setups for Topos.

## Available Demos

### 1. [Library Version Evaluations](./libraries/README.md)
Compares two versions of popular Python libraries (`numpy`, `scipy`, `scikit-learn`, `networkx`) using Topos structural metrics.
- **Location:** `demos/libraries/`
- **Runner:** `uv run python demos/libraries/run_all.py`

### 2. [Binary Trees AST Comparison](./binarytrees/README.md)
Generates and compares ASTs for the Binary Trees benchmark across multiple languages (Python, Rust, JS, C++).
- **Location:** `demos/binarytrees/`
- **Runner:** `uv run python demos/binarytrees/get_asts.py`

### 3. [Measure Sensitivity Benchmark](./sensitivity/README.md)
Curates Self-Contained and Composable reference programs from popular packages, then applies axis-specific I/O-preserving noise to characterize how the Topos scores respond. Outputs feed into regularization analysis.
- **Location:** `demos/sensitivity/`
- **Runners:** `uv run python demos/sensitivity/curate.py` → `experiments/run_structural.py` → `experiments/run_coupling.py`

## Structure

Each demo is organized as a self-contained module:
- `README.md`: Specific instructions for the demo.
- `run.py` or similar: The main entry point.
- `src/`: (Optional) Source code or assets used by the demo.
- `results/`: (Optional) Output artifacts.
