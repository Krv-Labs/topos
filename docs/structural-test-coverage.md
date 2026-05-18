# Structural test coverage

This document describes the **structural test coverage** measure implemented in Topos. It is a prototype metric: it estimates how much of a **program-under-test (PUT)** appears in a **test suite** at the level of normalized **Universal AST (UAST) structure**. It does **not** replace line or branch coverage, and it does **not** establish that tests invoke specific production functions unless you add separate call-linkage analysis.

The implementation lives in `topos/functors/profunctors/uast/structural_test_coverage.py`. The CLI entry point is `topos structural-test-coverage`. Formal definitions also appear in Sphinx as `docs/source/measures.rst`.

---

## Motivation

Traditional coverage counts executed lines or branches. For static or structural tooling, it is useful to ask a different question: *does the test code contain structural shapes (syntax categories, control-flow constructs, short paths through the kind tree) that resemble the PUT?* That signal can complement line coverage: a project might have high line coverage while tests rarely exercise loops, pattern matches, or error paths that exist in production code—or the opposite pattern.

Topos uses an **asymmetric**, declaration-level bipartite matching algorithm to score how well the **tests’** structure **overlaps** the **PUT’s** structure.

---

## Declaration-level Bipartite Coverage

Topos maps coverage at the declaration level using the following algorithm:

1. **Extraction**: `FunctionDecl` and `MethodDecl` nodes are extracted from the PUT and test files.
2. **Fingerprinting**: Each PUT declaration is fingerprinted by its body's kind histogram (excluding the root `FunctionDecl`/`MethodDecl` to avoid vacuous matches).
3. **Matching**: Each PUT declaration finds the test declaration that maximizes kind recall using multiset overlap.

This approach provides several advantages:
- **Bounded Context**: Localizes missing structure to specific functions in the code.
- **Monotonicity**: Adding unrelated tests does not artificially boost coverage by simply dumping kind-mass into a pooled bucket.
- **Precision Penalties**: Topos computes an **F2 score** combining declaration recall with test precision, heavily penalizing bloated test suites that have low precision relative to the PUT.
- **Category Stratification**: Uses disjoint recall checks on `Stmt` and `Expr` kinds to avoid double-counting.

---

## Interpretation

- **Higher Mean Declaration Coverage**: More of the PUT’s specific structural declarations have matching structural declarations in the test suite.
- **Lower scores**: Tests may be missing whole classes of syntax or control-flow that specific PUT declarations use, or the tests are structurally disjoint.
- **F2 Score**: If the F2 score is significantly lower than the Mean Declaration Coverage, it indicates the test suite contains significant "bloat" or structures entirely unrelated to the PUT.

The `DeclarationCoverageReport` dataclass includes diagnostics like precise locations of uncovered declarations (those that fall below the coverage threshold).

---

## API and CLI (reference)

**Python**

```python
from topos.graphs.ast.dispatch import parse_source
from topos.functors.profunctors.uast.structural_test_coverage import declaration_coverage

put = parse_source(source=..., language="python", file="src/m.py").uast_root
tst = parse_source(source=..., language="python", file="tests/test_m.py").uast_root
report = declaration_coverage([put], [tst], k=3, include_unknown=False)
# report.mean_declaration_coverage, report.stmt_recall, report.f2_score
```

**CLI**

```bash
topos structural-test-coverage --tests tests/test_mod.py src/mod.py
topos structural-test-coverage --tests t1.py --tests t2.py --language python --k 3 --json src/a.py src/b.py
```

Options include `--language` (`python`, `rust`, `javascript`, `cpp`), `--k`, `--include-unknown`, `--coverage-threshold`, and `--json`.

---

## Limitations and failure modes

| Topic | Effect |
|--------|--------|
| No call linkage | High recall does not mean tests call production entry points. |
| Framework / mock heavy tests | Shared kinds (especially calls) can inflate overlap without exercising PUT logic. |
| `Unknown` kinds | With `include_unknown=True`, parser gaps can add mass that tests accidentally align with. |
| Symmetric size | A very small PUT and a very large test suite can still show moderate recall if kinds overlap; read diagnostics alongside scores. |
