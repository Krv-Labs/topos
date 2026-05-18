# Calibration Experiments (Top-100 Python Libraries)

This document sets up the calibration workflow for validating Topos score thresholds
and priority behavior against real-world code.

## Scope

- Population: top 100 Python libraries listed in
  `benchmarks/calibration/top100_pypi.txt`.
- Unit of analysis: each library repository (default branch).
- Scoring command: `topos_evaluate_project` (or CLI equivalent `topos evaluate <path> -r`).

## Experiment 1 — Threshold calibration

Goal: validate whether the current structural threshold (`0.6`) matches empirical
quality inflection points.

### Procedure

1. Materialize local source checkouts for all libraries in the top-100 list.
2. For each checkout, run project-level Topos evaluation:

   ```bash
   topos evaluate /path/to/library -r --priority secure --json
   ```

3. Store each result as one JSON row with:
   - `package`
   - `structural_score`
   - `coupling_score` (when coupling data is available)
   - `overall_lattice`

4. Plot distributions for structural and coupling scores.
5. Identify elbows/inflection points and compare against `0.6`.

   **Elbow method:** Elbow = argmax of d²P/ds² on the ECDF, estimated via KDE
   with bandwidth 0.05. Run on both the per-file distribution and the per-project
   rolled-up distribution separately — they differ because `combine_dimensions()`
   uses `min()`.

   Note: `--output-json` was a bug in an earlier version of this document; the
   correct flag is `--json`.

### Decision rule

Re-pick threshold(s) when the measured elbow differs by more than `0.05` from
current defaults.

## Experiment 2 — Priority-profile A/B

Goal: verify that ``simple``, ``composable``, and ``secure`` produce
meaningfully different rank ordering.

### Procedure

1. Reuse the same top-100 checkout set.
2. Run each project with all three priorities.
3. Build three ranked lists by structural and coupling outcomes.
4. Measure rank-order divergence (Spearman distance) **per dimension**:
   - Run separate Spearman comparisons for the structural and coupling
     dimensions independently.
   - Because the weight profiles affect both dimensions independently
     (structural: `w_complexity`; coupling: `w_coupling`), analyzing each
     separately prevents coupling noise from masking structural movement and
     vice versa.
   - Libraries where `complexity_quality ≈ entropy_quality` will show no rank
     movement under profile shifts — those are **uninformative**, not evidence
     that profiles are equivalent.

### Decision rule

|ρ₁ − ρ₂| < 0.05 across priority pairs is considered **negligible**.
If rank-order changes are negligible across all profiles for a given dimension,
consider collapsing profiles for that dimension.

## Usage-based classification scheme

A deterministic labeling scheme is provided in
`benchmarks/calibration/usage_profiles.csv`.

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

## Hypotheses

Three falsifiable hypotheses connecting usage labels to expected Topos metric
behavior. Each experiment below specifies which hypothesis it tests.

**H₁ (score distribution):** Libraries labeled `composable` will have lower
`mdg.instability` than libraries labeled `self_contained` (Wilcoxon
rank-sum, α=0.05). Rationale: composable libraries are designed to be imported
without large transitive dependency trees, so their internal modules should
have lower instability scores.

**H₂ (threshold alignment):** For composable-labeled libraries, the coupling
score distribution should cluster near 0.6, not sit uniformly below it. If
>90% fail COMPOSABLE, either the threshold is too high or instability does not
capture the construct. A uniform failure distribution is evidence of threshold
miscalibration; a distribution with mass near 0.55–0.65 is evidence of a tight
threshold that could be adjusted.

**H₃ (priority sensitivity by group):** `Δcoupling = score(priority=composable)
− score(priority=simple)` should be larger for composable-labeled
packages than self-contained ones — weight shifts matter most when sub-metrics
are near the decision boundary. If Δcoupling is uniformly near zero, the two
sub-metrics (coupling_quality and instability_quality) are correlated and
profile differentiation is ineffective.

## Experiment 3 — PyPI evidence audit

Goal: cross-validate manual usage labels against automated signals from PyPI
metadata **before running any scoring**. This guards against labeling errors
that would invalidate downstream hypothesis tests.

### Procedure

1. Run `python benchmarks/calibration/scripts/collect_pypi_evidence.py`
2. Inspect disagreements printed to stdout
3. For each disagreement, review the `pypi_classifiers` and
   `has_framework_classifier` fields in the output
4. Update `usage_profiles.csv` rationale for any reclassified entries

### Decision rule

Entries where `signal_confidence=high` and `signal_classification` disagrees
with the manual label should be reviewed by a human before proceeding to
Experiments 4 and 5. Entries where `signal_confidence=low` are inconclusive —
leave the manual label unchanged.

### Output

`benchmarks/calibration/evidence/pypi_evidence.jsonl`

## Experiment 4 — Structural score baseline (no GitNexus required)

