# Calibration Experiments (Top-100 Python Libraries)

This document sets up the calibration workflow for validating Topos score thresholds
and priority behavior against real-world code.

## Scope

- Population: top 100 Python libraries listed in
  `/home/runner/work/topos/topos/evaluations/calibration/top100_pypi.txt`.
- Unit of analysis: each library repository (default branch).
- Scoring command: `topos_evaluate_project` (or CLI equivalent `topos evaluate <path> -r`).

## Experiment 1 — Threshold calibration

Goal: validate whether the current structural threshold (`0.6`) matches empirical
quality inflection points.

### Procedure

1. Materialize local source checkouts for all libraries in the top-100 list.
2. For each checkout, run project-level Topos evaluation:

   ```bash
   topos evaluate /path/to/library -r --priority balanced --output-json
   ```

3. Store each result as one JSON row with:
   - `package`
   - `structural_score`
   - `coupling_score` (when coupling data is available)
   - `overall_lattice`

4. Plot distributions for structural and coupling scores.
5. Identify elbows/inflection points and compare against `0.6`.

### Decision rule

Re-pick threshold(s) when the measured elbow differs by more than `0.05` from
current defaults.

## Experiment 2 — Priority-profile A/B

Goal: verify that `balanced`, `composable`, and `self_contained` produce
meaningfully different rank ordering.

### Procedure

1. Reuse the same top-100 checkout set.
2. Run each project with all three priorities.
3. Build three ranked lists by structural and coupling outcomes.
4. Measure rank-order divergence (e.g., Spearman distance).

### Decision rule

If rank-order changes are negligible across profiles, collapse profiles.

## Usage-based classification scheme

A deterministic labeling scheme is provided in
`/home/runner/work/topos/topos/evaluations/calibration/usage_profiles.csv`.

### Labels

- `composable`: package is primarily consumed as reusable API surface.
- `self_contained`: package is primarily used as an app/runtime/orchestrator
  leaf in downstream systems.

### Assignment rules

Use package usage intent (not implementation details) and assign:

1. **Library/API-first** (`sdk`, `client`, `orm`, `parser`, `types`, `util`) → `composable`
2. **Framework/runtime/app-first** (`server`, `worker`, `scheduler`, `cli`, `notebook`) → `self_contained`
3. **Dual-use packages**:
   - If external import surface is primary, use `composable`
   - If deployment/runtime orchestration is primary, use `self_contained`

When uncertain, record `classification_confidence` and a short rationale.

## Output artifacts

- Inputs:
  - `/home/runner/work/topos/topos/evaluations/calibration/top100_pypi.txt`
  - `/home/runner/work/topos/topos/evaluations/calibration/usage_profiles.csv`
- Suggested outputs (generated during experiment run):
  - `evaluations/calibration/results/top100_scores.jsonl`
  - `evaluations/calibration/results/priority_ab.csv`
  - `evaluations/calibration/results/plots/*.png`

## Notes

- This is experiment setup and classification scaffolding only.
- Collected scores/plots should be appended after running the full benchmark.
