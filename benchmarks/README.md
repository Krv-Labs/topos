# Topos Rust Migration & Benchmarking

## Overview
As of v1.1.0, performance-critical components of the Topos backend have been migrated to Rust (`topos-functors`). This migration adopts a hybrid architecture where documented Python modules act as wrappers for a high-performance Rust core exposed via `PyO3`.

## Performance Speedups
Benchmarking against the pure-Python v1.0.0 release shows significant efficiency gains across all structural metrics:

| Metric Category | Implementation | Complexity | Observed Speedup |
|-----------------|----------------|------------|------------------|
| **CFG Probes**  | Rust (petgraph)| $O(V+E)$   | ~5-10x           |
| **AST Entropy** | Rust (flate2)  | $O(N)$      | ~2-5x            |
| **Edit Distance**| Rust (Wagner-Fischer)| $O(N \cdot M)$ | ~10-20x          |

*Note: Overall `topos evaluate` execution time has decreased by approximately **6-8x** on average source files.*

## Implementation Equivalence & Variances
We have algorithmically verified the equivalence of the Rust implementation with the original Python logic.

### 1. Categorical Functors
The transformation from source programs (Category $\mathcal{P}$) to structured representations (Category $\mathcal{E}$) is implemented as a **Functor** $R: \mathcal{P} \to \mathcal{E}$. The Rust backend faithfully preserves the structural mappings defined in the original specification.

### 2. Metric Precision
While the algorithms are equivalent, minor floating-point variances may exist due to low-level library differences:

- **Entropy (Kolmogorov Proxy):** 
  - Python uses `zlib` (level 9). 
  - Rust uses `flate2` with `ZlibEncoder` (level 9).
  - **Observed Variance:** $\approx 2.4 \times 10^{-4}$ (0.02%).
  - **Reason:** Minor differences in compression heuristics and headers between C-based zlib and Rust's miniz_oxide backend.
  
- **Graph Algorithms (Complexity, Pathing):**
  - **Observed Variance:** 0.0% (Exact match).
  - Both implementations follow identical BFS/DFS and connected-component logic.

- **Edit Distance:**
  - **Observed Variance:** 0.0% (Exact match).
  - Both implementations use the Wagner-Fischer dynamic programming algorithm.

## Running Benchmarks
To reproduce these results, use the provided benchmarking suite:

```bash
# Setup: Download v1.0.0 binary
mkdir -p benchmarks/bin
curl -L https://github.com/Krv-Labs/topos/releases/download/v1.0.0/topos-macos-arm64 -o benchmarks/bin/topos-v1.0.0
chmod +x benchmarks/bin/topos-v1.0.0

# Run side-by-side comparison
uv run python benchmarks/run_bench.py
```
