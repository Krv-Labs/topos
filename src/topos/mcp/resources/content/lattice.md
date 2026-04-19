# The Topos Diamond Lattice

Topos does not reduce code quality to a single score. Code is classified
on a **diamond lattice** with four elements:

```
           ⊤  SOUND         (both targets achieved)
          / \
 COMPOSABLE   SELF_CONTAINED    (incomparable — neither ≤ the other)
          \ /
           ⊥  BROKEN        (neither achieved)
```

## The four values

| Value | Symbol | Meaning |
|---|---|---|
| **BROKEN** | ⊥ | Fails both targets (or parse failure). |
| **COMPOSABLE** | ◑ | Coupling target achieved: good fan-in/out, balanced instability. Composes cleanly with other modules. **Requires a DependencyGraph; unreachable from AST alone.** |
| **SELF_CONTAINED** | ◐ | Structural target achieved: low cyclomatic complexity, balanced entropy. Stands alone cleanly. |
| **SOUND** | ⊤ | Both targets achieved. |

## Why COMPOSABLE and SELF_CONTAINED are incomparable

They measure orthogonal properties. A file can have pristine internal
structure (SELF_CONTAINED) while coupling badly to the rest of the codebase
(not COMPOSABLE), and vice versa. Neither subsumes the other. Their **meet**
(greatest lower bound) is BROKEN; their **join** (least upper bound) is SOUND.

This is a Heyting algebra — the internal logic of a topos. It gives you
intuitionistic reasoning: partial evidence across dimensions, no law of
excluded middle.

## Reading an evaluation result

A `ClassificationResult` has:
- `lattice_element` — the overall verdict (one of the four above).
- `dimensions` — per-axis verdict (`structural`, `coupling`). `coupling`
  is only populated when a `DependencyGraph` was provided.
- `scores` — continuous [0, 100] score per dimension. Threshold: 60%.
- `coupling_available` — `false` if no `.gitnexus/` was found.
  COMPOSABLE/SOUND are **unreachable** when this is false.

## Agent loop

Treat the lattice as the **goal**, and dimension scores as the **gradient**.
Move toward SOUND by improving whichever dimension is below 60%. See
`topos://docs/workflows` for the canonical refactor loop.
