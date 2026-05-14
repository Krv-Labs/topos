# Topos Measure Sensitivity Benchmark

Issue [#13](https://github.com/Krv-Labs/topos/issues/13). Measures how Topos
generators respond to controlled, I/O-preserving perturbations of reference
code. The output is data + observations to inform whether the score
normalization constants in
[`topos.evaluation.policies`](../../src/topos/evaluation/policies/) need regularizing.

## What gets measured

The scoring generators are independently sensitive:

- **SIMPLE** (`topos.evaluation.policies.simple`)
  - `cfg.cyclomatic` and related CFG probes drive code-complexity quality.
  - `ast.entropy` → bell-curve peak at `ENTROPY_IDEAL = 0.5`.
  - Scored from intrinsic AST/CFG/PDG/CPG construction; a single file is enough.
- **COMPOSABLE** (`topos.evaluation.policies.coupling`)
  - `mdg.coupling` → linear fall to 0 at `MAX_COUPLING = 35`.
  - `mdg.instability` → flat-top tent over `[INSTABILITY_LOW=0.3, INSTABILITY_HIGH=0.7]`.
  - Requires a `ModuleDependencyGraph` from `.gitnexus/` (multi-module package needed).

Single-file noise tests SIMPLE; package-level noise tests COMPOSABLE. The two
are exercised separately.

## Layout

```
demos/sensitivity/
├── README.md
├── curate.py                   # PyPI sdists → corpus/, baseline scoring, manifest.json
├── corpus/
│   ├── self_contained/         # 3 single-file SIMPLE references
│   └── composable/             # 2 multi-module slices
├── noise/
│   ├── structural.py           # 4 single-file transforms
│   └── coupling.py             # 4 package-level transforms
├── experiments/
│   ├── run_structural.py       # sweep + score SIMPLE
│   └── run_coupling.py         # sweep + score COMPOSABLE
└── results/
    ├── self_contained_sweep.json/.md
    ├── composable_sweep.json/.md
    └── regularization_notes.md
```

## Prerequisites

```bash
uv sync                         # from repo root
npm install -g gitnexus        # required for Composable scoring
```

The runners use the current Topos Python API directly. Run them with
`uv run python ...`.

## Workflow

```bash
# 1. Curate the corpus (downloads sdists, scores baselines, pins selections)
uv run python demos/sensitivity/curate.py

# 2. Sweep structural noise over the SIMPLE corpus
uv run python demos/sensitivity/experiments/run_structural.py

# 3. Sweep coupling noise over the COMPOSABLE corpus
uv run python demos/sensitivity/experiments/run_coupling.py
```

Each runner writes a JSON artifact and a markdown matrix into `results/`.
Read `results/regularization_notes.md` for the human writeup.

## How to interpret the matrices

Rows are noise intensities (`0` is the unperturbed baseline). Columns are
transform names. Cells report `score [lattice_element]`. A transform that
crashes the smoke test for a given intensity is marked `--` and excluded
from analysis.

The AST drift guardrail uses
[`topos.functors.profunctors.ast.compare.calculate_ast_distance`](../../src/topos/functors/profunctors/ast/compare.py)
is recorded alongside each cell: large score moves with tiny edit distances
indicate the metric is over-responsive to surface changes.
