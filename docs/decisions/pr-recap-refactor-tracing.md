# pr-recap v2: trace splits, lead with the lattice

Status: proposal (2026-09-21). Evidence: CalenDeez PR #5 ("Topos Refactor", 23 files, +2539/-1864,
four large files split into 21) reviewed with `topos pr-recap 5` at commit c7ab9b4.

## What the current card got wrong on PR #5

| # | Symptom on the card | Cause (verified) | Where |
|---|---|---|---|
| 1 | `PollShell.tsx`, `WeekGrid.tsx`, `LinkForm.tsx` (the three files the PR is about) have no rows, no scores, `SLOP → SLOP`, `structural_distance: null`. 11 of 17 new files "SLOP". Headline `LATERAL · Existing files kept their medals`. | `classify_source`, `structural_distance`, `file_hotspots`, `new_security_finding` all build `ProgramMorphism::new(source, language)` with no file path, so `dispatch::tree_sitter_language` picks the plain TypeScript grammar for `.tsx`. JSX fails to parse → `is_parseable = false` → `file_status` returns `LateralMove`, pillars `measured: false`. `topos evaluate` uses `from_file` and scores the same files fine (PollShell.tsx is BRONZE with 18 raw metrics). Reproduced: copying `PollShell.tsx` to `x.ts` and evaluating gives `SLOP · 0%` with an empty pillar table. | `cli/src/commands/pr_recap.rs` (`classify_source`, `structural_distance`, `file_hotspots`, `dangerous_calls`, `new_security_finding`) |
| 2 | One row per (pillar, file) — five rows for a 23-file PR, none of them for the files that changed most. | `change_rows` iterates pillars inside files and emits only lost/cleared/≥1-point moves. | `render_recap`, `change_rows`, `change_row` |
| 3 | `scope.note` says "module coupling was not measured" while `coupling_available: true`. | Static format string. | `build_recap` |
| 4 | Context line promises "COMPOSABLE not measured. Building it on the next run" but `pr-recap` never generates stores; only `depgraph generate-pr` does. | `context_line`, `prepare_coupling` only reads. | `context_line` |
| 5 | `depgraph generate-pr` prints `Next: topos pr-recap 5 --coupling`; the flag does not exist. | Stale hint. | `depgraph/generate.rs` |
| 6 | "got simpler as 17 new files arrived. This does not trace which function moved." | No tracing implemented, even though both coupling stores and both trees are already on disk. | `split_note` |
| 7 | `create.ts` COMPOSABLE `27% → 0% DOWN` reads as a coupling regression. | Parent now imports its four extracted children; fan-out counts edges to its own children. A good split always raises the parent's raw fan-out. | `pillar_deltas` + COMPOSABLE gate |
| 8 | `inspect` lists `createBookingForSlot.<anonymous>@135` etc.; `function_scopes` on `PollShell.tsx` finds 1 named function and 9 anonymous. | `scopes::uast_name_node` only looks for an `Identifier` child of `FunctionDecl`. `const Foo = () => {}` puts the name on the enclosing `variable_declarator`, which is mapped `Unknown`. | `engine/src/functors/probes/ast/scopes.rs`, `mapper_javascript.rs` |

## What the data already proves about PR #5 (no new algorithms needed to obtain it)

Head store `.git/topos-pr-5/head/.gitnexus`, base store `.../base/.gitnexus` (GitNexus 1.6.8, 160 files, 1051 nodes, 2754 edges, ~6 s to build each).

- `DEFINES` set-diff across the 23 changed files: 134 symbols at base, 160 at head. **52 moved** (same name, defined in a different changed file at head), **27 new** (17 components/functions + 10 `*Props` interfaces), **1 gone** (`day2`).
  - PollShell.tsx → poll-shell-types.tsx: 12 symbols (`GoogleGlyph`, `formatDateTime`, `formatHours`, `readStoredPolarity`, `storePolarity`, `polarityStorageKey`, `fetcher`, `PollShellProps`, `PollSummary`, `RankedSlot`, `AUTOSAVE_DELAY_MS`, `MAX_KILL_MODAL_SLOTS`).
  - WeekGrid.tsx → week-grid-model.ts: 17 symbols incl. `buildModel` (cx 13 → 13); `cellClass` → WeekGridDesktop; `cellVisual` → both Desktop and Mobile (duplicated); `LegendItem` → WeekGridLegend.
  - LinkForm.tsx → link-form-defaults.ts: 14 symbols; `RadioCard`, `SectionCard`, `Stepper` → form-controls.tsx.
  - create.ts → booking-error.ts: `BookingCreationError`.
