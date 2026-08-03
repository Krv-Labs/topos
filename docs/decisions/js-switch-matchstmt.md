# JS/TS `switch_statement` → `MatchStmt` (intentional delta from 0.3.12)

Status: **DOCUMENTED** (v0.4.4). Records an intentional UAST mapping
change introduced in the Rust port, so cross-version histogram
comparisons against Python 0.3.12 are not mistaken for regressions.

## Context

In Python Topos 0.3.12, the JavaScript UAST mapper had no
`switch_statement` entry. It carried a copy-pasted `match_statement`
key (a Python-grammar node that never appears in JS/TS treesitter
output). A real JS/TS `switch` therefore fell through to UAST kind
`Unknown`.

The Rust mapper in [`mapper_javascript.rs`](../../topos/engine/src/graphs/uast/mapper_javascript.rs)
maps:

```text
switch_statement → MatchStmt
```

TypeScript inherits the same table. This matches the UAST schema note
in [`uast-industry-standards.md`](uast-industry-standards.md): JS/C++
`switch` is normalized as `MatchStmt` (optionally flavored via
attributes when needed).

## Decision

**Keep the Rust mapping.** Emitting `MatchStmt` for JS/TS `switch` is
correct for CFG construction and AST complexity (case arms contribute
branching). Do not restore the Python 0.3.12 `Unknown` behavior for
parity.

## Consequences

- Node-kind histograms / structural comparisons between Rust Topos and
  Python 0.3.12 diverge on any `.js`/`.ts` file that contains a
  `switch`: Python reported `Unknown`; Rust reports `MatchStmt`.
- That divergence is expected and intentional — not a silent bug in
  the Rust port.
- Downstream probes that already treat `MatchStmt` as multi-way
  branching (CFG, complexity) correctly score JS/TS switches under
  Rust Topos.

## Related

- Issue #213 (document intentional delta from 0.3.12)
- Review note from #159 (Rust v0.4.0 migration)
