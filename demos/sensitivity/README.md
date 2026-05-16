# Topos Measure Sensitivity Benchmark

Issue [#13](https://github.com/Krv-Labs/topos/issues/13). Measures how Topos
generators respond to controlled, I/O-preserving perturbations of reference
code. The output is data + observations to inform whether the score
normalization constants in
[`topos.evaluation.policies`](../../src/topos/evaluation/policies/) need regularizing.

## What gets measured

The three evaluation pillars are exercised independently where possible:

- **SIMPLE** (`topos.evaluation.policies.simple`)
  - `cfg.cyclomatic` and related CFG probes drive code-complexity quality.
  - `ast.entropy` → bell-curve peak at `ENTROPY_IDEAL = 0.5`.
  - Scored from intrinsic AST/CFG/PDG/CPG construction; a single file is enough.
- **COMPOSABLE** (`topos.evaluation.policies.coupling`)
  - `mdg.coupling` → linear fall to 0 at `MAX_COUPLING = 35`.
  - `mdg.instability` → flat-top tent over `[INSTABILITY_LOW=0.3, INSTABILITY_HIGH=0.7]`.
  - Requires a `ModuleDependencyGraph` from `.gitnexus/` (multi-module package needed).
- **SECURE** (`topos.evaluation.policies.secure`)
  - Always computed alongside SIMPLE/COMPOSABLE from the CPG; recorded in sweep JSON.

Single-file noise tests the SIMPLE pillar; package-level noise tests COMPOSABLE.

## Layout

```
demos/sensitivity/
├── README.md
├── curate.py                   # PyPI sdists → corpus/, baseline scoring, manifest.json
├── corpus/
│   ├── simple/                 # 3 single-file SIMPLE references (generated)
│   └── composable/             # 2 multi-module slices (generated)
├── noise/
│   ├── simple.py               # 4 single-file transforms
│   ├── composable.py           # 4 package-level transforms
│   └── secure.py               # 2 single-file transforms (vulnerability injection)
├── experiments/
│   ├── run_simple.py           # sweep + score SIMPLE pillar
│   ├── run_composable.py       # sweep + score COMPOSABLE pillar
│   └── run_secure.py           # sweep + score SECURE pillar
└── results/                    # gitignored sweep artifacts
    ├── simple_sweep.json/.md
    ├── composable_sweep.json/.md
    ├── secure_sweep.json/.md
    └── regularization_notes.md
```

Vendored PyPI slices under `corpus/` are **not** checked into git. Run `curate.py`
to download and pin them locally.

## Prerequisites

```bash
uv sync                         # from repo root
npm install -g gitnexus        # required for COMPOSABLE scoring
```

The runners use the current Topos Python API directly. Run them with
`uv run python ...`.

## Workflow

```bash
# 1. Curate the corpus (downloads sdists, scores baselines, pins selections)
uv run python demos/sensitivity/curate.py

# 2. Sweep SIMPLE-pillar noise
uv run python demos/sensitivity/experiments/run_simple.py

# 3. Sweep COMPOSABLE-pillar noise
uv run python demos/sensitivity/experiments/run_composable.py

# 4. Sweep SECURE-pillar noise
uv run python demos/sensitivity/experiments/run_secure.py
```

Each runner writes a JSON artifact and a markdown matrix into `results/`.
Read `results/regularization_notes.md` for the human writeup.

## How to interpret the matrices

Rows are noise intensities (`0` is the unperturbed baseline). Columns are
transform names. Cells report SIMPLE or COMPOSABLE score and `lattice_symbol`.
The JSON artifacts include all three pillar scores (`simple`, `composable`,
`secure`) and per-generator `dimensions` for every cell.

A transform that crashes the smoke test for a given intensity is marked `--`
and excluded from analysis.

The AST drift guardrail uses
[`topos.functors.profunctors.ast.compare.calculate_ast_distance`](../../src/topos/functors/profunctors/ast/compare.py)
alongside each SIMPLE sweep cell: large score moves with tiny edit distances
indicate the metric is over-responsive to surface changes.
