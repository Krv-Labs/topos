# Topos Agent Contract

Use Topos as a structural verifier inside an autonomous coding loop.

## Objective

Improve the target code toward the requested lattice target while preserving
behavior. Treat Topos as one signal: it measures structure, security footguns,
coupling, and structural test coverage; it does not prove functional
correctness.

## Call Shape

Every tool takes a flat arguments object: `{"filepath": "src/a.rs"}`. There is
no `params` wrapper; sending one is rejected as an unknown field.

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

- The assessment status is `IMPROVEMENT` or `IMPROVEMENT_SCORE`.
- The assessment status is not `SUSPICIOUS_NO_STRUCTURAL_CHANGE`.
- Active SECURE findings are fixed or intentionally acknowledged and disclosed.
- Project rollup does not regress after non-trivial cross-file changes.
- Relevant behavior checks pass, or missing checks are reported.

## Contract Fields

Evaluation, project, and assessment results may include `agent_contract`:

- `next_tool` — the next Topos tool to call, if Topos can identify one.
- `next_actions` — concise outcome-focused actions.
- `blocked_by` — missing preconditions such as `parse_failures`,
  `missing_gitnexus_dir` (no graph), or `stale_gitnexus_dir` (graph predates
  the latest commit or a source file was modified after generation). `topos_evaluate_file`/
  `topos_evaluate_project` generate/refresh the graph automatically before
  scoring, so these now only surface when that generation itself couldn't
  happen — GitNexus not installed, or the `gitnexus analyze` run failed;
  `warnings` carries the specific reason. An `invalid_gitnexus_dir` code means
  the supplied `gitnexus_dir` override escapes the file root — fix the path
  rather than generating. A not-yet-created in-root override is treated as
  missing and is auto-generated on evaluate (same as the default store path).
- `verification_gates` — checks required before accepting a patch.
- `risk_flags` — compact labels such as `parse_failures`, `grade_capped`,
  `active_security_findings`, or `metric_gaming_risk`.

In v0.5.0, the single-file parse code changed from singular `parse_failure`
to `parse_failures`, matching project evaluation. Field names remain
`blocked_by` and `risk_flags`; only the code value changed.

`next_tool`/`next_actions` never contradict `blocked_by`: when ranked refactor
targets are returned alongside a setup blocker, `next_actions` carries both
the edit step and the setup remedy (e.g. `topos_generate_depgraph`). A stale
graph is advisory cadence, not a per-edit chore — refresh it before *trusting*
COMPOSABLE (typically once per assess checkpoint), not after every edit.

Prefer these fields over parsing prose guidance.

## Reading `achieved`, `gate_scores`, and `scores`

A score below 50 means that pillar's gate failed. A score of at least 50
means every gate on that pillar passed. `pillars.*.achieved` is that same
cut. `gate_scores` is the gate-only value `G` (0.5 exactly at the gate).
`scores` is `G` moved by advisory quality, and that move stays on the same
side of 50.

Use `gate_scores` to choose the edit: among pillars the active goal still
requires, work the one with the greatest `G` below 0.5. That is
`preference_walk.next_pillar`. Use `scores` only to compare two pillars that
are already on the same side of 50. Advisory metrics (`cfg.cyclomatic` and
the rest) change `scores` and show up as `"improve"` targets. They do not
change `achieved`.

`binding_constraint` is the binding gate of that pillar: the metric, value,
threshold, and span of the top `"fix"` target. It is absent when no gating
metric is out of band. Tools that do not compute ranked targets omit the field.

## Refactor Targets

`topos_evaluate_file` returns ranked edit targets by default:
`refactor_targets` (default `3`, `0` disables, capped at `25`) gives that many
concrete spans with the failing metric, current value vs. threshold, and
`recommended_operations` tokens. Order is: pillars the active goal still
requires, then `"fix"` before `"improve"`, then the closest gate score, then
the lowest desirability inside that pillar. Preference rank only breaks a
tie. Verification guidance lives once on `agent_contract.verification_gates`,
not per target.

Each target carries a `severity` that mirrors whether its metric gates:
`"fix"` means the metric is costing that pillar's `achieved`; `"improve"`
means the metric is advisory. Prioritize `"fix"` targets. After every
required gate passes, `"improve"` targets raise the score inside that half
and do not move the lattice element.

These are metric-driven edit targets from the scoring pipeline. They are
not the same as advisory `topos_refactor(target="cycles"|"dependencies"|"process")`,
which never affects medals. See `topos://docs/workflows` § Advisory refactoring.

## Boundaries

- COMPOSABLE is scored automatically — `topos_evaluate_file`/
  `topos_evaluate_project` detect and generate/refresh `.gitnexus` by
  default (`no_composable: true` to skip). When `gitnexus_dir` /
  `--gitnexus-dir` is unset, the project root is derived from the MCP tool's
  absolute file or directory path (or the CLI **process cwd**).
  When the override is set, the COMPOSABLE project root is the **parent
  of that store path** (typically the parent of `.gitnexus`): freshness
  and `gitnexus analyze` target that derived root, not cwd/file-root.
  MCP still requires the store (and derived root) to stay inside the
  derived project root (and any optional `TOPOS_MCP_FILE_ROOT` boundary);
  the CLI allows absolute overrides outside cwd. If GitNexus
  isn't installed or generation fails, any verdict containing
  COMPOSABLE, including `IDEAL`, is unreachable — check `warnings` for
  why. `topos_depgraph_status` gives a read-only diagnosis without
  triggering generation; force an explicit refresh with
  `topos_generate_depgraph` rather than shelling out yourself.
- Use `allow` only for intentional dangerous calls. Acknowledged risks stay
  disclosed and can cap the grade.
- Use `verbose=true` only for deep inspection. Default outputs are designed to
  preserve agent context.
