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

import json

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
    pref_args = f', "preferences": {{"ranking": {json.dumps(ranking)}}}'
    return f"""Improve `{filepath}` with Topos. Priority: **{priority.value}**. Iteration budget: **{max_iterations}**. Preference order: `{ranking_str}`.

Use the compact contract in `topos://docs/agent-contract`. Success means a focused structural change moves the target toward `preference_walk.next_step` or the fallback target, preserves behavior, and leaves residual risks explicit.

Core tool calls:
```json
{{"params": {{"filepath": "{filepath}"{pref_args}}}}}
```
Use with `topos_evaluate_file` to measure the current verdict.

```json
{{"params": {{"filepath": "{filepath}"{pref_args}}}}}
```
Use with `topos_inspect_code` when the returned `agent_contract`, `guidance`, or `suggestions` indicate inspection is needed.

```json
{{"params": {{"filepath": "{filepath}", "baseline_ref": "HEAD"{pref_args}}}}}
```
Verification route:
- Default after in-place edits: `topos_assess_worktree_change` against `HEAD` or another git ref.
- Dirty or untracked baseline: call `topos_begin_refactor` before editing, then `topos_assess_snapshot`.
- Side-by-side variant only: use `topos_assess_improvement`.

Acceptance gates:
- Assessment `status` is `IMPROVEMENT` or `IMPROVEMENT_SCORE`.
- Assessment `status` is not `SUSPICIOUS_NO_STRUCTURAL_CHANGE`.
- Active SECURE findings are fixed or intentionally acknowledged and disclosed.
- Project rollup is checked after non-trivial cross-file changes.
- Relevant behavior tests, type checks, or linters pass when available; if unavailable or not run, report that explicitly.

Return only the baseline, change summary, Topos verification, behavior verification, and residual risks.
"""
