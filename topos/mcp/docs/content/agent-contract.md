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
- `blocked_by` — missing preconditions such as `missing_gitnexus_dir` (no
  graph) or `stale_gitnexus_dir` (graph predates the latest commit or a source
  file was modified after generation). `topos_evaluate_file`/
  `topos_evaluate_project` generate/refresh the graph automatically before
  scoring, so these now only surface when that generation itself couldn't
  happen — GitNexus not installed, or the `gitnexus analyze` run failed;
  `warnings` carries the specific reason. An `invalid_gitnexus_dir` code means
  the supplied `gitnexus_dir` override escapes the file root — fix the path
  rather than generating. A not-yet-created in-root override is treated as
  missing and is auto-generated on evaluate (same as the default store path).
- `verification_gates` — checks required before accepting a patch.
- `risk_flags` — compact labels such as `grade_capped`,
  `active_security_findings`, or `metric_gaming_risk`.

`next_tool`/`next_actions` never contradict `blocked_by`: when ranked refactor
targets are returned alongside a setup blocker, `next_actions` carries both
the edit step and the setup remedy (e.g. `topos_generate_depgraph`). A stale
graph is advisory cadence, not a per-edit chore — refresh it before *trusting*
COMPOSABLE (typically once per assess checkpoint), not after every edit.

Prefer these fields over parsing prose guidance.

## Reading `achieved` vs. `scores`

They answer different questions — do not substitute one for the other.

- `pillars.*.achieved` — the **gating conjunction**: a hard pass/fail over
  only that pillar's gating raw metrics. This is the lattice verdict.
- `scores.*` (same value as `pillars.*.score`) — **continuous quality**,
  computed as the *minimum* of the per-metric quality curves. That minimum
  includes **advisory** metrics that deliberately do not gate (notably
  `cfg.cyclomatic`, whose whole-file sum scales with function count — see
  `topos://docs/metrics`).

Consequence: `achieved: true` alongside a low or even `0.0` score is
**normal**, not a bug. One advisory metric far out of band pins the minimum
at `0.0` while every gating metric still passes — typical of large
dispatch/discovery-class files made of many small, individually-simple
functions. Such a file can still evaluate to `IDEAL`.

So: **do not infer "not SIMPLE" from a low score — read `achieved`.** Use
`scores.*` for ranking and for measuring improvement between runs, and the
returned `refactor_targets` for the concrete spans to edit — reading each
target's `severity` to tell the two apart (see below).

`binding_constraint` answers the same question in one field: when present, it
is the single **gating** metric currently costing a pillar its `achieved` —
pillar, metric, measured value vs. threshold, and the span. It is the
top-ranked `"fix"` target restated without the edit payload, so it can never
name a different metric than `refactor_targets` does. When it is absent, no
gating metric is out of band among the computed targets — an advisory metric
is never promoted into it — so a low score with `achieved: true` and no
`binding_constraint` is exactly the advisory-only case above. Tools that do
not compute ranked targets omit the field entirely.

## Refactor Targets

`topos_evaluate_file` returns ranked edit targets by default:
`refactor_targets` (default `3`, `0` disables, capped at `25`) gives that many
concrete spans with the failing metric, current value vs. threshold, and
`recommended_operations` tokens, ordered gate failures first. Verification
guidance lives once on `agent_contract.verification_gates`, not per target.

Each target carries a `severity` that mirrors whether its metric gates:
`"fix"` means the out-of-band metric is costing that pillar's `achieved`;
`"improve"` means the metric is advisory (currently only `cfg.cyclomatic`)
— worth addressing for the score, but no verdict depends on it. Prioritize
`"fix"` targets when you must pick one.

These are metric-driven edit targets from the scoring pipeline. They are
not the same as advisory `topos_refactor(target="cycles"|"dependencies"|"process"|"graphify")`,
which never affects medals. See `topos://docs/workflows` § Advisory refactoring.

## Boundaries

- COMPOSABLE is scored automatically — `topos_evaluate_file`/
  `topos_evaluate_project` detect and generate/refresh `.gitnexus` by
  default (`no_composable: true` to skip). When `gitnexus_dir` /
  `--gitnexus-dir` is unset, the project root is the MCP **file root**
  absolute file or directory path (or the CLI **process cwd**).
  When the override is set, the COMPOSABLE project root is the **parent
  of that store path** (typically the parent of `.gitnexus`): freshness
  and `gitnexus analyze` target that derived root, not cwd/file-root.
  MCP still requires the store (and derived root) to stay inside the
  derived project root; the CLI allows absolute overrides outside cwd. If GitNexus
  isn't installed or generation fails, any verdict containing
  COMPOSABLE, including `IDEAL`, is unreachable — check `warnings` for
  why. `topos_depgraph_status` gives a read-only diagnosis without
  triggering generation; force an explicit refresh with
  `topos_generate_depgraph` rather than shelling out yourself.
- Use `allow` only for intentional dangerous calls. Acknowledged risks stay
  disclosed and can cap the grade.
- Use `verbose=true` only for deep inspection. Default outputs are designed to
  preserve agent context.
