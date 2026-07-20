# The Topos Evaluation Lattice

Topos does not reduce code quality to a single score. Code is classified
in the **free Heyting algebra** `H(G_qual)` on three quality generators

```
G_qual = { SIMPLE, COMPOSABLE, SECURE }
```

Every program morphism `P` has a unique characteristic morphism `χ_S : P → Ω`
whose value records which quality generators the program satisfies.

## The Medal Podium (3-cube)

Topos maps every file to a **Code Quality Medal**. Compete to get Gold on every file:

```
                          🥇 GOLD  (IDEAL)
                         /    |    \
                  🥈 SILVER   🥈 SILVER   🥈 SILVER
                    |  \  /             \  /  |
                    |   \/               \/   |
                    |   /\               /\   |
                    |  /  \             /  \  |
                  🥉 BRONZE   🥉 BRONZE   🥉 BRONZE
                         \    |    /
                          \   |   /
                           \  |  /
                            ❌ SLOP (No Medal)
```

| Value | Symbol | Medal | Meaning |
|---|---|---|---|
| **IDEAL** | 🥇 | 🥇 **GOLD** | All three generators satisfied; joint satisfaction. |
| **SIMPLE_COMPOSABLE** | 🥈 | 🥈 **SILVER** | Both SIMPLE and COMPOSABLE satisfied. |
| **SIMPLE_SECURE** | 🥈 | 🥈 **SILVER** | Both SIMPLE and SECURE satisfied. |
| **COMPOSABLE_SECURE** | 🥈 | 🥈 **SILVER** | Both COMPOSABLE and SECURE satisfied. |
| **SIMPLE** | 🥉 | 🥉 **BRONZE** | Only SIMPLE satisfied (low CFG cyclomatic complexity). |
| **COMPOSABLE** | 🥉 | 🥉 **BRONZE** | Only COMPOSABLE satisfied (good coupling/instability). |
| **SECURE** | 🥉 | 🥉 **BRONZE** | Only SECURE satisfied (no dangerous APIs / taint flows). |
| **SLOP** | ❌ | ❌ **NONE** | Fails every generator (or parse failure). |

The three single-generator verdicts are **pairwise incomparable**: neither
SIMPLE ≤ COMPOSABLE nor the reverse. This is intuitionistic logic — partial
evidence across orthogonal axes.

## Where each generator comes from

| Generator    | Translational functor (Representation) | Probes |
|--------------|----------------------------------------|--------|
| `SIMPLE`     | Control Flow Graph (CFG)               | `cfg.cyclomatic`, `cfg.essential`, `cfg.nesting_depth` |
| `COMPOSABLE` | Module Dependency Graph (GitNexus)     | `mdg.coupling`, `mdg.instability`, `mdg.fan_in/out` |
| `SECURE`     | Code Property Graph (CPG)              | `cpg.dangerous_calls`, `cpg.taint_flows` |

The AST and UAST are substrate representations — every other graph is
derived from them.

## Reading an evaluation result

A `ClassificationResult` has:
- `lattice_element` — the overall verdict (one of the 8 above).
- `dimensions` — per-generator verdict keyed by `simple` / `composable` / `secure`.
- `scores` — continuous [0, 100] score per generator. Threshold: 60%.
- `coupling_available` — `false` when no `.gitnexus/` was found.
  `COMPOSABLE` (and any verdict that includes it, including `IDEAL`) is
  **unreachable** when this is false.

## Multi-file rollup

Combining per-file verdicts is the **lattice meet** `⋀_f χ_S(f)` —
pointwise per generator.  A generator is satisfied for the whole codebase
iff it is satisfied for every file.

## Agent loop

Treat the lattice as the **goal**, and the per-generator scores as the
**gradient**.  Move toward **🥇 GOLD** by improving whichever generator is below
60%.  See `topos://docs/workflows` for the canonical refactor loop.
