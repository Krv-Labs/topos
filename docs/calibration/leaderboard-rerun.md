# Leaderboard rerun for gate-anchored scoring

Operator guide for the one-time leaderboard rerun that recalibrates the
advisory priors behind [gate-anchored scoring](../decisions/gate-anchored-scoring.md).
The leaderboard lives in the sibling repository `topos-leaderboard`; its
[`README.md`](https://github.com/Krv-Labs/topos-leaderboard/blob/main/README.md)
and `leaderboard/README.md` are the authority on its commands and flags.

Below, `$TOPOS` is this checkout and `$LB` is the `topos-leaderboard` checkout.

## Why a rerun is needed

The advisory priors (`topos/engine/src/evaluation/advisory_priors.json`) are
per-language percentile tables plus a shrinkage constant `k` per metric. They
were first derived from the v0.5.0 leaderboard JSONL, which is stale in three
ways:

1. **NAVIGABLE gate.** v0.5.0 scored NAVIGABLE against gate `6`; the gate is now
   `10`. Raw divergence readings are unaffected, but every stored NAVIGABLE
   verdict and score is wrong.
2. **No COMPOSABLE metrics for C++, Go, or MCP.** Those cohorts were scored
   without `mdg.*` readings, so the COMPOSABLE priors cover only Python, Rust,
   and JavaScript/TypeScript.
3. **COMPOSABLE priors are provisional.** The `mdg.fan_in` and
   `mdg.instability` tables were bootstrapped from a ~1,000-file fresh run, not
   the full corpus.

Scores published from the old JSONL also predate gate-anchored scoring, so the
site's score distributions and the calibration report describe the retired
scheme.

## Prerequisites

- `uv`, Python 3.11+, Node.js (for preview), network access.
- `GITHUB_TOKEN` exported if you refresh the Go or MCP cohorts.
- GitNexus for COMPOSABLE readings (depgraph generation is on by default):

  ```bash
  npm i -g gitnexus
  ```

- A release build of Topos that includes gate-anchored scoring, on `PATH` or in
  `TOPOS_BIN`:

  ```bash
  cd $TOPOS
  cargo build --release
  export TOPOS_BIN=$TOPOS/target/release/topos   # or put it first on PATH
  $TOPOS_BIN --version
  ```

- `gcloud` credentials with access to the `topos-leaderboard-data` bucket for
  the publish step.

## Procedure

### 1. Check the scorer

```bash
cd $LB
uv sync
uv run topos-leaderboard-check-topos
```

Confirm the reported binary path and version are the build from the previous
step, not a stale Homebrew or `binaries/` copy.

### 2. Optional: refresh cohorts

```bash
uv run python benchmarks/scripts/refresh_top100_pypi.py
uv run python benchmarks/scripts/refresh_top100_cargo.py
uv run python benchmarks/scripts/refresh_top100_npm.py
uv run python benchmarks/scripts/refresh_top50_cpp.py
uv run python benchmarks/scripts/refresh_top50_go.py
uv run python benchmarks/scripts/refresh_top50_mcp.py
```

Skip this to keep the corpus comparable to the previous board.

### 3. Smoke test one package per ecosystem

`--package` needs a single `--ecosystem`. Use the first name in each cohort
file (or any package you know is small):

```bash
for eco in pypi:top100_pypi cargo:top100_cargo npm:top100_npm \
           cpp:top50_cpp go:top50_go mcp:top50_mcp; do
  name=${eco%%:*}; list=${eco#*:}
  pkg=$(head -1 benchmarks/$list.txt)
  uv run topos-leaderboard-eval -y --ecosystem $name --package "$pkg" \
    --versions-per-package 1
done
```

Then verify that COMPOSABLE readings are present in every ecosystem, especially
`cpp`, `go`, and `mcp`:

```bash
for f in leaderboard/data/raw/structural_scores_*.jsonl; do
  printf '%s  ' "$f"; grep -c '"mdg.fan_out"' "$f"
done
```

A zero count (or no new rows for the smoke package) means GitNexus is missing
or failed; fix that before the full run. Rows written by the smoke test carry
the new `topos_version`, so the full run resumes over them.

### 4. Full evaluation

```bash
uv run topos-leaderboard-eval -y
```

All six ecosystems run in sequence. Resume is on by default: rows already
stamped with this Topos version and eval fingerprint are skipped, so an
interrupted run can simply be restarted. Output lands in
`$LB/leaderboard/data/raw/structural_scores_<eco>.jsonl`.

### 5. Regenerate the priors

```bash
cd $TOPOS
python3 scripts/derive_scoring_priors.py \
  --raw-dir $LB/leaderboard/data/raw \
  --min-topos-version <this release>
```

`--min-topos-version` drops rows scored by older binaries, so a partially
re-evaluated corpus cannot mix old and new readings. Add
`--evaluate-json <file>` to fold in an additional `topos evaluate` JSON export
(for example a fresh COMPOSABLE run) when the leaderboard corpus is thin for a
language. The script writes `topos/engine/src/evaluation/advisory_priors.json`.

### 6. Review the diff

```bash
git diff --stat topos/engine/src/evaluation/advisory_priors.json
git diff topos/engine/src/evaluation/advisory_priors.json
```

Check:

- every language now has COMPOSABLE tables (`mdg.fan_in`, `mdg.instability`),
  including C++ and Go;
- `k` values stay in the same range as before (SIMPLE metrics about 3–5,
  divergence about 6). A `k` that jumps by an order of magnitude, or becomes
  infinite (no between-package variance), points to a corpus problem such as one
  package dominating a language;
- prior medians and p90s move plausibly (for example max function complexity
  p90 near C++ 22, PyPI 19, Cargo 8, npm 5 on the previous corpus).

### 7. Commit the priors

Record what the table was derived from:

```bash
git add topos/engine/src/evaluation/advisory_priors.json
git commit -m "scoring: regenerate advisory priors

Corpus: topos-leaderboard@<SHA of $LB>, Topos <version>, <file count> files.

Co-Authored-By: ..."
```

### 8. Check the committed priors

```bash
python3 scripts/derive_scoring_priors.py --check
```

`--check` validates the committed `advisory_priors.json` and exits non-zero on
a problem. Run the Rust test suite as well, since the table is embedded at
build time.

Raw metrics do not depend on the priors, but the pillar scores stored in the
JSONL were computed with the table embedded in the binary that produced them.
If the regenerated table differs materially, build Topos with the new table and
re-evaluate (a new version stamp makes resume rescore every package; `--fresh`
per ecosystem also works) before building the site.

### 9. Rebuild the site data

```bash
cd $LB
uv run topos-leaderboard-build
uv run topos-leaderboard-build-calibration
uv run topos-leaderboard-build-calibration --no-balance-ecosystems   # unbalanced view, for comparison
uv run topos-leaderboard-build-graphs
uv run topos-leaderboard-badges
uv run topos-leaderboard-prerender
uv run topos-leaderboard-verify --expected-versions 3
```

### 10. Preview

```bash
cd leaderboard/web && npm install && npm run dev   # http://localhost:8080
```

Check `/calibration.html` in particular: no file with a passing verdict should
show a pillar score below 50.

### 11. Publish

```bash
cd $LB
uv run python deploy/gcp/push_data_to_bucket.py --bucket topos-leaderboard-data
gcloud run jobs execute topos-leaderboard-publish --region=us-central1 --wait
```

### 12. Rewrite the calibration report

`CALIBRATION_REPORT.md` in `topos-leaderboard` is written by hand.
`uv run topos-leaderboard-report` regenerates the HTML evidence it draws on
(`leaderboard/web/calibration-report.html`). Update the Markdown report to:

- remove the score floors, including the 0.80 COMPOSABLE floor and its
  "binary distribution for modularity" justification. That distribution was an
  artifact of the fixed instability band (issue #351), and pass is now exactly
  `score ≥ 50`;
- state the current gates (SIMPLE: max function complexity ≤ 10, entropy in
  `[0.2, 0.8]`; COMPOSABLE: fan-out ≤ 10; SECURE: zero dangerous calls and taint
  flows; NAVIGABLE: max function divergence ≤ 10);
- describe the advisory priors and `k` values with the corpus SHA and Topos
  version from step 7.

## How calibration procedures change

| item | before | after |
| --- | --- | --- |
| gate thresholds | ECDF elbows per release | unchanged; re-derived only when a metric definition changes |
| score normalization | per-metric linear caps (cyclomatic 40, max function 20, fan 40, divergence 12) | fixed by construction: `d = 0.5` at the gate, `0` at twice the gate |
| pass rule | raw gates; separate normalized score floors (0.40 / 0.80 / 1.00 / 0.40) | raw gates, and equivalently `score ≥ 50`; floors retired |
| advisory metrics | fixed caps and bands inside the pillar `min` | percentiles vs. codebase, shrunk to a language prior; table + `k` from one corpus pass |
| COMPOSABLE instability | fixed band `[0.3, 0.7]` | two-sided relative advisory, skipped when `Ca + Ce < 2` |
| main-sequence distance | scored when abstractness > 0 | diagnostic only |
| medals | 4-pillar count | unchanged |
| calibration report | justified floors from score distributions | documents gates, priors, and `k` with corpus provenance |

## When to recompute

Recompute the priors only when:

- a metric definition changes (which also requires re-deriving that metric's
  gate), or
- the leaderboard is rerun with a new cohort or a Topos release that changes raw
  readings.

Do not recompute on a schedule. The priors describe the reference corpus, not the
user's code; refreshing them without a reason only moves every score and breaks
comparison across releases.