Goal: validate the structural threshold 0.6 against real PyPI packages without
coupling data. This experiment can be run on any machine without npm or GitNexus
installed.

### Procedure

1. Run `python benchmarks/calibration/scripts/run_structural_baseline.py`
   - Downloads and extracts each package from PyPI
   - Runs `topos evaluate -r --json --priority secure` on the primary source
     directory
   - Writes per-file results to
     `benchmarks/calibration/results/structural_scores.jsonl`
2. Run `python benchmarks/calibration/scripts/analyze_scores.py` to:
   - Print per-package mean/median/stdev structural score
   - Sweep thresholds [0.40, 0.50, 0.55, 0.60, 0.65, 0.70] and show pass rates
   - If `usage_profiles.csv` is present, stratify score distributions by label

### Decision rule

If fewer than 40% of files from composable-labeled libraries score ≥ 0.6 on
structural, the threshold may be too strict for real-world library code. Many
legitimate libraries have complex core files — for example, `toolz`'s
`functoolz.py` scores 34.9 on raw complexity. A high structural failure rate
across composable-labeled packages should prompt threshold re-evaluation via the
elbow method (see Experiment 1), not automatic re-labeling.

## Experiment 5 — Score-label alignment test (tests H₁–H₃)

Goal: test the three hypotheses after collecting coupling scores. Requires
GitNexus (`npm install -g gitnexus`).

### Procedure

1. For each package, run coupling evaluation:
   ```bash
   gitnexus analyze --force --skip-agents-md   # in the package root
   topos evaluate <src_dir> -r --json --priority secure --gitnexus-dir .gitnexus
   ```
2. For each file with both structural and coupling scores, compute:
   `Δcoupling = score(priority=composable) − score(priority=simple)`
3. Group packages by `usage_classification` from `usage_profiles.csv`.
4. Run Wilcoxon rank-sum on `mdg.instability` grouped by label (tests H₁).
5. Plot coupling score ECDF for composable vs self_contained groups (tests H₂).
6. Compare `|Δcoupling|` distributions by label (tests H₃).

### Decision rule

If H₁ and H₂ both fail — instability does not separate groups and the coupling
ECDF sits uniformly below 0.6 — the coupling metric is not capturing usage
intent. In that case, consider alternative metrics such as fan-out-to-fan-in
ratio or dependency depth. Do not adjust the threshold before investigating
whether the metric itself is miscalibrated.

## Output artifacts

- Inputs:
  - `benchmarks/calibration/top100_pypi.txt`
  - `benchmarks/calibration/usage_profiles.csv`
- Generated by scripts:
  - `benchmarks/calibration/evidence/pypi_evidence.jsonl`  (Experiment 3)
  - `benchmarks/calibration/results/structural_scores.jsonl`  (Experiment 4)
  - `benchmarks/calibration/results/priority_ab.csv`  (Experiment 2)
  - `benchmarks/calibration/results/score_label_alignment.json`  (Experiment 5)

## Multilanguage calibration (Rust, C++, JavaScript, TypeScript)

Structural scores are **not directly comparable** across languages: parsers,
UAST kind histograms, and file-selection rules differ. Use these baselines to
study **within-language** distributions and to tune thresholds per ecosystem,
not to rank Python against Rust or C++.

### Cohort lists

| Ecosystem | Cohort file | Default JSONL output |
|-----------|-------------|------------------------|
| Rust (crates.io) | `benchmarks/calibration/top100_crates_io.txt` | `benchmarks/calibration/results/structural_scores_rust.jsonl` |
| npm (JavaScript) | `benchmarks/calibration/top100_npm_js.txt` | `benchmarks/calibration/results/structural_scores_npm_js.jsonl` |
| npm (TypeScript) | `benchmarks/calibration/top100_npm_ts.txt` | `benchmarks/calibration/results/structural_scores_npm_ts.jsonl` |
| C++ (vcpkg ports) | `benchmarks/calibration/top100_vcpkg_ports.txt` | `benchmarks/calibration/results/structural_scores_vcpkg.jsonl` |

Refresh snapshots (requires network, except vcpkg port verification):

- `python benchmarks/calibration/scripts/refresh_top100_crates_io.py`
- `python benchmarks/calibration/scripts/refresh_top100_npm_cohorts.py`
- `python benchmarks/calibration/scripts/refresh_top100_vcpkg_ports.py`

### Experiment 4 — structural baseline (multilanguage)

Prerequisites:

