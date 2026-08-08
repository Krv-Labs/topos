# COMPOSABLE at module granularity — strategy

Status: **DEFERRED** past v0.5.0. No code written for the module work itself.
This document sets direction and phasing for moving the COMPOSABLE *verdict*
from files to modules while keeping the *metrics* per file.

v0.5.0 ships the stopgap instead: `mdg.instability` and
`mdg.main_sequence_distance` are **advisory** (`gates_achieved: false`), so
COMPOSABLE gates on the fan caps only. That concedes the problem rather than
solving it — file-level instability is still measured and reported, it just no
longer decides a verdict it cannot decide fairly. Everything below remains the
plan for making it decidable. Companion:
[`composable-instability-resolution.md`](composable-instability-resolution.md).

## Thesis

> Per-file composability **metrics**. Module-level composability **verdicts**.

Coupling is not a property of a file in isolation. Martin defines `Ca`, `Ce`,
`I`, and `A` for *packages* — units of release. Topos computes and gates them
per file, and that is a category error, not a calibration miss. The metrics
remain useful per file for attribution; the pass/fail judgment belongs one level
up, where the quantities are defined and the denominators are large enough to
mean something.

## The evidence

At file granularity the instability gate substantially measures the arithmetic
of its own denominator. For a module with `n = Ca + Ce` edges, `I` can only take
values `k/n`; the fraction of that grid inside the calibrated band `[0.3, 0.7]`
predicts the measured pass rate (Topos's own 176 files, post-#312):

| `n` | grid values in band | grid density | measured pass | files |
| --- | --- | --- | --- | --- |
| 2 | `1/2` | 33% | 38% | 21 |
| 3 | `1/3, 2/3` | 50% | 48% | 21 |
| 4 | `2/4` | 20% | 12% | 16 |
| 8 | `3/8, 4/8, 5/8` | 33% | 29% | 7 |

58 of 176 files sit at `n ∈ {2,3,4}`. The gate is also **non-monotonic** there:
a file at `n=3` faces a 50% grid; add one import and `n=4` cuts it to 20%, with
no change in design quality. #312 removed the `n=1` singularity; `n=2..4` remains
a lottery, and no amount of recalibrating a band can fix a grid that coarse.

## Design

### 1. Per-file metrics stay exactly as they are

`raw_metrics` continues to carry `mdg.coupling`, `mdg.instability`,
`mdg.fan_in`, `mdg.fan_out`, `mdg.dep_depth`, `mdg.abstractness` per file. They
are reported, compared (`profunctors/mdg/compare.rs`), and — critically — used
to **attribute** a module-level failure to specific files. When a module fails
`mdg.fan_out`, the answer to "which file do I edit?" is the module's members
ranked by their own fan-out. This is where per-file metrics earn their keep, and
it is the reason not to simply discard them.

### 2. All COMPOSABLE gates evaluate at module scope

One rule, no two-tier story: **per-file `mdg.*` metrics gate nothing.** The
`GateSpec`s for `mdg.instability`, `mdg.main_sequence_distance`, `mdg.fan_in`,
and `mdg.fan_out` are evaluated once per module, against module-level readings.

A file's `dimensions["composable"]` is **inherited** from its module's verdict.
It is not independently computed and not independently overridable.

### 3. Module coupling is recomputed, never summed

This is the technical heart. Module `Ca`/`Ce` must count edges **crossing the
module boundary**, with intra-module edges excluded and the far end deduplicated
by *module*, not by file. Summing member files' `Ca`/`Ce` would double-count
internal edges and inflate the denominator with exactly the edges that should
disappear.

`calculate_coupling` already implements precisely this pattern one level down —
it walks a file's contained symbols, resolves each edge's far end to its owning
*file*, drops self-edges, and dedupes into a `HashSet`. The module version is
the same function with the equivalence class widened from "file" to "module":
generalize `owning_file` to `owning_module` and make the grouping a parameter.
This should be a small change, not a new subsystem.

### 4. Module identity: coarsen until measurable

Granularity is the load-bearing choice. Measured over Topos's own tree:

| unit | modules | files/module (min / median / max) | single-file modules |
| --- | --- | --- | --- |
| crate | 3 | 32 / 39 / 105 | 0 |
| `src/` subdir | 12 | 1 / 11 / 38 | 2 (17%) |
| directory | 41 | 1 / 3 / 16 | 5 (12%) |

**Directory is too fine** — 5 single-file "modules" would reintroduce the exact
`n=1` pathology #312 just removed, one level up. **Crate is too coarse** — three
verdicts for 176 files gives agents nothing to act on. The right unit is not a
fixed depth; it is the result of a rule:

1. Start at the file's directory.
2. Walk up while the candidate module has fewer than `MIN_MODULE_FILES` members
   **or** its boundary-crossing coupling is below `MIN_RESOLVABLE_COUPLING`.
3. Never merge across a language package root — nearest `Cargo.toml` for Rust,
   the `__init__.py` chain root for Python, `package.json`/`tsconfig.json` for
   TS/JS. A crate boundary is a hard stop.

This generalizes `MIN_RESOLVABLE_COUPLING` from "abstain when unmeasurable" to
"regroup until measurable," which is the better move where regrouping is
available.

**Do not depend on GitNexus `Folder`/`Package` nodes.** `NODE_LABELS` is
documented in `graphs/mdg/models.rs` as non-exhaustive and not runtime-enforced
— "GitNexus can emit labels outside this list." Path-derived identity is
deterministic and works without them. If those nodes are present and reliable,
use them as a *refinement* to the walk-up rule, never as a precondition.

### 5. Exemptions and band-aids this deletes

Several existing carve-outs exist **only** because file-level coupling is
unrepresentative. At module granularity they stop having anything to describe:

- `instability_entrypoint_exempt` — a `lib.rs` re-export hub reads `I = 1.0`
  because, as a file, it only imports. As one member of its module it simply
  contributes its edges to the module total. The exemption becomes unnecessary.
- `distance_stable_leaf_exempt` / `is_stable_leaf_module` (for COMPOSABLE) — a
  declarations-only constants file is a member, not a module. Martin's Zone of
  Pain is a claim about *packages*; at package scope the existing gate already
  says the right thing. (`is_stable_leaf_module` may still earn its keep
  elsewhere; this only retires its COMPOSABLE use.)
- ~~`is_leaf_composable_zero` / `ProjectEvaluationResult::leaf_composable_zeros`~~
  — already retired by the v0.5.0 stopgap. The schema field is deprecated and
  always empty; delete it in the release after.
- ~~The `has_coupling_signal` symbol-fan guard in `coupling_gate_input`~~ —
  already fixed in v0.5.0 to read module coupling. At module granularity the
  guard stays, but its input becomes the module's own `Ca + Ce`.

That is four special cases removed by one structural correction — two of them
already gone via the stopgap. Treat this as the primary argument for the change,
not a side benefit: the special cases are the symptom that the granularity is
wrong.

## Surface contracts

| Surface | Today | After |
| --- | --- | --- |
| `topos evaluate <dir>` | per-file COMPOSABLE | per-module verdict; per-file inherits; `mdg.*` still per file |
| `topos evaluate <file>` | per-file COMPOSABLE | COMPOSABLE **abstains** unless the module is in scope |
| `topos_evaluate_project` | per-file rows | rows unchanged + a module rollup section |
| `topos_evaluate_file` | per-file COMPOSABLE | abstains, with a reason, unless module context is loaded |
| `topos_refactor` / `topos_assess_*` | file-level COMPOSABLE targets | module-level target + ranked member files |
| `docs/badge.svg`, leaderboard | mode over file medals | unchanged mechanically; see caveat below |

Single-file abstention needs **no new vocabulary**: the fail-open
`coupling_available: false` path — "COMPOSABLE not scored, here's why", never an
error — is already the documented contract for this exact situation
(`composable-by-default.md`). Reuse it verbatim.

### Per-file medals

A file's medal continues to exist and can still be PLATINUM; its COMPOSABLE bit
is inherited. Two structurally identical files in different modules can receive
different medals. **This is correct and is the point** — composability is a
property of a file's position in a dependency structure, not of its text.

*Alternative considered:* grade the lattice by scope — Ω restricted to file
scope becomes the 3-generator cube (SIMPLE, SECURE, NAVIGABLE), with the full
4-cube existing only at module scope and above. This is arguably the more
honest category statement and it is cleaner mathematically. Rejected for now
because it removes per-file PLATINUM entirely, which breaks the agent contract's
target vocabulary, the badge, and the leaderboard simultaneously. Worth
revisiting if the inheritance model produces confusing agent behavior.

## Phasing

**Phase 0 — stopgap. SHIPPED in v0.5.0.** `mdg.instability` and
`mdg.main_sequence_distance` demoted to advisory; `coupling_gate_input` now
reads module coupling rather than symbol fan; the `leaf_composable_zeros`
band-aid retired. Measured effect: COMPOSABLE failures 62 → 17, all of them
genuine fan-cap breaches.

The low-side stable-leaf exemption originally planned for Phase 0 was
**deliberately not added** — it would have introduced a special case to rescue a
gate that should not have been gating. Demoting the gate removed the need for it
instead. Phase 2 should preserve that instinct: prefer deleting a carve-out to
adding one.

When Phase 2 lands, the advisory flags flip back to `gates_achieved: true` at
module scope — the demotion is a statement about *file* granularity, not about
the metric.

**Phase 1 — measure, change nothing.** Implement module identity and module
coupling; emit `mdg.module.*` alongside the existing per-file metrics; gate
nothing on them. Then answer, with numbers, the questions this document cannot:
what does the `n` distribution look like at module granularity? Does the grid
problem actually disappear? How many modules does a typical repo get? This
mirrors the `composable-incremental-spike.md` precedent — investigate, publish
the numbers, decide after.

**Phase 2 — move the gate.** Per-file COMPOSABLE inherits the module verdict.
Delete the four special cases listed above. Update CLI/MCP contracts and the
`leaf_composable_zeros` schema field (with a deprecation window, per the
`worst_files` precedent in `ProjectEvaluationResult`).

**Phase 3 — recalibrate.** The `[0.3, 0.7]` band, `max_fan_in`/`max_fan_out`,
and `main_sequence_distance_max` were fit against **file-level** readings over
4,254 files. Module-level distributions will differ, probably a lot. Moving
granularity invalidates the calibration — do not assume the band survives the
move. Re-fit over the same corpus at the new granularity before shipping.

## Risks and open questions

- **Calibration is invalidated, not merely shifted.** Phase 3 is not optional
  polish; without it Phase 2 ships a gate tuned for the wrong distribution.
- **Agent verify-loop cost.** Agents edit file-by-file and re-check. A module
  verdict flips only when the module's aggregate moves, so an agent may make
  three good edits and see no change. Mitigation: per-file `mdg.*` metrics move
  immediately and should be surfaced as the progress signal, with the module
  verdict as the goal. `topos_assess_*` runs on every edit-verify cycle and needs
  this to feel responsive.
- **Cross-repo comparability.** Module count depends on repo layout, so the
  leaderboard compares repos partitioned differently. Publishing module count
  and median module size alongside the score is the minimum honest disclosure.
- **Second breaking COMPOSABLE change in one release line.** #312 already
  changed COMPOSABLE scoring in v0.5.0. This would be a second, larger one.
  Pre-1.0 permits it; the changelog should be explicit rather than quiet.
- **Does `dep_depth` belong at module scope too?** Not analyzed here.
- **Monorepos with a single flat source directory** collapse to one module under
  the walk-up rule. Needs a tested fixture before Phase 2.

## Measured vs. assumed

**Measured** (Topos's own tree, 176 files, `topos evaluate --json` post-#312):
the grid-density table, the 58 files at `n ∈ {2,3,4}`, and the module-count
table at three granularities.

**Assumed, not measured** — module-level `Ca`/`Ce` distributions, the resulting
`n` values, and therefore whether module granularity actually dissolves the grid
problem. That is exactly what Phase 1 exists to establish, and no part of Phase
2 should be committed to before those numbers exist.
