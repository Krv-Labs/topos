# User Preferences — Strict Total Order on G_qual

`priority` emphasizes one generator inside the policy translators `Φᵢ`.
`preferences` is the stronger statement: a **full strict total order**
on the four generators that linearizes the 16-element lattice
`Ω = H(G_qual)`.

```
ranking = [g₁, g₂, g₃, g₄]   g₁ ≻ g₂ ≻ g₃ ≻ g₄
{g₁, g₂, g₃, g₄} = {SIMPLE, COMPOSABLE, SECURE, NAVIGABLE}
```

A ranking must be a **permutation of all four** generators. A three-element
ranking written before v0.5.0 is rejected, and the caller falls back to the
default order.

## Induced order on Ω

Each verdict `v ∈ Ω` is scored by its satisfied-generator bitmask
weighted in preference order:

```
score(v) = 8·⟦g₁ satisfied⟧ + 4·⟦g₂ satisfied⟧ + 2·⟦g₃ satisfied⟧ + 1·⟦g₄ satisfied⟧
```

Weights halve down the ranking, so each generator outranks every
lower-ranked one combined — the order is strictly lexicographic. With the
default ranking `[SIMPLE, NAVIGABLE, SECURE, COMPOSABLE]`:

| Verdict                         | Score | Role                                    |
|---------------------------------|-------|-----------------------------------------|
| **IDEAL**                       | 15    | ← aspirational target (try first)       |
| SIMPLE_SECURE_NAVIGABLE         | 14    | ← concede only the last-ranked pillar   |
| SIMPLE_COMPOSABLE_NAVIGABLE     | 13    |                                         |
| **SIMPLE_NAVIGABLE**            | 12    | ← fallback (divert if IDEAL plateaus)   |
| SIMPLE_COMPOSABLE_SECURE        | 11    |                                         |
| SIMPLE_SECURE                   | 10    |                                         |
| SIMPLE_COMPOSABLE               | 9     |                                         |
| SIMPLE                          | 8     |                                         |
| COMPOSABLE_SECURE_NAVIGABLE     | 7     |                                         |
| SECURE_NAVIGABLE                | 6     |                                         |
| COMPOSABLE_NAVIGABLE            | 5     |                                         |
| NAVIGABLE                       | 4     |                                         |
| COMPOSABLE_SECURE               | 3     |                                         |
| SECURE                          | 2     |                                         |
| COMPOSABLE                      | 1     |                                         |
| SLOP                            | 0     |                                         |

The default ranks the two **agent-cognition** pillars highest — `SIMPLE`
(branch count) and `NAVIGABLE` (nesting depth), both read from the same
UAST with no external dependency, so both are always available and always
fixable inside one file. `SECURE` follows: also local, but zero-tolerance,
so it is a blocker rather than a gradient. `COMPOSABLE` ranks last because
it is the least locally actionable — it needs an external GitNexus graph,
degrades to unavailable without one, and is a property of a module's place
in the whole dependency graph rather than of the file in front of you.
Ranking it last means the walk concedes it first, which is the right thing
to concede when `coupling_available` is `false`.

This refines Ω's Heyting partial order: `a ≤_H b ⟹ a ⪯_r b`. Where the
Heyting order leaves atoms incomparable, the preference order
disambiguates.

## Two-stage targeting: aim for IDEAL, divert to the ideal intersection

The agent's strategy is **two-stage**:

1. **Aim for `IDEAL`.** First try to beat the policy thresholds for
   *all four* generators. Some files genuinely make it.
2. **Divert to the `fallback_target`.** When IDEAL plateaus (a few
   iterations without lattice movement), aim for the meet of the top-two
   ranked generators — what we call the **"ideal intersection"**:
   guarantee what the operator cares most about, concede the rest.

| Ranking (top → bottom)                       | Aspirational | Fallback (ideal intersection) |
|----------------------------------------------|--------------|-------------------------------|
| SIMPLE ≻ NAVIGABLE ≻ SECURE ≻ COMPOSABLE     | IDEAL        | `SIMPLE_NAVIGABLE` (default)  |
| SECURE ≻ SIMPLE ≻ COMPOSABLE ≻ NAVIGABLE     | IDEAL        | `SIMPLE_SECURE`               |
| NAVIGABLE ≻ COMPOSABLE ≻ SECURE ≻ SIMPLE     | IDEAL        | `COMPOSABLE_NAVIGABLE`        |
| …                                            | …            | …                             |

Override the aspirational target via `preferences.target` if the caller
knows up front that IDEAL is out of reach for the file.

> **Changed in v0.5.0.** The fallback target is no longer adjacent to
> IDEAL in the walk. With three generators, "meet of the top two" and "one
> step below IDEAL" happened to be the same element; with four they differ.
> One step below IDEAL concedes only the single lowest-ranked generator
> (`SIMPLE_SECURE_NAVIGABLE` under the default ranking); the fallback
> concedes the bottom two.

## The targeted relaxation walk

Given a current verdict, the **relaxation walk** is the descending
preference-ordered list of verdicts from the aspirational target down
to (but not including) the current verdict.

```
ranking = [SIMPLE, NAVIGABLE, SECURE, COMPOSABLE]   current = SECURE
target  = IDEAL                                     fallback = SIMPLE_NAVIGABLE
walk    = [IDEAL, SIMPLE_SECURE_NAVIGABLE, SIMPLE_COMPOSABLE_NAVIGABLE,
           SIMPLE_NAVIGABLE, SIMPLE_COMPOSABLE_SECURE, SIMPLE_SECURE,
           SIMPLE_COMPOSABLE, SIMPLE, COMPOSABLE_SECURE_NAVIGABLE,
           SECURE_NAVIGABLE, COMPOSABLE_NAVIGABLE, NAVIGABLE,
           COMPOSABLE_SECURE]
next_step = COMPOSABLE_SECURE
```

The `next_step` field is the *smallest* improvement that still respects
the preference order — the safest immediate goal.

## How to use it

Pass `preferences` to any evaluate or assess tool:

```json
{
  "filepath": "src/server.rs",
  "preferences": {
    "ranking": ["composable", "secure", "simple", "navigable"]
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

- `priority` (`simple` / `composable` / `secure` / `navigable`) — knob on
  the scorers `Φᵢ`. Steers per-generator guidance. **Does not** linearize
  Ω.
- `preferences.ranking` — strict total order. Induces a total order on
  Ω and decides which meet is the divert-point when IDEAL is
  unreachable.

Use them together: `priority` tells the scorer which generator to
emphasize; `preferences` tells the agent which lattice neighbor to aim for
next.
