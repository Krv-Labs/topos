# Topos Agent Contract

Use Topos as a structural verifier inside an autonomous coding loop.

## Objective

Improve the target code toward the requested lattice target while preserving
behavior. Treat Topos as one signal: it measures structure, security footguns,
coupling, and structural test coverage; it does not prove functional
correctness.

## Required Loop

1. Measure the current state with `topos_evaluate_file` or
   `topos_evaluate_project`.
2. Inspect only the weakest relevant area with `topos_inspect_code` or the
   returned `suggestions`.
3. Make one focused structural change.
4. Verify the change. If you edited the file in place, use
   `topos_assess_worktree_change` (baseline = a git ref, default `HEAD`) or, for
   untracked/uncommitted baselines, snapshot first with `topos_begin_refactor`
   and verify with `topos_assess_snapshot`. If you have a proposed variant in
   hand, use `topos_assess_improvement`. All share the same status semantics.
5. Run relevant project tests, type checks, or linters when available. If they
   are unavailable or not run, report that explicitly.

## Done Gates

A change is ready to accept only when:

- `topos_assess_improvement.status` is `IMPROVEMENT` or `IMPROVEMENT_SCORE`.
- The status is not `SUSPICIOUS_NO_STRUCTURAL_CHANGE`.
- Active SECURE findings are fixed or intentionally acknowledged and disclosed.
- Project rollup does not regress after non-trivial cross-file changes.
- Relevant behavior checks pass, or missing checks are reported.

## Contract Fields

Evaluation, project, and assessment results may include `agent_contract`:

- `next_tool` — the next Topos tool to call, if Topos can identify one.
- `next_actions` — concise outcome-focused actions.
- `blocked_by` — missing preconditions such as `missing_gitnexus_dir`.
- `verification_gates` — checks required before accepting a patch.
- `risk_flags` — compact labels such as `grade_capped`,
  `active_security_findings`, or `metric_gaming_risk`.

Prefer these fields over parsing prose guidance.

## Boundaries

- Use `gitnexus_dir` to score COMPOSABLE. Without it, any verdict containing
  COMPOSABLE, including `IDEAL`, is unreachable.
- Use `allow` only for intentional dangerous calls. Acknowledged risks stay
  disclosed and can cap the grade.
- Use `verbose=true` only for deep inspection. Default outputs are designed to
  preserve agent context.
