# File-level COMPOSABLE as outward dependency burden

Status: **ACCEPTED for v0.5.0**. This is the file-scope policy. Package
stability remains a possible later, separately scoped measure; see
[`composable-at-module-granularity.md`](composable-at-module-granularity.md).

## Decision

At file scope, COMPOSABLE answers one narrow question:

> How much external behavior must this file coordinate in order to do its job?

`mdg.fan_out` counts distinct external symbols called by symbols contained in
the file. It is the only metric that gates the file-level COMPOSABLE verdict:

```text
COMPOSABLE achieved ⇔ mdg.fan_out ≤ 10
```

All dependency readings remain visible in `inspect`. The thresholded advisory
readings also remain in suggestions/refactor targets and continue to influence
the continuous COMPOSABLE score:

| metric | file-scope interpretation | verdict role |
| --- | --- | --- |
| `mdg.fan_out` | outward interaction/dependency burden | **gating** |
| `mdg.fan_in` | responsibility and change-impact radius | advisory |
| `mdg.coupling` | total import-graph degree (`Ca + Ce`) | diagnostic |
| `mdg.dep_depth` | transitive import reach | diagnostic |
| `mdg.instability` | dependency role/direction | advisory |
| `mdg.abstractness` | package-design context projected onto the file | diagnostic input |
| `mdg.main_sequence_distance` | stability/abstractness balance | advisory |

High fan-in alone is not a failure. A stable interface, schema, or shared
utility can correctly have many callers. High fan-in is still important during
impact analysis, so removing it from the hard verdict does not remove it from
inspection or refactoring surfaces.

## Construct validity and literature

This policy does **not** claim that a source file is a Robert Martin release
package. Martin instability, Abstractness, and Distance from the Main Sequence
remain useful context, but their package-level interpretation does not justify
a hard file-level band.

The narrower dependency-burden claim has program-unit precedent:

- Parnas frames modularization around comprehensibility, flexibility,
  replaceability, and information hiding; a physical file is an operational
  component boundary, not automatically a release package. [Parnas,
  1972](https://doi.org/10.1145/361598.361623).
- Chidamber and Kemerer's class-level CBO/RFC measures treat coupling and the
  reachable response set as properties of individual program units. Topos's
  external-callee count is closest to an outward interaction subset of that
  family. [Chidamber & Kemerer,
  1994](https://doi.org/10.1109/32.295895).
- Class-level coupling metrics have shown value as fault-proneness and change-
  impact indicators, but those studies support risk ranking rather than a
  universal numeric threshold. [Basili, Briand & Melo,
  1996](https://doi.org/10.1109/32.544352); [Briand, Wüst & Lounis,
  1999](https://doi.org/10.1109/ICSM.1999.792645).
- Dependency-graph degree and reach measures can identify risky or central
  program units beyond local code-complexity measures. [Zimmermann & Nagappan,
  2008](https://doi.org/10.1145/1368088.1368161).
- Henry and Kafura introduced information-flow fan-in/fan-out at procedure and
  module scope, but their composite is `length × (fan-in × fan-out)²`; it is
  not the formula formerly stated in `fan.rs`, and later measurement work
  questioned its multiplication and exponent. Topos uses the separate raw
  directions instead. [Henry & Kafura,
  1981](https://doi.org/10.1109/TSE.1981.231113); [Briand, Morasca & Basili,
  1996](https://www.cs.umd.edu/users/basili/publications/journals/J58.pdf).

These sources justify measuring local coupling and outward interaction. They do
not supply the threshold `10`; that number is an empirical Topos policy.

## Gate calibration

The v0.5.0 candidate binary was run over fresh leaderboard samples from Python
(PyPI), Rust (Cargo), TypeScript packages, and the polyglot MCP-server cohort.
Tests/examples were removed before selecting the gate because the leaderboard
policy intends to score production source. The resulting population was 2,979
files:

| cohort | production files | `fan_out > 10` | failure rate |
| --- | ---: | ---: | ---: |
| Python | 1,063 | 13 | 1.2% |
| Rust | 404 | 12 | 3.0% |
| TypeScript | 585 | 37 | 6.3% |
| MCP (polyglot) | 927 | 63 | 6.8% |

With equal ecosystem weight, the gate fails 4.3% of files. A cap of `8` failed
6.1%; `10` was selected as the conservative point closest to the roughly 5%
tail policy used by other Topos structural gates. The language spread is
published rather than hidden: the same count does not have identical incidence
across languages.

The calibration is evidence for a release policy, not validation of external
quality. A future corpus should link the reading to defects, change propagation,
or review outcomes and should stratify by file size and generated/test role.

## Consequences

- A file can achieve COMPOSABLE while carrying a poor advisory instability,
  distance, or fan-in score. `achieved` means no hard outward-burden breach;
  the continuous score and inspection surfaces retain the richer diagnosis.
- Entrypoints and orchestrators may legitimately fail. The verdict describes
  outward coordination burden, not moral judgment; inspect/refactor must show
  the actual callees so the user can distinguish intentional orchestration from
  accidental dependency accumulation.
- Package-level stability must be introduced as a separate scoped result. It
  must not be inherited into every member file or multiplied through the file
  medal aggregate.
