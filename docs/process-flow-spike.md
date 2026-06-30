# GitNexus Process (dynamic behavior) extraction — spike

Status: **IMPLEMENTED (v1)**. This document records what GitNexus
"Processes" capture, how Topos lifts them into a `Representation`, and the
per-pillar metrics they feed. Calibration is provisional and flagged below.

## 1. What GitNexus captures

`gitnexus analyze` writes a LadybugDB graph store to `.gitnexus/lbug`. Beyond
the file/symbol/call structure consumed by the `ModuleDependencyGraph` (MDG),
the store contains a **process layer**: statically-reconstructed *execution
flows*. This is the closest signal the index has to dynamic behavior — the
interprocedural call *sequence* a program runs, recovered without execution.

Confirmed schema (read-only Cypher against this repo's `.gitnexus/lbug`):

- **`Process` node** properties:
  - `id` (e.g. `proc_0_curate_composable`)
  - `label` / `heuristicLabel` (e.g. `Curate_composable -> _prepare`)
  - `processType` (e.g. `cross_community`)
  - `stepCount` (int)
  - `communities` (list of community ids)
  - `entryPointId` / `terminalId` (symbol ids of the flow's first / last step)
- **`STEP_IN_PROCESS` edge**: direction `(Function|Class|Method) -> (Process)`,
  with a `step` order index plus `confidence` / `reason` (`trace-detection`).

Symbol ids embed the source path: `Function:<path>:<name>`
(e.g. `Function:topos/mcp/evaluation.py:classify_file`). This repo's index
holds **269 processes** over 3256 nodes / 213 communities.

## 2. What Topos used before vs. now

The MDG loader (`topos/graphs/mdg/object.py`) already pulls **every** node
table and **every** `CodeRelation` edge into memory, so `Process` nodes and
`STEP_IN_PROCESS` edges were already resident — just never read. `Process` /
`STEP_IN_PROCESS` appeared only in the schema `Literal` enums and nowhere
else. This spike surfaces them as a first-class `ProcessFlowGraph`
representation.

## 3. Extraction shape

- `topos/graphs/process/object.py`
  - `ProcessFlow` — one parsed flow (id, label, type, step count, communities,
    entry/terminal ids, ordered step symbol ids).
  - `ProcessFlowGraph` — implements the `Representation` protocol. Built from
    an already-loaded `ModuleDependencyGraph` via
    `ProcessFlowGraph.from_dep_graph(mdg, target_file, dimension=...)`, so the
    LadybugDB read is paid **once** (shared with COMPOSABLE) and reuses the MCP
    cache. It keeps only the flows that *touch* `target_file` (any of the
    entry, terminal, or step symbols resolves to the file via the same
    suffix-aware path match the MDG uses).
  - One `Representation` feeds exactly one pillar (`dimension`), so the graph is
    fanned out into three views via `for_dimension("simple"|"composable"|"secure")`,
    sharing the parsed flow list.

- `topos/functors/probes/process/flow.py` — pure measurements over a flow list:
  flow length, participation, community span, cross-community count, and
  dangerous-step (sink-on-flow) count using the existing danger registry.

## 4. Per-pillar metrics (v1)

| Pillar | Metric key | Meaning |
| --- | --- | --- |
| SIMPLE | `process.max_flow_length` | longest flow (steps) anchored at the file |
| SIMPLE | `process.flow_participation` | number of flows the file participates in |
| COMPOSABLE | `process.max_community_span` | most community boundaries any flow crosses |
| COMPOSABLE | `process.cross_community_flows` | count of `cross_community` flows on the file |
| SECURE | `process.dangerous_flows` | flows whose steps include a dangerous-API symbol |

These complement (do not replace) the existing intra-procedural lenses:

- **SIMPLE** today is per-function CFG cyclomatic + AST entropy — blind to
  complexity that lives *between* functions. Process length/participation make
  sprawling interprocedural flows visible.
- **COMPOSABLE** today is static fan-in/out + Martin instability. Community
  span / cross-community counts add *behavioral* (flow-level) coupling.
- **SECURE** today is **intra-file** CPG taint (single UAST root) — it cannot
  follow a sink across functions or files. `process.dangerous_flows` lifts
  reachability to flow level: a dangerous sink that actually lies on an
  execution flow from an entry point.

## 5. Integration with the lattice

Process metrics are scored by dedicated translators in
`topos/evaluation/policies/process.py` (`score_process_simple`,
`score_process_composable`, `score_process_secure`) and **merged** into each
pillar's `ScoredDecision` inside the dispatchers
(`topos/evaluation/characteristic_morphism.py`): `achieved` is AND-ed, `score`
is min-ed, interpretations are unioned. Existing scorers
(`simple.py` / `composable.py` / `secure.py`) are untouched. When no
`.gitnexus` index is present, no process rep is attached and behavior is
unchanged.

## 6. Calibration — PROVISIONAL

Gates/caps live in `ProcessPolicyThresholds` (`policies/calibration.py`) and
are **not yet empirically calibrated** (unlike the PyPI-ECDF-calibrated
SIMPLE/COMPOSABLE/SECURE singletons). SECURE uses zero-tolerance
(`max_dangerous_flows = 0`) consistent with the existing SECURE philosophy;
SIMPLE/COMPOSABLE gates are conservative starting points.

## 7. Open items / future work

- **Ordered taint.** The `step` index on `STEP_IN_PROCESS` enables true
  `source -> ... -> sink` ordering within a flow; v1 only checks
  membership of a dangerous step, not source-before-sink ordering.
- **Calibration.** Run the PyPI corpus to derive ECDF-based gates/caps for the
  three process axes, mirroring `CALIBRATION_REPORT.md`.
- **Branching/depth.** `processType` and the step graph could yield true
  branching factor and flow depth beyond raw step count.
