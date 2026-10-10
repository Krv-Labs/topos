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

### 5. Copy the prior snapshot

Derive the table in `$LB` with `topos-leaderboard-build-priors` (Krv-Labs/topos-leaderboard#17).
It reads `leaderboard/data/raw/structural_scores_*.jsonl`, drops rows scored by
binaries older than `--min-topos-version` (default `0.5.0`) so a partial rerun
cannot mix readings, and writes the scorer-facing snapshot plus a provenance
sidecar (`advisory_priors.provenance.json`: sources, row counts, per-language
`n`, generation time, leaderboard SHA). Copy only the snapshot:

```bash
cd "$LB" && uv run topos-leaderboard-build-priors
cp "$LB/leaderboard/data/advisory_priors.json" \
  "$TOPOS/topos/engine/src/evaluation/advisory_priors.json"
```

Extra corpora can be pooled with `--evaluate-json GROUP=FILE` (repeatable;
`topos evaluate -r --json` output). `--check` exits non-zero when the
snapshot is stale.

The snapshot vendored with v0.10.0 pooled the v0.5.0 leaderboard rows with 11
`topos evaluate --json` runs (clap, click, cobra, ehrapy, got, httpx, pulsar,
requests, ripgrep, zod, and Topos itself), which supplied the only `mdg.*`
readings. The rerun replaces it from leaderboard rows alone, now that
GitNexus records `mdg.*` for every ecosystem; expect small shifts in `k`
(on the order of ±0.1) and in a few medians.

The file the engine embeds has `schema`, `provisional`, and `metrics`
(`k`, `flag_percentile`, and one line per language: `median` plus the CDF
`points`). Leave provenance (source paths, row counts, the generator name)
on the leaderboard artifact. The crate does not download this file at build
time; `include_str!` bakes in whatever is committed.

### 6. Review `k` and the medians

Each language is one line, so a diff of the CDF points is not readable.
Review the scalars. Current snapshot:

| metric | k | medians |
| --- | ---: | --- |
| `cfg.cyclomatic` | 3.881 | _all 5, cpp 13, go 12, javascript 2, python 10, rust 6, typescript 4 |
| `cfg.essential` | 3.015 | _all 2, cpp 5, go 4, javascript 2, python 2, rust 1, typescript 2 |
| `cfg.nesting_depth` | 3.835 | _all 1, cpp 1, go 2, javascript 0, python 0.5, rust 0, typescript 0 |
| `mdg.fan_in` | 7.377 | _all 0, python 0, rust 1, typescript 0 |
| `mdg.instability` | 9.395 | _all 0.6, python 0.5, rust 0.5, typescript 0.75 |

`mdg.fan_in` and `mdg.instability` are provisional (Python, Rust, TypeScript
only) until this rerun fills C++ and Go. After the copy, check:

- those two metrics gain C++ and Go tables;
- `k` stays near the values above (SIMPLE metrics about 3–5). A jump of an
  order of magnitude means one package is dominating a language;
- medians move plausibly. The previous corpus put max-function-complexity p90
  near C++ 22, PyPI 19, Cargo 8, npm 5.

### 7. Commit the snapshot

Name the leaderboard commit the table came from:

```bash
git add topos/engine/src/evaluation/advisory_priors.json
git commit -m "scoring: refresh advisory prior snapshot (topos-leaderboard@<SHA>)"
```

Rebuild and run the Rust tests. The table is embedded at compile time, so a
stale binary will not pick up the new file. Raw metrics in the JSONL do not
depend on the priors, but the pillar scores stored there were computed with
the previous table. If `k` or the medians moved materially, re-evaluate (a
new version stamp makes resume rescore every package; `--fresh` per ecosystem
also works) before building the site.

### 8. Rebuild the site data

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

### 9. Preview

```bash
cd leaderboard/web && npm install && npm run dev   # http://localhost:8080
```

Check `/calibration.html` in particular: no file with a passing verdict should
show a pillar score below 50.

### 10. Publish

```bash
cd $LB
uv run python deploy/gcp/push_data_to_bucket.py --bucket topos-leaderboard-data
gcloud run jobs execute topos-leaderboard-publish --region=us-central1 --wait
```

### 11. Rewrite the calibration report

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
