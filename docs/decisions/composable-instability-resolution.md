# File-level instability resolution

Status: **RESOLVED and superseded as verdict policy in v0.5.0**. The current
policy is [`file-level-composable.md`](file-level-composable.md); the possible
package redesign is documented in
[`composable-at-module-granularity.md`](composable-at-module-granularity.md).

## Problem

Topos originally hard-gated file COMPOSABLE on Martin instability:

```text
I = Ce / (Ca + Ce)
```

For `n = Ca + Ce`, the only attainable values are `{k/n}`. Sparse file graphs
therefore make a fixed band depend heavily on denominator arithmetic. With one
edge, for example, only `0` and `1` are attainable; a balanced band cannot be
reached at all. This is not evidence that every single-edge file has poor
design.

A second defect used symbol-level `CALLS` fan as the test for whether the
import-derived instability signal was real. Pure interface files can make no
calls while still carrying genuine `IMPORTS` coupling, so the test consulted the
wrong graph.

## Resolution retained in v0.5.0

- `coupling_gate_input` tests resolvability using `mdg.coupling` (`Ca + Ce`),
  the same import graph from which instability is computed.
- `mdg.instability` and `mdg.main_sequence_distance` remain scored,
  interpreted advisories and can produce `improve` refactor targets.
- Neither metric can hard-fail a file.
- The former `is_leaf_composable_zero` suppression was retired;
  `leaf_composable_zeros` remains temporarily empty for wire compatibility.

The later file-level decision also made `mdg.fan_in` advisory. Only
`mdg.fan_out ≤ 10` now gates file COMPOSABLE.

## Why this is not score inflation

The continuous COMPOSABLE score still takes the minimum over its scored
readings, including instability, distance, and fan-in. Only `achieved` filters
out advisory metrics. A file can therefore pass the narrow outward-burden gate
while retaining a low score and detailed architectural warnings.

This distinction is intentional: the verdict answers a defensible file-level
question, while inspection preserves signals whose stronger interpretation
belongs at package scope.
