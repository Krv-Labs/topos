# New code meets the same complexity limit as changed code

Status: **decided (2026-11)**. Changes the default of the `new_file_pillar`
gate in `topos pr-recap`.

## The gap

`pr-recap` blocks a change that makes a function in an existing file too
complex (`pillar_lost`). The same function in a new file only raised a note:
`new_file_pillar` was off under relaxed, info under recommended and warn under
strict. A new file holding a function of complexity 14, or 301, was READY.

Splits made it worse. A file that splits out of a changed file (a split child)
skipped the new-file gates entirely, and the split gates check SECURE, moved
growth and bloat, not SIMPLE or NAVIGABLE. Moving one small function out of
`big.py` into a new `helpers.py`, beside a brand-new 14-branch function, was
READY under every preset, with one `split_bloat` note.

## Decision

New code must meet the same limit as changed code.

- `new_file_pillar` defaults to `block` under relaxed, recommended and strict,
  as `pillar_lost` does. The gate key is unchanged, so a project that set it in
  `[pr_recap.gates]` keeps its own severity.
- The block covers SIMPLE and NAVIGABLE, the complexity and nesting limits.
  SECURE and SLOP were already blocked by `new_file_insecure` and
  `new_file_slop`.
- A new file failing COMPOSABLE keeps the severity `new_file_pillar` had
  before: off under relaxed, info under recommended, warn under strict. The
  gate's severity is capped there for COMPOSABLE findings, the way a score
  drop under the materiality thresholds is capped at info, so no gate row is
  added. COMPOSABLE on a new file is its fan-out in the dependency graph, and
  that graph has false edges (2.4 to 3.7% of imports in recent trials, and 43
  impossible cross-crate edges in this repository), so a new file could be
  blocked on fan-out it does not have.
- The finding names the most complex function that is new logic, its
  complexity and the limit, and the fix (extract a helper, split decision
  logic): `src/route.py is new and fails SIMPLE in route (line 1):
  ast.max_function_complexity is 12, limit 10.`

## Split children and moved code

A move is not new code. The rule uses the range's function ledger, the one
that already turns a move into an existing file into `moved_pillar`:

- A function that arrived from another file in the range without growing
  (moved byte for byte, or edited on the way without gaining complexity) is
  moved code. In a split child it raises nothing, as before. In any other new
  file it is reported under `moved_pillar` (info, warn under strict), naming
  the file it came from.
- A function that is new, grew on the way, or that the ledger cannot match
  (including when the range is too large to ledger) is new logic.
- A split child that fails SIMPLE or NAVIGABLE raises `new_file_pillar` when
  the failing function is new logic. For SIMPLE every function over the
  complexity limit is checked; for NAVIGABLE the most divergent function is.
  A split child failing only on a file-level metric (the entropy band, or
  COMPOSABLE fan-out) is left to its split, as before.

## Measured impact

Replayed on 23 merged pull requests (16 on Krv-Labs/topos, 7 on
pytest-dev/pytest; 39 added files, 24 of them split children), with the build
before and after this change:

| | Blocked | Readiness changed |
|---|---|---|
| Before | 1 of 23 | |
| After | 1 of 23 | none |

The one blocked pull request was already blocked by four `pillar_lost`
findings; its two new files with functions over the limit now add four
`new_file_pillar` blocks beside them. No split child in the set raised the
new finding. The adversarial cases (a new file with a complexity-14 or
complexity-301 function, and the split above) go from READY to BLOCKED under
recommended and strict; a complex function moved unchanged into a new file
stays READY under recommended.

The presets were calibrated on 1,609 pull requests; this gate's default was
changed by owner decision after that calibration, and the replay above is the
measurement behind it, not a recalibration.

## Out of scope

- COMPOSABLE on new files, until the dependency graph's false edges are
  gone. `pillar_lost` still blocks a changed file that loses COMPOSABLE.
- `pillar_inherited` (an already-failing file getting worse), `score_drop`
  and the entropy band are unchanged. Whether a new file outside the entropy
  band alone should block, and whether an already-failing file that gets worse
  should, are open questions.
- NAVIGABLE in a split child checks only the most divergent function; a second
  new function over the divergence limit behind a moved one is not seen.
- A moved function counts as moved when its complexity did not grow; a move
  that deepened nesting without adding complexity is still read as a move.