- `topos evaluate` with `--language` (`python`, `rust`, `javascript`, `typescript`, `cpp`).
- For vcpkg: a local [vcpkg](https://github.com/microsoft/vcpkg) clone on disk, `VCPKG_ROOT` set, and the `vcpkg` executable on `PATH`.

Run the baseline runner:

```bash
python benchmarks/calibration/scripts/run_structural_baseline_i18n.py --ecosystem rust
python benchmarks/calibration/scripts/run_structural_baseline_i18n.py --ecosystem npm_js --limit 10
python benchmarks/calibration/scripts/run_structural_baseline_i18n.py --ecosystem npm_ts
python benchmarks/calibration/scripts/run_structural_baseline_i18n.py --ecosystem vcpkg --vcpkg-root "$VCPKG_ROOT"
```

Each JSONL line includes `package`, `version`, `ecosystem`, `language`, plus the usual per-file fields from `topos evaluate`.

### Experiment 3 — registry evidence (lighter audit)

- Rust: `python benchmarks/calibration/scripts/collect_crates_evidence.py`
- npm: `python benchmarks/calibration/scripts/collect_npm_evidence.py --cohort benchmarks/calibration/top100_npm_ts.txt`
- vcpkg: `python benchmarks/calibration/scripts/collect_vcpkg_evidence.py --vcpkg-root "$VCPKG_ROOT"`

Artifacts: `evidence/crates_evidence.jsonl`, `evidence/npm_evidence.jsonl`, `evidence/vcpkg_evidence.jsonl`.

### Analysis

Point `analyze_scores.py` at the ecosystem JSONL and matching evidence file. Optional filters match `ecosystem` / `language` columns on score rows:

```bash
python benchmarks/calibration/scripts/analyze_scores.py \
  --scores benchmarks/calibration/results/structural_scores_rust.jsonl \
  --evidence benchmarks/calibration/evidence/crates_evidence.jsonl \
  --ecosystem rust --language rust \
  --output benchmarks/calibration/results/score_analysis_rust.json
```

## Notes

- This is experiment setup and classification scaffolding only.
- Collected scores/plots should be appended after running the full benchmark.
- Run Experiment 3 first to validate labels, then Experiment 4 (no GitNexus
  required), then Experiment 2 and 5 (GitNexus required).

## Relationship to sensitivity benchmark

`demos/sensitivity/` measures how scores *respond to controlled perturbations*
of known-baseline files (toolz, funcy, humanize, tabulate). That benchmark
validates **metric sensitivity** — given a file that scores near the boundary,
do small structural changes produce the expected score movement? It answers:
"does the metric move in the right direction?"

The calibration experiments here measure **distribution fit** — whether
real-world packages of known usage type land in the expected regions of the
score space. They answer: "is the threshold placed correctly in the natural
distribution of scores?"

Both are needed. Sensitivity alone cannot tell you whether the threshold is
correctly placed in the natural distribution — a highly sensitive metric can
still have a miscalibrated threshold if the empirical distribution is bimodal
with both modes on the same side. Calibration alone cannot tell you whether
the metric can detect small improvements — a well-placed threshold on a
low-sensitivity metric gives accurate classification on average but fails to
guide incremental refactoring.
## Calibration Results (v1.0.0 Threshold Alignment)

Based on the execution of Experiment 4 (Structural score baseline) against the Python PyPI corpus (requests, numpy, pandas), we found a misalignment between the **raw policy thresholds** (which define the generator logic) and the **normalized score floors** in base.py (which are used by some aggregator tools).

### Findings

The raw metric distributions perfectly match their theoretical bounds on the Python corpus:
- **SIMPLE**: Median cyclomatic complexity is 12, with 55.2% of files falling under the <=15 threshold. Median max function complexity is 5, with 74% under the <=10 threshold.
- **SECURE**: Any file with dangerous_calls > 0 or taint_flows > 0 strictly fails the achieved boolean, as designed.

However, the THRESHOLDS mapping in base.py was acting as an incorrect secondary gate. These thresholds are designed to be "score-floor helpers" for pre-aggregated normalized scores.
- **SIMPLE**: A file sitting exactly on the boundary of passing all raw thresholds (cyclomatic=15, max_func=10, entropy=0.2) mathematically evaluates to a normalized score of exactly 0.40. The old base.py floor of 0.60 incorrectly rejected these perfectly valid files. 
- **COMPOSABLE**: The fan_in / fan_out cap of 15/40 yields a minimum passing normalized score of 0.625. The base.py floor of 0.60 is correctly aligned with this.
- **SECURE**: Because the SECURE generator uses an exponential decay function, a single dangerous API call or taint flow results in a normalized score of approximately 0.71. The old base.py floor of 0.70 would mistakenly allow a file with 1 dangerous call to pass the fallback check.

### Actions Taken

We updated topos/evaluation/policies/base.py to correctly align the fallback THRESHOLDS with the mathematical minimum normalized scores implied by the raw generator bounds:

* **SIMPLE**: 0.60 -> 0.40
* **SECURE**: 0.70 -> 1.00
* **COMPOSABLE**: 0.60 (unchanged)

This change ensures that tools utilizing the meet_satisfied aggregation path maintain strict parity with the raw metric judgments performed during a live structural evaluation. With the SIMPLE threshold lowered to 0.40, the overall file pass rate in the sample corpus appropriately rose to 51.2%.
