# Topos Version-Based Library Evaluations

This demo setup evaluates source code from two versions of each popular library:

- `numpy`: `1.26.4` vs `2.4.4`
- `scipy`: `1.11.4` vs `1.17.1`
- `scikit-learn`: `1.4.2` vs `1.8.0`
- `networkx`: `2.8.8` vs `3.6.1`

The runner downloads source archives, extracts them into a local cache, finds the
library import directory (for example `sklearn` for `scikit-learn`), then runs:

```bash
topos evaluate <package_dir> -r --json
```

## Install

From the repository root:

```bash
uv pip install -e .
```

The runner fetches release files from PyPI, so network access is required on first run.

## Run all version evaluations

```bash
uv run python demos/libraries/run_all.py
```

Write a JSON summary artifact:

```bash
uv run python demos/libraries/run_all.py --write-json
uv run python demos/libraries/run_all.py --write-json --json-output demos/libraries/results/version_summaries.json
```

## Run a subset of libraries

```bash
uv run python demos/libraries/run_all.py --library numpy --library scipy
```

## Reuse cached downloads

```bash
uv run python demos/libraries/run_all.py --skip-download
```

This mode fails if the required archives are not already available under
`demos/libraries/.cache/downloads`.

## What the output includes

For each library/version pair, the runner reports:

- `overall` lattice outcome from Topos
- `files` evaluated
- average complexity score
- average entropy score
- count of files per evaluation level

It also prints a delta line between the older and newer version for quick comparison.
