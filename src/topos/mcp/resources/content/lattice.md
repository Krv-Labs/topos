# The Topos Evaluation Lattice

Topos does not reduce code quality to a single score. Code is classified
in the **free Heyting algebra** `H(G_qual)` on three quality generators

```
G_qual = { SIMPLE, COMPOSABLE, SECURE }
```

This is the codomain of the subobject classifier `Ω` in the Topos
`E = Set^(C × H^op)`.  Every program morphism `P` has a unique
characteristic morphism `χ_S : P → Ω` whose value records which
generators the program satisfies.

## The 8 verdicts (3-cube)

Each verdict is a subset of `G_qual`; the lattice has `2³ = 8` elements.

```
                          IDEAL  ⊤   (all generators satisfied)
                         /  |  \
        SIMPLE_COMPOSABLE  SIMPLE_SECURE  COMPOSABLE_SECURE
               |  \  /             \  /  |
               |   \/               \/   |
               |   /\               /\   |
               |  /  \             /  \  |
             SIMPLE   COMPOSABLE        SECURE
                        \    |    /
                         \   |   /
                          \  |  /
                           SLOP  ⊥   (no generator satisfied)
```

| Value | Symbol | Meaning |
|---|---|---|
| **IDEAL** | ⊤ | All three generators satisfied; the ideal program state. |
| **SIMPLE_COMPOSABLE** | ◐◑ | Both SIMPLE and COMPOSABLE satisfied; SECURE not yet. |
| **SIMPLE_SECURE** | ◐◇ | Both SIMPLE and SECURE satisfied; COMPOSABLE not measured / not satisfied. |
| **COMPOSABLE_SECURE** | ◑◇ | Both COMPOSABLE and SECURE satisfied; SIMPLE not satisfied. |
| **SIMPLE** | ◐ | Only SIMPLE satisfied (low CFG cyclomatic complexity). |
| **COMPOSABLE** | ◑ | Only COMPOSABLE satisfied (good coupling/instability). |
| **SECURE** | ◇ | Only SECURE satisfied (no dangerous APIs / taint flows). |
| **SLOP** | ⊥ | Fails every generator (or parse failure). |

The three single-generator verdicts are **pairwise incomparable**: neither
SIMPLE ≤ COMPOSABLE nor the reverse. This is intuitionistic logic — partial
evidence across orthogonal axes, no law of excluded middle.

## Where each generator comes from

| Generator    | Translational functor (Representation) | Probes |
|--------------|----------------------------------------|--------|
| `SIMPLE`     | Control Flow Graph (CFG)               | `cfg.cyclomatic`, `cfg.essential`, `cfg.nesting_depth` |
| `COMPOSABLE` | Module Dependency Graph (GitNexus)     | `mdg.coupling`, `mdg.instability`, `mdg.fan_in/out` |
| `SECURE`     | Code Property Graph (CPG)              | `cpg.dangerous_calls`, `cpg.taint_flows` |

The AST and UAST are substrate representations — every other graph is
derived from them.  AST entropy still folds into the SIMPLE generator as
a secondary signal.

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

## Convention note

The mathematical specification states `IDEAL = g₁ ∧ g₂ ∧ ⋯ ∧ g_n` ("meet of
all generators").  That is the informal "joint satisfaction" reading;
`IDEAL` is the verdict in which all generators are satisfied together.  In
the underlying algebra `IDEAL` is the top `⊤` of `H(G_qual)`, and the
algebraic meet of two incomparable generators (e.g. `meet(SIMPLE,
COMPOSABLE)`) is `SLOP`.

## Agent loop

Treat the lattice as the **goal**, and the per-generator scores as the
**gradient**.  Move toward IDEAL by improving whichever generator is below
60%.  See `topos://docs/workflows` for the canonical refactor loop.
