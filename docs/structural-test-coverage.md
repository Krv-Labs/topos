# Structural test coverage

This document describes the **structural test coverage** measure implemented in Topos for Issue #24. It is a prototype metric: it estimates how much of a **program-under-test (PUT)** appears in a **test suite** at the level of normalized **Universal AST (UAST) `kind`** structure. It does **not** replace line or branch coverage, and it does **not** establish that tests invoke specific production functions unless you add separate call-linkage analysis.

The implementation lives in `src/topos/metrics/uast/structural_test_coverage.py`. The CLI entry point is `topos structural-test-coverage`. Formal definitions also appear in Sphinx as `docs/source/structural_test_coverage.rst`.

---

## Motivation

Traditional coverage counts executed lines or branches. For static or structural tooling, it is useful to ask a different question: *does the test code contain structural shapes (syntax categories, control-flow constructs, short paths through the kind tree) that resemble the PUT?* That signal can complement line coverage: a project might have high line coverage while tests rarely exercise loops, pattern matches, or error paths that exist in production code—or the opposite pattern.

Topos already compares two programs symmetrically via `compare_uast` (histogram distance, edit distance on kind sequences, etc.). Structural test coverage is **asymmetric**: it scores how well the **tests’** structure **overlaps** the **PUT’s** structure, using multiset **recall**-style formulas.

---

## Inputs and preprocessing

1. **Parse** each PUT and test source file with the same pipeline used elsewhere: `topos.graphs.ast.dispatch.parse_source`, yielding a UAST root per file.
2. **Aggregate** multiple PUT files by **summing** per-kind counts (and the same for multiple test files). Multiple test modules are treated as one pooled test corpus for counting purposes.
3. **`include_unknown`**: When `False` (the default on the CLI), nodes mapped to the catch-all `Unknown` kind are **omitted** from histograms and from DFS kind sequences. That reduces noise for grammars with thinner UAST mapping. When `True`, unknown nodes participate like any other kind.

---

## Version 0: Kind recall, control-flow recall, composite

Let \(n_P(k)\) be the count of UAST kind \(k\) in the **merged PUT** histogram, and \(n_T(k)\) the count in the **merged test** histogram (after aggregation across files).

### Kind recall

\[
R_{\text{kind}} = \frac{\sum_k \min\bigl(n_P(k), n_T(k)\bigr)}{\sum_k n_P(k)}
\]

- The sum runs over kinds that appear in the merged histograms; kinds absent from both contribute nothing; kinds only in tests do not increase the numerator (there is no PUT mass to match).
- The score lies in \([0, 1]\). It is **1** when for every kind, test count is at least PUT count (up to floating-point detail). It is **0** when the test corpus contributes no overlapping kind mass (e.g. empty tests with a non-empty PUT).

**Vacuous PUT:** If \(\sum_k n_P(k) = 0\), the denominator is zero. The implementation defines \(R_{\text{kind}} = 1.0\) in that case (vacuous full recall).

### Control-flow recall

The same formula is applied to the **control-flow profile**: counts restricted to the fixed set `CONTROL_FLOW_KINDS` (e.g. `IfStmt`, `ForStmt`, `WhileStmt`, `ReturnStmt`, `CallExpr`, …) as produced by `control_flow_profile` in `src/topos/metrics/uast/signature.py`. Denote the result \(R_{\text{cf}}\). Vacuous empty PUT control-flow mass again yields \(1.0\).

### Composite v0

A single headline scalar for v0 is the unweighted mean:

\[
C_0 = \tfrac{1}{2} R_{\text{kind}} + \tfrac{1}{2} R_{\text{cf}}
\]

The API and CLI still expose \(R_{\text{kind}}\) and \(R_{\text{cf}}\) separately so you can see whether mismatch is “general shape” or “control-flow heavy.”

---

## Version 1: Path recall (k-grams)

Version 1 adds a **path-aware** signal derived from the **DFS pre-order sequence of kinds** (the same traversal order used for UAST edit distance in `compare_uast`).

