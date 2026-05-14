# Topos Agent Workflows

Per CodeScene's 2026 best-practice research ("agent-first tools need
AGENTS.md-style orchestration"), this document is the canonical recipe for
using Topos tools in a closed-loop refactor. Agents should read this on
first encounter with the server.

## The canonical loop: review → plan → refactor → re-measure

```
┌────────────┐      ┌────────────┐      ┌──────────────┐      ┌────────────┐
│ 1. MEASURE │ ───► │ 2. PLAN    │ ───► │ 3. PROPOSE   │ ───► │ 4. VERIFY  │
│  (evaluate)│      │ (identify  │      │  (refactor)  │      │  (assess)  │
│            │      │  weakest)  │      │              │      │            │
└────────────┘      └────────────┘      └──────────────┘      └─────┬──────┘
                                                                    │
                          ┌─────────────────────────────────────────┘
                          ▼
                    ┌──────────────┐
                    │ 5. DECIDE    │
                    │ accept / try │
                    │ again / stop │
                    └──────────────┘
```

### 1. Measure

- Single file: `topos_evaluate_file(filepath, gitnexus_dir)` — `gitnexus_dir`
  is required for the COMPOSABLE generator.  Without it, any verdict
  containing COMPOSABLE (including IDEAL) is unreachable.
- Whole project: `topos_evaluate_project(path, gitnexus_dir)` — rollup +
  worst-N file list. Start here to pick a target.

### 2. Plan

Read the `guidance` field of the evaluation result. It's priority-aware and
tells you which dimension to work on. If `guidance` says "provide
gitnexus_dir" you must run `topos depgraph generate` first.

For deep analysis of a specific file, call `topos_inspect_code` — it returns
top-N functions by complexity, entropy details, and the full metric table.

### 3. Propose

Write a refactor. Keep the change focused on one dimension at a time.
Submit via `topos_assess_improvement(filepath=..., proposed_code=...)`.

### 4. Verify

`topos_assess_improvement` returns one of:

- `IMPROVEMENT` — lattice moved up (e.g. SLOP → SIMPLE, or SIMPLE → SIMPLE_COMPOSABLE). Commit.
- `IMPROVEMENT_SCORE` — lattice unchanged but per-dim scores improved.
  Progress, but not a verdict jump yet.
- `LATERAL_MOVE` — neither improved nor regressed. Try a different angle.
- `REGRESSION` / `REGRESSION_SCORE` — revert and re-plan.
- **`SUSPICIOUS_NO_STRUCTURAL_CHANGE`** — ⚠️ scores moved but AST barely
  changed. The refactor is probably cosmetic (whitespace / comments /
  renames). Make a structural change, not a textual one. **Do not commit.**

### 5. Decide

Stop when:
- Verdict = `IDEAL` (all three generators satisfied), OR
- Priority-specific generator satisfied (`simple` → SIMPLE bit set,
  `composable` → COMPOSABLE bit set, `secure` → SECURE bit set), OR
- `max_iterations` exhausted — report partial progress honestly rather than
  gaming one more iteration.

## Escape hatches — when the loop stalls

### Stall #1: Every generator score plateaus below 60%

Often a sign the file needs to be **split**, not refactored. Use
`topos_inspect_code` to find the top-complexity functions; consider
extracting them into a separate module. Re-run `topos_evaluate_project` to
check the rollup doesn't regress as a result.

### Stall #2: `SUSPICIOUS_NO_STRUCTURAL_CHANGE` repeatedly

You're iterating on presentation. Step back: what is the *structural*
problem? Rename → not a refactor. Whitespace → not a refactor. Loop
unrolling, extracted helpers, collapsed conditionals → real refactors.

### Stall #3: SIMPLE improves, COMPOSABLE regresses

Classic "moved complexity elsewhere" anti-pattern. Re-run
`topos_evaluate_project` — did the other file's score drop? If so, the
refactor didn't reduce total system complexity, it just relocated it.
Consider if the abstraction is actually an improvement or just a shuffle.

## Priority selection cheat sheet

- Leaf module (few callers) → `simple`
- Library surface (many importers) → `composable`
- File handling untrusted input → `secure`
- Unknown / general cleanup → `secure` (default scorer emphasis)

See `topos://docs/priority` for more.

## Preference-driven targeting

For agent loops that need a concrete *next-best* verdict to aim for —
not just an upweighted generator — pass `preferences` alongside
`priority`. A `preferences.ranking` like `["composable", "secure",
"simple"]` induces a total order on Ω and produces a **two-stage**
target:

1. **`target`** — aspirational, default `IDEAL`. Try to beat the
   thresholds for all three generators first.
2. **`fallback_target`** — the **"ideal intersection"**, i.e. the meet
   of the top-two ranked generators (`COMPOSABLE_SECURE` in the
   example above). When IDEAL plateaus, divert here.

The result also returns a **`walk`** (descending verdicts from IDEAL
down) and a **`next_step`** (the smallest improvement above the
current verdict).

Concretely: aim for IDEAL for the first few iterations; if the lattice
verdict won't move, switch to `fallback_target` and try to satisfy
only the top-two generators. See `topos://docs/preferences`.

## What Topos does NOT measure

- **Test coverage.** A refactor that improves the score but breaks tests
  is a regression. Topos cannot see this; run the test suite separately.
- **Functional correctness.** AST edit distance measures *change*, not
  *preservation of behavior*. Always verify behavior with tests.
- **Runtime performance.** Orthogonal to all Topos metrics.
- **Beyond-syntactic security.** The SECURE generator catches obvious
  footguns (dangerous-API call sites, source→sink taint paths) via
  textual / structural pattern matching on the CPG.  It is not a full
  SAST / pen-test — pair with dedicated security tooling for high-stakes
  code.

Topos is one signal in a multi-signal loop. Pair it with test coverage and
type checks for the full picture.
