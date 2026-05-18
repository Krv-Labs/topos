# Structural test coverage — evaluation notes

This document accompanies `run_evaluation.py` and the UAST structural test coverage profunctor in `topos/functors/profunctors/uast/structural_test_coverage.py`.

It records what the prototype measures well, where it is noisy, and what to iterate next.

## What we ran

1. **Synthetic pair** — A small list-summing PUT compared to (a) a one-line assert test and (b) a test that mirrors loops and branches. Expect higher kind, control-flow, k-gram, and declaration-level recall for (b). This checks that the metric moves with meaningful structural change rather than staying flat.

2. **binarytrees-style** — `demos/binarytrees/src/binarytrees.py` as PUT vs a thin smoke test vs a slightly richer Python test with a `for` / `if` / `return` skeleton. Deltas between the two test bodies show sensitivity on a larger, more realistic PUT.

Run locally:

```bash
uv run python demos/structural_test_coverage/run_evaluation.py
```

## Stability vs noise

- **Stable:** Kind and control-flow recalls respond monotonically when the test suite adds nodes whose UAST kinds overlap the PUT (see unit tests in `tests/test_structural_test_coverage.py`).
- **Noisy / inflated:** Tests heavy in `CallExpr` (frameworks, mocks) can raise histogram overlap without exercising PUT logic. No call-graph linkage is applied in v0/v1.
- **k-grams:** Path recall uses DFS pre-order kind sequences per file; k-grams do not span file boundaries. Changing traversal order would change k-grams; the metric is intentionally tied to the current DFS convention shared with `uast_edit_distance`.
- **Declaration coverage:** v2 matches PUT declarations against test declarations, reports uncovered declaration locations, separates statement and expression recall, and includes a precision signal so unrelated test code does not only inflate recall.

## Failure modes (documented limitations)

| Limitation | Effect |
|------------|--------|
| **No test–PUT call linkage** | High recall does not mean tests invoke production functions. |
| **Fixture / setup dominance** | Large `setUp` or helper modules can look structurally “close” to application code. |
| **`Unknown` UAST kinds** | With `--include-unknown`, parser gaps add mass that tests can accidentally match. Default CLI omits unknown kinds for stability (aligned with binarytrees demos). |
| **Size asymmetry** | A tiny PUT and a huge test file can still show high recall if kinds overlap. Diagnostics (`put_kind_nodes`, `test_kind_nodes`) are reported alongside scores. |
| **Structural similarity only** | Declaration coverage can say a test body looks structurally similar to a PUT function; it still cannot prove that the test calls that function. |

## Next iterations

- Optional down-weighting of kinds common in test harnesses.
- Static or dynamic call linkage between tests and PUT for a “reachable structure” variant.
- Alternative path encodings (e.g. leaf paths, branch-local sequences) to reduce DFS-order sensitivity.