- `IMPORTS` File→File at head: PollShell.tsx imports all 5 new poll files; WeekGrid.tsx imports its 4; LinkForm.tsx its 4; create.ts its 4. Every added file is imported by its parent. `CALLS` corroborates (PollShell → PollIdentifyView, KillCheckModal, ThinCoverageModal, …; `routeAfterIdentify → readStoredPolarity`).
- Afferent coupling of each new file at head: 13 of 17 are imported only by their parent ("private extraction"); `poll-shell-types` (6), `week-grid-model` (5), `link-form-defaults` (4), `booking-error` (3), `form-controls` (2) are shared.
- Cluster ledger from `topos evaluate --gitnexus-dir <store>` at base vs head:

| Cluster | LOC | Σ cfg.cyclomatic | worst fn complexity | parent fan-out | parent medal | children |
|---|---|---|---|---|---|---|
| PollShell | 1304 → 1475 (+13%) | 68 → 71 | 117 → 85 | 15 → 24 | BRONZE held (SIMPLE, NAVIGABLE still fail) | 4 PLATINUM, 1 GOLD |
| WeekGrid | 1191 → 1439 (+21%) | 83 → 87 | 134 → 93 | 0 → 5 | SILVER held | 1 PLATINUM, 3 GOLD |
| LinkForm | 1138 → 1305 (+15%) | 33 → 46 (+39%) | 117 → 73 | 11 → 19 | BRONZE held | 2 PLATINUM, 1 GOLD, 1 SILVER |
| create | 313 → 411 (+31%) | 17 → 14 | 33 → 13 | 13 → 11 | BRONZE → SILVER (cleared NAVIGABLE) | 4 PLATINUM |

Honest structural verdict, which nothing on the current card says: worst-function complexity fell 27–60 % in every cluster, total decision count did not fall (201 → 218), lines grew 18 % (glue and props types), the three component parents still fail SIMPLE and NAVIGABLE, and 13 of 17 new files are used only by their parent.

## Design

### Principles
1. Rows are files, pillars are columns. Files are grouped into split clusters (parent, then `├`/`└` children) because that is the shape of the change.
2. Lead with what competitors cannot compute: lattice movement, cosmetic-change detector, complexity ledger, real coupling delta, project-level regression. Medal counts come last.
3. Every number on the card is deterministic and reproducible from `--json`. No prose that is not backed by a field.
4. Splits are judged as clusters. Parent→own-children edges are excluded from the parent's *coupling delta* used for headline/regression (raw values still shown), otherwise every good split reads as a COMPOSABLE regression (symptom 7).
5. Fold aggressively: siblings with the same medal, same pillar row and fan-in 1 collapse into one `N files` row; `--verbose` unfolds and prints the per-function ledger.

### Card (primary, ≤100 cols, NO_COLOR-safe)

Same conventions as `topos evaluate`: a project pillar table with `━━◆──` score rails (before → after,
failing counts), medal emoji plus tier and average on the floor, `Why` / `Where to look` blocks in `inspect`'s
style under it, and a `Tip:` line after the card. `topos pr-recap --help` carries the full glossary. Then two headed sections, each omitted when empty, sharing one row shape: mark and row type, file,
medal as a tier word (`BRONZE → SILVER` when it changed), then four spaced pillar columns
`S C E N` (SIMPLE, COMPOSABLE, SECURE, NAVIGABLE) that explain the medal positionally: `●` passed,
`○` failed, `·` not measured. On a modified file a pillar whose score moved at least one point
carries an arrow (`●↑`, `○↓`). The scores table ends there. The splits table appends WORST FN and
DECISIONS on parent rows and one fact on child rows; notable children get rows, the rest fold
into `N more` with a tier tally. Held and deleted files collapse to counts. `--verbose` adds the
FROM/TO detail per moved pillar, lists names, unfolds children, and prints the function ledger.

