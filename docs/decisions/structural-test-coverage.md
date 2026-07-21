# Structural test coverage

This document describes the **structural test coverage** measure implemented in Topos. It is a prototype metric: it estimates how much of a **program-under-test (PUT)** appears in a **test suite** at the level of normalized **Universal AST (UAST) structure**. It does **not** replace line or branch coverage, and it does **not** establish that tests invoke specific production functions unless you add separate call-linkage analysis.

The UAST implementation lives in `crates/topos-core/src/functors/profunctors/uast/structural_test_coverage.rs` (Rust, since PR [#159](https://github.com/Krv-Labs/topos/pull/159)'s v0.4.0 migration). The CLI entry point is `topos coverage` (`crates/topos-cli/src/commands/coverage.rs`); the same computation is also exposed as the `topos_calculate_coverage` MCP tool (`crates/topos-mcp/src/tools/coverage.rs`).

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

The `DeclarationCoverageReport` struct includes diagnostics like precise locations of uncovered declarations (those that fall below the coverage threshold).

---

## API and CLI (reference)

**Rust** (`crates/topos-core/src/functors/profunctors/uast/structural_test_coverage.rs`)

```rust
use topos_core::graphs::ast::dispatch::parse_source;
use topos_core::functors::profunctors::uast::structural_test_coverage::declaration_coverage;

let put = parse_source(&put_source, "python", Some("src/m.py"))?.uast_root;
let tst = parse_source(&test_source, "python", Some("tests/test_m.py"))?.uast_root;
let report = declaration_coverage(&[&put], &[&tst], 3, false)?;
// report.mean_declaration_coverage, report.stmt_recall, report.f2_score
```

**CLI** (`crates/topos-cli/src/commands/coverage.rs`)

```bash
topos coverage --tests tests/test_mod.py src/mod.py
topos coverage --tests t1.py --tests t2.py --language python --k 3 src/a.py src/b.py
```

Options: `--language` (any [tree-sitter-supported language](../../crates/topos-core/src/graphs/ast/languages.rs)), `--k` (n-gram length), `--include-unknown`, `--coverage-threshold`. The same computation is also exposed as the `topos_calculate_coverage` MCP tool (`crates/topos-mcp/src/tools/coverage.rs`), which does return structured JSON over the wire — unlike the CLI, which is plain-text only for this pass (issue [#147](https://github.com/Krv-Labs/topos/issues/147)).

---

## Limitations and failure modes

| Topic | Effect |
|--------|--------|
| No call linkage | High recall does not mean tests call production entry points. |
| Framework / mock heavy tests | Shared kinds (especially calls) can inflate overlap without exercising PUT logic. |
| `Unknown` kinds | With `include_unknown=True`, parser gaps can add mass that tests accidentally align with. |
| Symmetric size | A very small PUT and a very large test suite can still show moderate recall if kinds overlap; read diagnostics alongside scores. |
