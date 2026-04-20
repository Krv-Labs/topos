# ruff: noqa: E501
"""
Canonical refactor loop prompt.

Invoking this prompt gives the agent a fully-populated plan referencing the
concrete tools and the workflow resource. It's the lowest-friction path to
getting an agent to use Topos *correctly* — no prompt engineering required
on the caller's side.

Line-length is waived at the file level because this module contains
user-facing prompt prose where soft-wrapping mid-sentence hurts readability.
"""

from __future__ import annotations

from topos.logic.policies.base import Priority
from topos.mcp.server import mcp


@mcp.prompt(
    name="topos_refactor_until_sound",
    tags={"workflow"},
    description=(
        "Scaffolds the canonical Topos refactor loop (review → plan → refactor "
        "→ re-measure) with a concrete target, tool call sequence, and "
        "termination criteria."
    ),
)
def topos_refactor_until_sound(
    filepath: str,
    priority: Priority = Priority.BALANCED,
    max_iterations: int = 5,
) -> str:
    """Generate a refactor-loop prompt for the given file.

    Args:
        filepath: Target file to refactor.
        priority: Which dimension to prioritize (``balanced``, ``composable``,
                  or ``self_contained``).
        max_iterations: Budget for iterations before stopping.
    """
    return f"""Refactor `{filepath}` using the Topos closed-loop method. Priority: **{priority.value}**. Budget: **{max_iterations} iterations**.

**Before you start**, read `topos://docs/workflows` — it's the orchestration guide.

---

### Step 1 — Measure baseline
Call `topos_evaluate_file(filepath="{filepath}", priority="{priority.value}")`.
If `coupling_available: false` in the response, run `topos depgraph generate` first; COMPOSABLE/SOUND are unreachable without it.

### Step 2 — Inspect
Read the file. Call `topos_inspect_code(code=<contents>, priority="{priority.value}")` to find the highest-complexity functions.

### Step 3 — Propose
Make ONE focused change targeting the lowest-scoring dimension. Do not shuffle complexity between dimensions; reduce it.

### Step 4 — Verify
Call `topos_assess_improvement(filepath="{filepath}", proposed_code=<new code>, priority="{priority.value}")`.

Read the `status` field:
- `IMPROVEMENT` → apply the change, record progress, return to step 1 if not SOUND.
- `IMPROVEMENT_SCORE` → lattice unchanged but progress made; continue.
- `LATERAL_MOVE` / `REGRESSION*` → discard the change, try a different angle.
- **`SUSPICIOUS_NO_STRUCTURAL_CHANGE`** → ⚠️ the tree barely changed. You are gaming the metric. Make a real structural change (extract, inline, split, merge), not a cosmetic one. Do NOT commit.

### Step 5 — Stop when
- Lattice = `SOUND`, OR
- Priority-specific target reached (`{priority.value}` → matching lattice element), OR
- Iteration budget exhausted → report partial progress honestly.

### Do NOT
- Run tests to "improve" the score by deleting them.
- Rename symbols or shuffle whitespace as a "refactor".
- Optimize one file while regressing the project rollup. Re-check with `topos_evaluate_project` after non-trivial changes.

Begin at Step 1.
"""