```
◇  Reviewed 23 changed files  +2539/-1864
│  #5 refactor/topos → main · priority secure · COMPOSABLE measured · 1 skipped
│
│  · LATERAL   0 lost · 1 up · 17 new (11 PLATINUM, 5 GOLD, 1 SILVER)
│
│  PILLAR        STATUS   BEFORE   AFTER   FAILING   SCORE
│  SIMPLE        X FAIL      11%     38%   12 / 23   ━━━◆────── ↑
│  COMPOSABLE    X FAIL      23%     26%    3 / 23   ━━◆─────── ↑
│  SECURE        ✓ PASS     100%    100%    0 / 23   ━━━━━━━━━◆
│  NAVIGABLE     X FAIL      29%     70%    4 / 23   ━━━━━━◆─── ↑
│
│  CHANGE       FILE                           MEDAL             S   C   E   N
│  ✓ UP         lib/polls/ranges.test.ts       GOLD              ○↑  ●   ●   ●↑
│  ✓ UP         lib/bookings/create.ts         BRONZE → SILVER   ○↑  ○↓  ●   ●↑
│
│  SPLIT        PARENT → CHILDREN              MEDAL             S   C   E   N   WORST FN DECISIONS
│  ! SPLIT      components/links/LinkForm.tsx  BRONZE            ○   ○↑  ●   ○   117→73   33→46 +39%
│               ├─ link-form-defaults.ts       SILVER            ○   ●   ●   ○   19 in · shared ×3
│               ├─ form-controls.tsx           PLATINUM          ●   ●   ●   ●   5 in · shared ×2
│               ├─ LivePreviewCard.tsx         GOLD              ○   ●   ●   ●   1 in
│               └─ 1 more                      PLATINUM
│  ✓ SPLIT      components/polls/PollShell.tsx BRONZE            ○   ○↑  ●   ○   117→85   68→71
│               ├─ poll-shell-types.tsx        PLATINUM          ●   ●   ●   ●   11 in · shared ×4
│               ├─ PollIdentifyView.tsx        PLATINUM          ●   ●   ●   ●   1 in
│               ├─ PollModals.tsx              PLATINUM          ●   ●   ●   ●   2 in
│               └─ 2 more                      PLATINUM, GOLD
│  ✓ SPLIT      components/polls/WeekGrid.tsx  SILVER            ○   ●↓  ●   ○   134→93   83→87
│               ├─ week-grid-model.ts          GOLD              ○   ●   ●   ●   19 in · shared ×4
│               ├─ WeekGridDesktop.tsx         GOLD              ○   ●   ●   ●   9 in
│               ├─ WeekGridLegend.tsx          PLATINUM          ●   ●   ●   ●   LegendItem in
│               └─ 1 more                      GOLD
│  ✓ SPLIT      lib/bookings/create.ts         BRONZE → SILVER   ○↑  ○↓  ●   ●↑  33→13    17→14
│               ├─ booking-error.ts            PLATINUM          ●   ●   ●   ●   3 in · shared ×3
│               ├─ assign-hosts.ts             PLATINUM          ●   ●   ●   ●   4 in
│               ├─ calendar-event.ts           PLATINUM          ●   ●   ●   ●   3 in
│               └─ 1 more                      PLATINUM
│
│  · 1 file held its medal
│
└  · LATERAL · 🥉 BRONZE · SECURE · 41% → 59% average.

  Why  worst functions down 27–60%, decisions 201→218

Tip: add --verbose to list the functions that moved and each score that changed.
```

Row types: `LOST` (a pillar lost), `DOWN` (score-only dip), `COSMETIC` (score moved, syntax tree did
not), `UP` (cleared a pillar or score rose), `NEW` (added, not part of a split), `SPLIT` with `✓`/`!`/`X`
(worst function fell and decisions grew ≤10 % / decisions grew >10 % or a child arrived SLOP / the
parent lost a pillar or a moved function got more complex). Rows are ordered worst first; SECURE
losses sort first and are counted separately in the headline. Removed functions and deleted files
are counts, never losses. A project-level loss is the floor sentence, and every `X LOST` file gets a
hotspot line with the fix. Colors are redundant encoding only: marks, tier words by medal, arrows by
direction. Dots are never colored.

### Compact card (`--compact`, or automatically when stdout is not a TTY)

```
◇  topos pr-recap #5  23 files +2539/-1864 · COMPOSABLE measured
│  ✓ IMPROVEMENT  1 up · 0 lost · 17 new: 11 PLATINUM 5 GOLD 1 SILVER · 0 cosmetic
│  ✓ SPLIT  PollShell.tsx  → 5   worst 117→85   decisions 68→71   lines +13%   moved 12
│  ✓ SPLIT  WeekGrid.tsx   → 4   worst 134→93   decisions 83→87   lines +21%   moved 17
│  ! SPLIT  LinkForm.tsx   → 4   worst 117→73   decisions 33→46   lines +15%   moved 17
│  ✓ SPLIT  create.ts      → 4   worst  33→13   decisions 17→14   lines +31%   moved 1
│  ✓ UP     create.ts  BRONZE→SILVER  cleared NAVIGABLE
│  · HELD   PollShell BRONZE · WeekGrid SILVER · LinkForm BRONZE · ranges.ts GOLD · ranges.test GOLD
│  ! STILL  PollShell, WeekGrid, LinkForm fail SIMPLE + NAVIGABLE (worst fn 85, 93, 73 > gate 10)
│  · PRIV   13 of 17 new files are imported by their parent only
└  ✓ PASS · project BRONZE→BRONZE · decisions 201→218 · exit 0 · --json for the full document
```

