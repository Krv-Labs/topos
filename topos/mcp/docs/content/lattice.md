# The Topos Evaluation Lattice

Topos does not reduce code quality to a single score. Code is classified
in the **free Heyting algebra** `H(G_qual)` on four quality generators

```
G_qual = { SIMPLE, COMPOSABLE, SECURE, NAVIGABLE }
```

Every program morphism `P` has a unique characteristic morphism `χ_S : P → Ω`
whose value records which quality generators the program satisfies. `Ω` has
`2^4 = 16` elements — one per subset of `G_qual`.

## The Medal Podium

Topos maps every file to a **Code Quality Medal**, banded by how many
pillars it satisfies. Compete to get Platinum on every file:

| Pillars satisfied | Symbol | Medal |
|---|---|---|
| 4 of 4 | 🏆 | 🏆 **PLATINUM** |
| 3 of 4 | 🥇 | 🥇 **GOLD** |
| 2 of 4 | 🥈 | 🥈 **SILVER** |
| 1 of 4 | 🥉 | 🥉 **BRONZE** |
| 0 of 4 | ❌ | ❌ **NO MEDAL** |

The 16 verdicts, by medal tier:

| Medal | Verdicts |
|---|---|
| 🏆 **PLATINUM** | `IDEAL` |
| 🥇 **GOLD** | `SIMPLE_COMPOSABLE_SECURE`, `SIMPLE_COMPOSABLE_NAVIGABLE`, `SIMPLE_SECURE_NAVIGABLE`, `COMPOSABLE_SECURE_NAVIGABLE` |
| 🥈 **SILVER** | `SIMPLE_COMPOSABLE`, `SIMPLE_SECURE`, `SIMPLE_NAVIGABLE`, `COMPOSABLE_SECURE`, `COMPOSABLE_NAVIGABLE`, `SECURE_NAVIGABLE` |
| 🥉 **BRONZE** | `SIMPLE`, `COMPOSABLE`, `SECURE`, `NAVIGABLE` |
| ❌ **NO MEDAL** | `SLOP` (fails every generator, or a parse failure) |

`IDEAL` is the top element `⊤` — all four generators. `SLOP` is the bottom
`⊥`. The four single-generator verdicts are **pairwise incomparable**:
neither `SIMPLE ≤ COMPOSABLE` nor the reverse. This is intuitionistic
logic — partial evidence across orthogonal axes.

> **Changed in v0.5.0.** `IDEAL` now means all *four* pillars. The verdict
> that used to be called `IDEAL` — the top of the three-generator algebra —
> is now `SIMPLE_COMPOSABLE_SECURE`, and it bands as GOLD rather than the
> top tier. CI pinned to `IDEAL` will need updating.

## Where each generator comes from

| Generator    | Translational functor (Representation) | Probes |
|--------------|----------------------------------------|--------|
| `SIMPLE`     | Control Flow Graph (CFG) + AST         | `ast.entropy`, `ast.max_function_complexity` gates; `cfg.cyclomatic`, `cfg.nesting_depth`, `cfg.essential` relative advisories |
| `COMPOSABLE` | Module Dependency Graph (GitNexus)     | `mdg.fan_out` gate; `mdg.fan_in`, `mdg.instability` relative advisories; `mdg.main_sequence_distance` diagnostic |
| `SECURE`     | Code Property Graph (CPG)              | `cpg.dangerous_calls`, `cpg.taint_flows` |
| `NAVIGABLE`  | AST scope tree                         | `nav.max_function_divergence` |

The AST and UAST are substrate representations — every other graph is
derived from them.

`SIMPLE` and `NAVIGABLE` need nothing but the file itself, so they are
always evaluated. `COMPOSABLE` needs a dependency graph and `SECURE` needs
a CPG; either is reported as *not measured* rather than *failed* when its
input is unavailable.

## SIMPLE vs NAVIGABLE

These are deliberately orthogonal, and the distinction matters when you're
deciding what to change:

- **SIMPLE** counts *branches*. A function with ten sequential `if`s has
  high cyclomatic complexity.
- **NAVIGABLE** measures *nesting*. That same flat function scores `0.0` —
  it is maximally navigable. A function with the same branch count folded
  four levels deep scores badly.

Nesting is what predicts LLM failure once code length is controlled for:
each level is another hierarchical state a reader has to hold open. A file
can therefore fail NAVIGABLE while passing SIMPLE, and the fix — extract
the nested block into a top-level helper — is different from the fix for a
SIMPLE failure.

## Reading an evaluation result

A `ClassificationResult` has:
- `lattice_element` — the overall verdict (one of the 16 above).
- `dimensions` — per-generator verdict keyed by `simple` / `composable` /
  `secure` / `navigable`.
- `scores` — continuous [0, 100] score per generator, **gate-anchored**:
  50 is exactly at the gate, so a generator is satisfied ⇔ its score ≥ 50.
  Above 50 it measures margin past the gates; codebase-relative advisories
  move it within its half, never across 50.
- `gate_scores` — the gate-only score `G` per generator (min over that
  generator's gates), before advisories.
- `advisories` — per advisory metric: `value`, `relative_percentile`
  (vs. this codebase, shrunk to a language prior), `global_percentile`
  (vs. the language prior), `local_weight`, `quality`, `flagged`.
- `coupling_available` — `false` when no usable `.gitnexus/` graph is
  attached. `COMPOSABLE` (and any verdict that includes it, including
  `IDEAL`) is **unreachable** when this is false. `topos_evaluate_file`/
  `topos_evaluate_project` generate/refresh `.gitnexus` automatically
  when it's missing or stale, so this is normally `true`; it stays
  `false` only when GitNexus isn't installed or generation itself
  failed — `warnings` explains which. Pass `no_composable: true` to skip
  that detection/generation and force SIMPLE/SECURE/NAVIGABLE-only
  scoring.

## Multi-file rollup

Combining per-file verdicts is the **lattice meet** `⋀_f χ_S(f)` —
pointwise per generator.  A generator is satisfied for the whole codebase
iff it is satisfied for every file.

## Continuous reading

The score vector lives in `[0, 1]^4`, a product of Gödel chains (min, max,
Gödel implication). The 16 verdicts are its cut at 0.5. The cut preserves
meet and join, so the continuous score of "all of these pillars" is the
**min** of their scores (PLATINUM's is the min of all four), and the score of
"at least `j` pillars" is the `j`-th largest. Implication and negation do not
survive the cut and are evaluated on the crisp verdicts only.

## Agent loop

Treat the lattice as the **goal**, and the per-generator scores as the
**gradient**. Move toward **🏆 PLATINUM** by fixing whichever pillar's
gates are failing. See `topos://docs/workflows` for the canonical refactor
loop.
