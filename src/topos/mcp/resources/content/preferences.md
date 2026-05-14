# User Preferences — Strict Total Order on G_qual

`priority` upweights one generator inside the policy translators `Φᵢ`.
`preferences` is the stronger statement: a **full strict total order**
on the three generators that linearizes the 8-element lattice
`Ω = H(G_qual)`.

```
ranking = [g₁, g₂, g₃]    g₁ ≻ g₂ ≻ g₃     {g₁, g₂, g₃} = {SIMPLE, COMPOSABLE, SECURE}
```

## Induced order on Ω

Each verdict `v ∈ Ω` is scored by its satisfied-generator bitmask
weighted in preference order:

```
score(v) = 4·⟦g₁ satisfied⟧ + 2·⟦g₂ satisfied⟧ + 1·⟦g₃ satisfied⟧
```

So with ranking `[SIMPLE, COMPOSABLE, SECURE]`:

| Verdict             | Score | Role                                  |
|---------------------|-------|---------------------------------------|
| **IDEAL**           | 7     | ← aspirational target (try first)     |
| **SIMPLE_COMPOSABLE** | 6   | ← fallback (divert if IDEAL plateaus) |
| SIMPLE_SECURE       | 5     |                                       |
| SIMPLE              | 4     |                                       |
| COMPOSABLE_SECURE   | 3     |                                       |
| COMPOSABLE          | 2     |                                       |
| SECURE              | 1     |                                       |
| SLOP                | 0     |                                       |

This refines Ω's Heyting partial order: `a ≤_H b ⟹ a ⪯_r b`. Where the
Heyting order leaves atoms incomparable, the preference order
disambiguates.

## Two-stage targeting: aim for IDEAL, divert to the ideal intersection

The agent's strategy is **two-stage**:

1. **Aim for `IDEAL`.** First try to beat the policy thresholds for
   *all three* generators. Some files genuinely make it.
2. **Divert to the `fallback_target`.** When IDEAL plateaus (a few
   iterations without lattice movement), drop the lowest-ranked
   generator and aim for the meet of the top-two — what we call the
   **"ideal intersection"**.

| Ranking (top → bottom)             | Aspirational | Fallback (ideal intersection) |
|------------------------------------|--------------|-------------------------------|
| SIMPLE ≻ COMPOSABLE ≻ SECURE       | IDEAL        | `SIMPLE_COMPOSABLE`           |
| SECURE ≻ SIMPLE ≻ COMPOSABLE       | IDEAL        | `SIMPLE_SECURE`               |
| COMPOSABLE ≻ SECURE ≻ SIMPLE       | IDEAL        | `COMPOSABLE_SECURE`           |
| …                                  | …            | …                             |

Override the aspirational target via `preferences.target` if the caller
knows up front that IDEAL is out of reach for the file.

## The targeted relaxation walk

Given a current verdict, the **relaxation walk** is the descending
preference-ordered list of verdicts from the aspirational target down
to (but not including) the current verdict. The walk's **second**
element is always the `fallback_target` — the natural divert-point
when IDEAL stalls.

```
ranking = [SIMPLE, COMPOSABLE, SECURE]   current = SECURE
target  = IDEAL                          fallback = SIMPLE_COMPOSABLE
walk    = [IDEAL, SIMPLE_COMPOSABLE, SIMPLE_SECURE, SIMPLE, COMPOSABLE_SECURE, COMPOSABLE]
next_step = COMPOSABLE
```

The `next_step` field is the *smallest* improvement that still respects
the preference order — the safest immediate goal.

## How to use it

Pass `preferences` to any evaluate or assess tool:

```json
{
  "filepath": "src/server.py",
  "preferences": {
    "ranking": ["composable", "secure", "simple"]
  }
}
```

The result includes a `preference_walk` field with:

- `target` — aspirational (default: `IDEAL`)
- `fallback_target` — the ideal intersection (top-2 meet)
- `walk` — descending sequence from aspirational target down
- `next_step` — the immediate next goal
- `progress` — fractional progress to IDEAL in `[0.0, 1.0]`

### Agent strategy

```
iteration 1..N:    aim for `target` (IDEAL)
if plateaued:      aim for `fallback_target` (top-2 meet by preference)
if still stuck:    follow `next_step` down through atoms
```

## Preferences vs. Priority

- `priority` (`balanced` / `simple` / `composable` / `secure`) — knob on
  the scorers `Φᵢ`. Changes how raw metrics combine into per-generator
  scores. **Does not** linearize Ω.
- `preferences.ranking` — strict total order. Induces a total order on
  Ω and decides which pairwise meet is the divert-point when IDEAL is
  unreachable.

Use them together: `priority` tells the scorer how to weight metrics
within a generator; `preferences` tells the agent which lattice
neighbor to aim for next.