### GitHub sticky comment (`--format github`, future Action)

`<!-- topos-pr-recap:v2 -->` marker on line 1 so the Action edits rather than reposts. Headline line, one cluster summary table, one `<details>` per cluster with the moved/new/lost symbol ledger, one `<details>` with a Mermaid `graph LR` of parent → children labelled with fan-in, and a footer with the non-claim and the reproducing command. Budget ~60k chars: if a PR has more than 12 clusters, emit ledgers for the top 6 by symbols moved and point to the job summary for the rest. Same JSON document feeds all three renderers.

## Algorithms

### A. Split-cluster detection (uses data already on disk)
Inputs: `git diff --name-status --find-renames` (A/M/D), base and head `ModuleDependencyGraph`.
1. Candidate children = A-status files. Candidate parents = M-status files (and D-status for pure moves).
2. Edge evidence (head store): parent `IMPORTS`/`CALLS` child (`outgoing(sym, "IMPORTS")` → `owning_file`).
3. Symbol evidence: `moved(P→C) = DEFINES_base(P) ∩ DEFINES_head(C)` by symbol name. Attach child to the parent with the most moved symbols; fall back to edge evidence; fall back to shared directory + name prefix.
4. Emit `Cluster { parent, children, moved: Vec<(symbol, from, to)>, new, lost }`.
Without stores: step 3 runs on UAST `function_scopes` names (callables only, after fix B); step 2 falls back to the file's import statements, which the UAST has as `ImportDecl`/native import nodes.

### B. Name inference for arrow/expression functions (engine)
In `scopes.rs`, when a `FunctionDecl` has no `Identifier` child, walk to the parent and accept the first `Identifier`/`property_identifier` of an enclosing `variable_declarator`, `pair`, `public_field_definition`, `assignment_expression`, or `export_statement → lexical_declaration → variable_declarator`. Pass the parent down in `collect_scopes` (it already carries `chain`). This fixes `inspect`'s `<anonymous>@135` rows, makes `ast.max_function_complexity` targetable for React components, and makes the ledger work without coupling stores. Same rule helps Python (`x = lambda`), Go (`var f = func`), Rust (`let f = |x|`).

### C. Function-level move matching
For each cluster, `calculate_function_complexity_entries` at base (parent) and head (parent + children).
1. Exact qualified-name match across files → `MOVED` (same body if subtree distance == 0, `MOVED+EDITED` otherwise). Use `compare_uast`/`uast_edit_distance` on the two function subtrees (exists in `profunctors/uast/compare.rs`; today only whole-program distance is wired).
2. Unmatched base functions vs unmatched head functions: subtree similarity ≥ 0.8 and same complexity ± 1 → `RENAMED`. Below that: base-only = `REMOVED`, head-only = `NEW`.
3. Complexity ledger: Σ complexity of moved functions before vs after (should be equal), Σ of NEW (= glue), Σ of REMOVED. Plus file-level Σ `cfg.cyclomatic` over the cluster (double-count-free).
4. Sibling duplication: `uast_kind_histogram`/`merge_kgram_counters` similarity between two children ≥ 0.85 (WeekGridDesktop vs WeekGridMobile, `cellVisual` in both) → `! DUPLICATED`.

### D. Split-aware coupling delta
`fan_out_delta(parent)` and `coupling_delta(parent)` for the headline exclude targets whose `owning_file` ∈ cluster.children. Raw values are still reported in the row. Child classification: `afferent == {parent}` → `private`, otherwise `shared (n)`.

### E. Project rollup line
Reuse `rollup`/`aggregate` from `mcp/src/tools/assess.rs` (move to the engine or a shared crate module) over the 23 touched files at base and head, so `project_regression` and `aggregate_before/after` appear on the CLI card as they do in `topos_assess_changeset`.

### F. Coupling stores inline
`gitnexus analyze` took ~6 s per tree on this repo. `pr-recap <PR>` should create the two worktrees and build both stores itself (reusing `generate-pr` code) when GitNexus is installed, print `COMPOSABLE not measured (gitnexus not installed)` otherwise, and never promise a build it does not do. Keep `depgraph generate-pr` as the explicit warm-up.

