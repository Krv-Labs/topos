# Instability below its resolution limit — and the reporting band-aid above it

Status: **SHIPPED** (v0.5.0). The resolution-limit fix landed in
[PR #312](https://github.com/Krv-Labs/topos/pull/312); the follow-up described
under "Remaining work" landed separately — as an **advisory demotion** rather
than the exemption originally proposed here, for the reasons recorded below.
The structural fix (gating COMPOSABLE at module granularity) is deferred to a
later release: see
[`composable-at-module-granularity.md`](composable-at-module-granularity.md).

## Context — how this surfaced

Dogfooding v0.5.0 on Topos itself (`topos/cli`, `topos/engine`, `topos/mcp`;
176 files), the repo scored **GOLD**, held back by COMPOSABLE alone. 98 of 176
files failed the pillar — and **81 of those 98 failed on `mdg.instability`
only**. Nothing about how those files were written could change that.

Pass/fail against measured module coupling (`Ca + Ce`) made the shape obvious:

| `mdg.coupling` | pass | fail |
| --- | --- | --- |
| 0 | 45 | 0 |
| **1** | **1** | **36** |
| 2 | 8 | 13 |
| 3 | 10 | 11 |
| 4+ | 14 | 38 |

A module with **zero** imports passed vacuously. A module with **one** import
failed automatically. That discontinuity is not a property of the code being
measured.

## The defect

Martin's Instability is `I = Ce / (Ca + Ce)` — a ratio whose resolution is
`1 / (Ca + Ce)`. The attainable readings at a measured total of `n` are
`{k/n : k = 0..n}`:

| `Ca + Ce` | attainable `I` | intersects band `[0.3, 0.7]`? |
| --- | --- | --- |
| 0 | — (no signal) | n/a |
| **1** | `{0.0, 1.0}` | **no** |
| 2 | `{0.0, 0.5, 1.0}` | yes |
| 3 | `{0.0, 0.33, 0.67, 1.0}` | yes |

At a single coupling edge, `I` is decided entirely by that one edge's
*direction*. It carries no information about balance — the quantity `I` exists
to express. The calibrated band was therefore **unreachable by construction**
for every single-edge module, and `Φ_COMPOSABLE` failed all of them.

`instability_from_coupling` already returned the `0.5` no-signal midpoint at
zero coupling. The bug is that the rule stopped one edge short of where the
measurement actually becomes readable.

## Decision

Extend the existing no-signal rule to the ratio's resolution limit, in
`functors/probes/mdg/coupling.rs`:

```rust
const MIN_RESOLVABLE_COUPLING: usize = 2;

fn instability_from_coupling(result: &CouplingResult) -> f64 {
    if result.total() < MIN_RESOLVABLE_COUPLING {
        0.5
    } else {
        result.efferent as f64 / result.total() as f64
    }
}
```

**Placed in the probe, not the policy.** This is a statement about what the
*measurement* can resolve, not about where a threshold should sit. Probes in
this crate never import `evaluation::policies` (verified: zero `use
crate::evaluation` in `functors/`), and that separation holds — `calibration`
owns the numbers, `gates` owns the structure, and the probe owns what it is
physically able to report.

### Effect, measured

Exactly the `coupling = 1` tier moved — 36 files, nothing above it:

| | before | after |
| --- | --- | --- |
| COMPOSABLE failures | 98 | 62 |
| PLATINUM files | 72 | **106** |
| GOLD files | 98 | 66 |
| SILVER files | 6 | 4 |
| self-badge | GOLD | 🏆 PLATINUM |
| composite | 71.2 | 75.7 |

COMPOSABLE does not become vacuous: 62 files still fail, including 13 of 21 at
`coupling = 2` and 14 of 16 at `coupling = 4`. Every gate above the resolution
limit behaves exactly as before; `mdg.fan_in` / `mdg.fan_out` are untouched.

### Why not refactor the source instead

There was no honest source-level remedy. Moving a `coupling = 1` file into the
band means *adding* imports to a leaf module — worse code by every measure
Topos otherwise rewards. Across all 98 failures, only 2 files had a real
refactor available (`mdg.fan_in` over cap with instability in band); the other
96 were gated behind either the unreachable band or a fan cap they also
breached. Recalibrating the band itself was rejected too: the band is fine, it
was being applied to readings that cannot express it.

---

## Remaining work — retiring `leaf_composable_zeros`

`ProjectEvaluationResult::leaf_composable_zeros` is a **reporting-layer
band-aid on this same defect**, and it should be removed rather than kept.

### What it does today

`topos/mcp/src/tools/evaluate.rs`:

```rust
fn is_leaf_composable_zero(result: &ClassificationResult) -> bool {
    if !result.is_parseable { return false; }
    if result.raw_metrics.get("mdg.instability").copied() != Some(0.0) { return false; }
    let fan_in = result.raw_metrics.get("mdg.fan_in").copied();
    let fan_out = result.raw_metrics.get("mdg.fan_out").copied();
    !has_coupling_signal(fan_in, fan_out)
}
```

It threads through five call sites (`is_hard_fail`, `is_maintainability_giant`,
`hard_fail_sort_key`, `classify_project_rows`, `project_file_sort_key`) and
surfaces as a public MCP schema field.

### Why it is the wrong shape

1. **Wrong layer.** It re-classifies a verdict the scorer already got wrong.
   `Φ_COMPOSABLE` still returns `SLOP`, the file still shows a GOLD medal
   instead of PLATINUM, and the mean composable score is still dragged down.
   The band-aid only hides the file from one list.
2. **Wrong surface.** It exists solely in `topos_evaluate_project`. The CLI,
   `docs/badge.svg`, the leaderboard, and `topos_assess_*` all see the
   uncorrected verdict — so two surfaces disagree about the same file.
3. **Wrong predicate, in the wrong graph.** It tests `mdg.fan_in`/`mdg.fan_out`
   — symbol-level `CALLS` edges — to decide whether *module-level* `IMPORTS`
   coupling is trustworthy. Those are different graphs. It is also asymmetric:
   it keys on exact float equality with `0.0` and never covers the pure-efferent
   (`I = 1.0`) single-edge leaf at all.
4. **Duplicates a role that already exists.** `evaluation::file_roles::
   is_stable_leaf_module` is a filename-agnostic UAST predicate for exactly this
   file shape (declarations only: no `CallExpr`, `FunctionDecl`, or control
   flow), and `gates::distance_stable_leaf_exempt` already encodes Martin's
   Zone-of-Pain carve-out for it. The band-aid is a second, weaker
   implementation of a carve-out the gate layer already models.

### The concrete case it is hiding

After PR #312, the bucket collapsed from 3 files to 1 — and that one file
proves the point:

```
topos/engine/src/graphs/base.rs
  abstractness = 1.0   instability = 0.0   coupling = 12
  fan_in = 0           fan_out = 0
```

`base.rs` is a pure trait-definition module. `A = 1.0`, `I = 0.0` puts it
**exactly on the main sequence** (`D = |A + I − 1| = 0.0`) — Martin's ideal
position for a stable abstraction. It fails COMPOSABLE.

It fails because `composable::coupling_gate_input` refuses to enter distance
mode:

```rust
let has_coupling_signal = !(fan_in == Some(0.0) && fan_out == Some(0.0));
```

A pure trait module has no calls, so its symbol-level fan is legitimately zero —
but it has **12 module-level import edges**. The coupling signal is
unambiguously present; the guard is looking in the wrong graph. With distance
mode suppressed, raw `I = 0.0` gates and fails low, and
`distance_stable_leaf_exempt` never gets a chance to fire because the gate that
carries it is inactive.

The stated intent in that function's own doc comment is about
`calculate_coupling`'s module-level no-signal fallback — so this is a
mis-implementation of the documented intent, not a deliberate choice.

### What shipped instead

The three-step fix originally drafted here (fix the coupling-signal guard, add a
low-side stable-leaf exemption to `mdg.instability`, then delete the band-aid)
was superseded once the grid analysis below made the scope of the problem clear.
Step 2 in particular was the wrong instinct: it would have *added* a special case
to rescue a gate that should not have been gating in the first place.

What landed:

1. **Test the coupling signal in the graph it comes from.** `coupling_gate_input`
   now takes `mdg.coupling` (`Ca + Ce`) and enters distance mode on
   `coupling >= MIN_RESOLVABLE_COUPLING` rather than on symbol-level fan — one
   constant shared with the probe, so measurement and policy cannot disagree
   about what is resolvable. `base.rs` went from `SLOP` / score 0.0 to
   `COMPOSABLE` / score 100.0, lattice `IDEAL`.

2. **`mdg.instability` and `mdg.main_sequence_distance` became advisory**
   (`gates_achieved: false`) instead of gaining an exemption. The band's
   reachability swings with `n` rather than with design quality — 33% of the
   attainable grid lands in band at `n = 2`, 50% at `n = 3`, 20% at `n = 4` — and
   measured over Topos's 176 files the pass rate tracked that grid density. Both
   metrics are still scored, interpreted, and offered as refactor targets at
   severity `"improve"`; they simply cannot hard-fail a file for the arithmetic
   of its own denominator. `mdg.fan_in` / `mdg.fan_out` are absolute counts with
   no resolution limit and keep gating COMPOSABLE.

   The precedent is `cfg.cyclomatic` (issue #193), advisory for the structurally
   identical reason: a whole-file reading that scales with something other than
   the quality it claims to measure.

3. **The band-aid is retired.** `is_leaf_composable_zero` and its five call sites
   are deleted; `leaf_composable_zeros` is marked `**Deprecated**` and always
   empty, kept one release for wire compatibility per the `worst_files`
   precedent. With instability advisory there is no composable hard-fail left to
   suppress.

### Effect of the advisory demotion, measured

| | post-#312 | advisory |
| --- | --- | --- |
| COMPOSABLE failures | 62 | **17** |
| PLATINUM files | 106 | 148 |
| GOLD / SILVER | 66 / 4 | 27 / 1 |
| mean composable score | 63.2 | 64.1 |
| composite | 75.7 | 75.9 |

The 17 survivors are all genuine fan-cap breaches and read as a hand-checkable
list of real coupling smells — `schemas.rs` (fan-in 74), `dispatch.rs` (51),
`install/testing.rs` (60), `tools/evaluate.rs` (fan-out 39), `tools/assess.rs`
(34). At 17/176 COMPOSABLE remains the *most* discriminating of the four pillars
(SIMPLE fails 11, NAVIGABLE 1, SECURE 0), so it has not gone vacuous.

**The guard against this being score inflation:** `ScoredDecision.score` is
`min(qualities)` over *all* gates including advisory ones, while `achieved`
filters on `gates_achieved`. So the verdict changes and the reported score does
not — the composable mean moved only 63.2 → 64.1, and that 0.9 is the `base.rs`
class of correction, not gate loosening. The badge says "no hard coupling
failures"; the score still says "instability readings are poor."

### The honest caveat

This is the second change in a row that improves Topos's own badge, and PLATINUM
now rests mainly on the fan caps plus SIMPLE. The defense is that the 17-file
list is independently checkable and the score curve is untouched — but the real
resolution is structural, not another threshold decision, and it is deferred:
see [`composable-at-module-granularity.md`](composable-at-module-granularity.md).

## Testing (PR #312)

- `cargo test --workspace` — 637 passed, 0 failed.
- `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -D warnings` — clean.
- `python scripts/generate_self_badge.py --self-check` — ok.
- Two existing tests asserted the old behavior on single-edge fixtures
  (`instability_all_efferent`, `metrics_reports_coupling_instability_fan_and_depth`).
  Both were re-fixtured to two edges so their original claims — "pure efferent",
  "all efferent" — stay meaningful, and `instability_single_edge_is_unresolvable`
  pins the new case.

A cross-layer test asserting "the calibrated band contains no `k/1`" was
considered and skipped: it holds for any band excluding both `0.0` and `1.0`,
so it could only fail in a configuration where the gate is already disabled.
