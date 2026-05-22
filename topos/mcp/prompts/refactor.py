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

from topos.evaluation.policies.base import Priority

from ..server import mcp


@mcp.prompt(
    name="topos_refactor_until_ideal",
    tags={"workflow"},
    description=(
        "Scaffolds the canonical Topos refactor loop (review → plan → refactor "
        "→ re-measure) with a concrete target, tool call sequence, and "
        "termination criteria."
    ),
)
def topos_refactor_until_ideal(
    filepath: str,
    priority: Priority = Priority.SECURE,
    max_iterations: int = 5,
    preferences: list[str] | None = None,
) -> str:
    """Generate a refactor-loop prompt for the given file.

    Args:
        filepath: Target file to refactor.
        priority: Which generator to prioritize (``simple``, ``composable``,
                  or ``secure``; default ``secure``).
        max_iterations: Budget for iterations before stopping.
        preferences: Optional strict total order on the three generators
                     (e.g. ``["composable", "secure", "simple"]``).  When
                     provided, the agent uses a two-stage strategy:
                     **first aim for IDEAL** (beat all three thresholds);
                     **then divert** to the ideal intersection (meet of
                     the top-two ranked generators) if IDEAL plateaus.
    """
    ranking = preferences or [
        priority.value,
        *[p.value for p in Priority if p.value != priority.value],
    ]
    ranking_str = " ≻ ".join(ranking)
    pref_args = f', preferences={{"ranking": {ranking!r}}}'
    pref_block = (
        f"\n**Preference order:** `{ranking_str}` — Two-stage strategy: "
        "aim for `preference_walk.target` (IDEAL) first; if it stalls "
        "after a few iterations, divert to `preference_walk.fallback_target` "
        "(the meet of your top-two ranked generators). `next_step` is "
        "always your immediate goal.\n"
    )
    return f"""Refactor `{filepath}` using the Topos closed-loop method. Priority: **{priority.value}**. Budget: **{max_iterations} iterations**.{pref_block}

**Before you start**, read `topos://docs/workflows` — it's the orchestration guide.

---

### Step 1 — Measure baseline
Call `topos_evaluate_file(filepath="{filepath}"{pref_args})`.
If `coupling_available: false` in the response, run `topos depgraph generate` first; any verdict containing COMPOSABLE (including IDEAL) is unreachable without it.

### Step 2 — Inspect
Call `topos_inspect_code(filepath="{filepath}"{pref_args})` to find the highest-complexity functions and their line numbers.

### Step 3 — Propose
Make ONE focused change targeting the lowest-scoring generator. Do not shuffle complexity between generators; reduce it.

### Step 4 — Verify
Call `topos_assess_improvement(filepath="{filepath}", proposed_code=<new code>{pref_args})`.

Read the `status` field:
- `IMPROVEMENT` → apply the change, record progress, return to step 1 if not IDEAL.
- `IMPROVEMENT_SCORE` → verdict unchanged but progress made; continue.
- `LATERAL_MOVE` / `REGRESSION*` → discard the change, try a different angle.
- **`SUSPICIOUS_NO_STRUCTURAL_CHANGE`** → ⚠️ the tree barely changed. You are gaming the metric. Make a real structural change (extract, inline, split, merge), not a cosmetic one. Do NOT commit.

### Step 5 — Stop when
- Verdict = `IDEAL` (`preference_walk.target` by default — beat all three thresholds), OR
- Verdict reaches `preference_walk.fallback_target` after IDEAL plateaus (the ideal intersection — meet of your top-two ranked generators), OR
- Priority-specific generator satisfied (`{priority.value}` bit set), OR
- Iteration budget exhausted → report partial progress honestly.

**Divert rule:** if IDEAL hasn't moved after 2 consecutive iterations, switch your goal to `preference_walk.fallback_target` instead.

### Do NOT
- Run tests to "improve" the score by deleting them.
- Rename symbols or shuffle whitespace as a "refactor".
- Optimize one file while regressing the project rollup. Re-check with `topos_evaluate_project` after non-trivial changes.

Begin at Step 1.
"""