## Implementation order
1. Bug fixes, no design change: pass the file path into every `ProgramMorphism` in `pr_recap.rs` (add `ProgramMorphism::with_path(source, language, path)`), fix `scope.note`, `context_line`, and the `--coupling` hint. Add a `.tsx` regression test to `pr_recap.rs` tests. This alone changes PR #5 from `LATERAL` to a real result.
2. Cluster detection (A) + split-aware coupling (D) + file-row renderer with folding. `--json` gains `clusters[]`.
3. Name inference (B) in the engine, with mapper tests for TS/JS/Python.
4. Function ledger (C) with `--verbose` rendering; `RENAMED` and `DUPLICATED` behind thresholds documented in `docs/decisions`.
5. Project rollup (E), inline stores (F), `--compact`, `--format github`. Then the Action.

## Non-goals
- No LLM prose. No behavior claims; keep the `non_claim` field.
- Cross-repository PRs and forks stay unsupported until the Action needs them.

## Where this sits against the field (research, 2026-09-21)

Refactoring detection
- RefactoringMiner 3.x (Java, Python, Kotlin, TS/JS via swc4j "validation pending", C/C++) detects ~106 refactoring types with statement-level AST diff; ships a GitHub Action (`Pogut/RefactoringMiner-action`) that posts one grouped markdown comment per PR, deletes and re-posts on every push, and reports **no metrics**. That Action is the closest existing product to `pr-recap`; the wedge is the ledger and the lattice.
- RefDiff (1.0 calibration, arXiv 1704.01544): entities as TF-IDF token multisets, one matching relationship per entity, highest similarity wins. **Extract Method threshold is 0.1** because the evidence is the *call edge* (survivor calls the new method), not similarity. Our cluster rule (A) mirrors this: parent imports/calls child + moved-symbol containment.
- GumTree: per-file-pair only (minHeight 2, minDice 0.5, maxSize 100 in the paper); no cross-file move notion. difftastic, diffsitter, mergiraf, Sourcegraph, OpenRewrite: none documents cross-file function moves.
- git `-M`/`-C`: whole-blob, 50 % similarity default, copy detection needs `--find-copies-harder`, rename-limit silently skips the exhaustive pass. Wrong granularity for a function ledger.
- Complexity conservation: no prior art publishes a ledger. Empirical Extract-Method studies report average CC drops; CodeScene argues CC ignores nesting and does not analyse redistribution. This is open ground.

Adopt from the research into algorithm C
- Matching ladder, cheapest first: (0) normalized body hash → `moved_identical`; (1) same name + same file → `in_place`; (2) same name, different file → `moved`; (3) residue by `sim = 1 − distance` with assignment sorted by similarity, lexicographic tiebreak, both ends unmatched (stops a copied function from being claimed twice). Thresholds: `≥0.8` moved_modified / renamed, `0.5–0.8` low-confidence candidate, `<0.5` deleted + added. Name equality lowers the bar to 0.5, never raises it.
- Extract is structural: `h` new, `b'` survives, `b'` calls `h`, directional containment `simp(h, b) ≥ 0.1`.
- JSX identity: key on the exported PascalCase binding and unwrap `React.memo(Foo)`, `forwardRef(Foo)`; normalize attribute order. Charge imports, `*Props` interfaces and exports to file-level `new_glue`, never to the moved component.
- Ledger invariant, asserted at runtime: `Σ(head) − Σ(base) == in_place_Δ + moved_modified_Δ + extracted + new_logic + new_glue − deleted`. If it does not balance, print the failure, not the number. Verdict words: `relocated` (|Δ| ≤ 5 %, extracted + new_logic ≈ 0), `reduced` (Δ < 0), `inflated` (Δ > 0 driven by glue). Always print worst-function and nesting separately from the total.

PR review bots (14 products, official docs only)
- Moved-code and cosmetic/no-op columns are empty for every product; no product posts a dependency/coupling graph (all shipped diagrams are sequence diagrams: CodeRabbit, Greptile, Bito, Sourcery, Qodo). Only CodeScene posts a structural before→after score (Code Health `10.0 → 9.0`); Codacy and GitHub Code Quality post coverage deltas; Sonar is new-code scoped, which is exactly the "touched file improves, project regresses" blind spot. Only DeepSource has a non-LLM component in PR output.
- CodeScene already markets "deterministic" (PR Refactoring Agent, 2026-05). Position on **reproducible arithmetic with a balancing ledger and a lattice**, not on determinism alone.
- GitHub mechanics for the Action: comment body cap **65,536 chars** (edit in place via a hidden marker, do not delete-and-repost); job summary **1 MiB per step, overflow silently dropped**; check-run annotations **50 per request**; Mermaid renders in PR comments; `<details>` collapse is the convention.