For each file:

1. Build the list of kinds `uast_dfs_kind_sequence(root, include_unknown=...)`.
2. Slide a window of length \(k\) (default \(k = 3\)) over the sequence to obtain **k-grams**: tuples \((k_1,\ldots,k_k)\) of consecutive kinds.
3. Count k-grams in a multiset (a `Counter`). **k-grams do not span file boundaries**: each file is sequenced independently, then counts are **summed** across files for PUT and separately for tests.

Let \(c_P(g)\) and \(c_T(g)\) be the counts of k-gram \(g\) in PUT and tests (after merging files). **Path recall** is:

\[
R_{\text{path}} = \frac{\sum_g \min\bigl(c_P(g), c_T(g)\bigr)}{\sum_g c_P(g)}
\]

If the PUT produces no k-grams (e.g. sequence shorter than \(k\)), the denominator is zero and \(R_{\text{path}} = 1.0\) (vacuous).

**Note:** Because k-grams are tied to DFS order, different but equivalent AST shapes could yield different k-grams; the metric is intentionally defined on this traversal, not on a canonical graph path abstraction. That tradeoff keeps the implementation simple and aligned with existing sequence-based distance code.

---

## Interpretation

- **Higher** \(R_{\text{kind}}\), \(R_{\text{cf}}\), \(R_{\text{path}}\): more of the PUT’s counted structure also appears somewhere in the test corpus (in the multiset sense).
- **Lower** scores: tests may be missing whole classes of syntax or control-flow that the PUT uses, or the PUT is large and tests are tiny and structurally disjoint.
- **High scores are not sufficient for “good tests”:** shared structure can come from fixtures, harness code, or frameworks (`CallExpr` inflation) without semantic exercise of the PUT. There is **no** static or dynamic call linkage in v0/v1.

The `StructuralTestCoverageReport` dataclass includes diagnostics: total kind nodes in PUT vs tests, control-flow node totals, and k-gram masses. Use these to spot **size asymmetry** (huge test tree vs tiny PUT) or boilerplate dominance.

---

## API and CLI (reference)

**Python**

```python
from topos.graphs.ast.dispatch import parse_source
from topos.metrics.uast import structural_test_coverage

put = parse_source(source=..., language="python", file="src/m.py").uast_root
tst = parse_source(source=..., language="python", file="tests/test_m.py").uast_root
report = structural_test_coverage([put], [tst], k=3, include_unknown=False)
# report.kind_recall, report.control_flow_recall, report.composite_v0, report.path_recall_kgram
```

**CLI**

```bash
topos structural-test-coverage --tests tests/test_mod.py src/mod.py
topos structural-test-coverage --tests t1.py --tests t2.py --language python --k 3 --json src/a.py src/b.py
```

Options include `--language` (`python`, `rust`, `javascript`, `cpp`), `--k`, `--include-unknown`, and `--json`.

---

## Limitations and failure modes

| Topic | Effect |
|--------|--------|
| No call linkage | High recall does not mean tests call production entry points. |
| Framework / mock heavy tests | Shared kinds (especially calls) can inflate overlap without exercising PUT logic. |
| `Unknown` kinds | With `include_unknown=True`, parser gaps can add mass that tests accidentally align with. |
| DFS-specific k-grams | Path recall changes if traversal order changes; it is not a language-independent canonical path set. |
| Symmetric size | A very small PUT and a very large test suite can still show moderate recall if kinds overlap; read diagnostics alongside scores. |

For a short empirical note on stability vs noise and a runnable script, see `demos/structural_test_coverage/EVALUATION.md` and `demos/structural_test_coverage/run_evaluation.py`.

---

## Relation to `compare_uast`

`compare_uast` measures **divergence** between two programs (e.g. L1 distance between **normalized** kind histograms, edit distance on kind sequences). Structural test coverage measures **how much of the PUT multiset is contained in the test multiset** using **raw counts** and \(\min\) overlap. The problems are different; both reuse UAST kinds and shared DFS sequencing where appropriate.
