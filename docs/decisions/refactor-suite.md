# Refactoring Suite: Cycles, Dependencies, Process

Three advisory targets exposed as one MCP tool,
`topos_refactor(target="cycles"|"dependencies"|"process", ...)`.
The MCP surface is collapsed into a single tool — rather than three —
specifically to stay under the tool-definition wire-size ratchet: each
separate tool would carry its own self-contained `outputSchema`, multiplying
the embedded `RefactorHotspot` schema on the wire. One tool means one embed.

All three implement the Methods Upgrade milestone
(issues [#83](https://github.com/Krv-Labs/topos/issues/83),
[#84](https://github.com/Krv-Labs/topos/issues/84),
[#86](https://github.com/Krv-Labs/topos/issues/86)).

**None of this feeds SIMPLE/COMPOSABLE/SECURE/NAVIGABLE scoring.** All three
targets are read-only, advisory refactoring guidance — a separate concern from
`topos evaluate`'s medal computation. This is distinct from the
`RefactorTarget`/`refactor_targets` parameter on `topos_evaluate_file`,
which surfaces *gate-failure* edit targets computed from the scoring
pipeline itself. The tools documented here apply independent
structural-analysis engines (cycle-basis extraction, discrete Ricci
curvature) that don't participate in scoring at all.

The tool's in-code docstring and schema field set are deliberately terse for
the same wire-size reason — see `topos/mcp/src/schemas.rs`'s
`RefactorInput`/`RefactorResult`/`RefactorHotspot`. This document carries
the fuller explanation that didn't fit.

## Shared input/result shape (MCP)

`RefactorInput`: `target` (`"cycles"` | `"dependencies"` | `"process"`,
required), `filepath` (required), `gitnexus_dir` (optional, ignored for
`target="cycles"`), `limit` (default 5, 1–50 — caps how many hotspots come
back).

`RefactorResult`: `target`, `filepath`, `betti_1` (set only for
`target="cycles"`), `gitnexus_available` (set only for `dependencies`/
`process`), `tool_available` (set for `dependencies`/`process`, where it's
always identical to `gitnexus_available`, kept as a separate field so a wire
consumer has one generic "is-the-backing-tool-available" name across targets
instead of needing to know which per-target availability field applies),
`hotspots`, `error`.

Every result carries a ranked list of `RefactorHotspot` rows:

| Field | Meaning |
| --- | --- |
| `kind` | `"cycle"` \| `"dependency_edge"` \| `"process_transition"` |
| `label` | Human-readable identifier (block list, `source -> target` edge, or node label) |
| `filepath` | The file the hotspot is associated with |
| `line_start` / `line_end` | Source range, when known (cycles only today) |
| `score` | Betti contribution (cycles) or curvature value (dependencies/process). **Sign matters for curvature: more negative means a stronger bottleneck/choke-point signal.** Cycles use a non-negative "span" score (larger = bigger hotspot). Don't assume a uniform sign convention across targets. |
| `suggestion` | An imperative, one-line refactor suggestion |

(An earlier revision also carried a per-hotspot `evidence` dict of raw
supporting detail — dropped to claw back wire-size margin; `label` already
carries the same information, e.g. the block-id list or the two endpoint
ids, just formatted as text instead of structured key/value pairs.)

Hotspots are always returned pre-sorted so the most actionable row is first
(largest cycle span; most negative curvature).

## `topos refactor cycles` / `topos_refactor(target="cycles")` (issue #83)

**What it replaces:** cyclomatic complexity (`cfg.cyclomatic`, `E - N + 2P`)
is a single summary number — it tells you *how many* independent cycles a
function's control flow has, but not *which* cycles, or where they live in
the source.

**Algorithm:** a fundamental cycle basis is extracted from the CFG via a
spanning tree + back-edge closure (`topos/engine/src/functors/probes/cfg/homology.rs::compute_cycle_basis`,
O(V+E)). Every non-tree ("back") edge closes exactly one cycle — the tree
path between its endpoints plus the back edge itself. The count of these
cycles (`betti_1`, the rank of the cycle space / dim H1) is provably equal
to `cyclomatic_complexity() - 1` for the single-connected-component CFGs
this builder always produces; that equality is asserted directly in
`topos/engine/src/functors/probes/cfg/homology.rs`'s test suite as a cross-check against the already-trusted
cyclomatic-complexity metric.

**Why not true persistent homology?** The issue's title mentions "persistent
homology," but a full treatment would require a filtration (e.g. by
betweenness centrality, as floated in the original issue discussion) and a
boundary-matrix reduction to produce birth/death pairs. That's substantially
more engineering (a new centrality algorithm, no existing linear-algebra or
simplicial-complex dependency in this crate) for a payoff — birth/death
pairs — that doesn't map to anything actionable in a code-quality tool: a
cycle "born at edge-rank 7, dying at edge-rank 12" isn't something an agent
can act on, whereas "this loop spans lines 40–58" is. Cyclomatic complexity
already *is* the Betti number under the trivial (no) filtration, so the
real gap this tool fills is cycle generators mapped to source, which
fundamental-cycle-basis extraction gives directly. This scope decision was
confirmed with the user before implementation.

**Source mapping:** each cycle's block-id list is mapped through
`BasicBlock.statements[].span.{start_line,end_line}` to a `(min start_line,
max end_line)` range — the smallest source span that covers every statement
in every block the cycle touches.

**Params:** `filepath` (required), `limit` (default 5, 1–50) caps how many
hotspots come back, ranked by span size (`end_line - start_line`)
descending. `gitnexus_dir` is accepted but ignored for this target.

## `topos refactor dependencies` / `topos_refactor(target="dependencies")` (issue #84)

**Engine:** balanced Forman curvature (Topping, Di Giovanni, Chamberlain,
Dong & Bronstein, "Understanding over-squashing and bottlenecks on graphs
via curvature," ICLR 2022, [arXiv:2111.14522](https://arxiv.org/abs/2111.14522),
Definition 1) applied to the file-level MDG dependency graph
(`topos/engine/src/functors/curvature.rs::balanced_forman_curvature`). The paper uses this curvature to
find GNN message-passing bottlenecks — edges with very negative curvature
"squash" information from exponentially many distant neighborhoods through
a single transition. Applied to module dependencies instead of message
passing, very negative curvature flags a load-bearing import edge: many
otherwise-unrelated modules route their coupling through this one
dependency.

The implementation is ported directly from the paper authors' reference
code ([jctops/understanding-oversquashing](https://github.com/jctops/understanding-oversquashing),
`gdl/src/gdl/curvature/numba.py::_balanced_forman_curvature`) rather than
re-derived from the paper's set-builder notation, because the 4-cycle
("sharp"/"lambda") term has indexing subtleties that are easy to get wrong
from prose alone — verified by hand-deriving the exact expected curvature
value for a triangle graph (2.0) and cross-checking against the ported
algorithm's output in `topos/engine/src/functors/curvature.rs`'s test suite. Per the paper, curvature is
0 for any "pendant" edge (an endpoint of degree 1) — the raw
degree/triangle/4-cycle formula degenerates to a spurious large value at
degree-1 endpoints otherwise.

**Graph construction:** builds the *whole project's* file-level dependency
graph (resolving symbol-level `IMPORTS` edges up to their owning `File`
nodes, mirroring `topos.functors.probes.mdg.coupling`'s approach) rather
than a truncated one-file neighborhood, so curvature at each edge reflects
its true local structure. Results are then filtered to edges touching the
requested file.

**Params:** `filepath` (required), `gitnexus_dir` (optional, auto-detected
from `<project_root>/.gitnexus` when omitted — matching
`topos_evaluate_file`'s behavior), `limit` (default 5, 1–50).
`gitnexus_available: false` is returned (no error) when no `.gitnexus`
graph is present, the same graceful-degradation contract
`topos_evaluate_file` uses for COMPOSABLE.

**Scope note:** this implements *plain* balanced Forman curvature, which
already gives the triangle + 4-cycle terms from Topping et al.'s Definition
1. It does not implement anything from the paper's rewiring algorithm
(SDRF) beyond the curvature computation itself — SDRF is a *graph-editing*
procedure (add/remove edges to flatten curvature), which is out of scope
for an advisory reporting tool.

## `topos refactor process` / `topos_refactor(target="process")` (issue #86)

**Engine:** directed Forman-Ricci curvature (Samal et al.) applied to
GitNexus process graphs (`topos/engine/src/functors/curvature.rs::directed_forman_curvature`):

```
Ric(e = u -> v) = w_e * ( w_u/w_e + w_v/w_e
                          - sum_{e_in ~ u}  sqrt(w_u / w_e_in)
                          - sum_{e_out ~ v} sqrt(w_v / w_e_out) )
```

where `e_in` ranges over edges incoming to `u` and `e_out` ranges over
edges outgoing from `v`. Node/edge weights default to uniform `1.0` — no
call-frequency or timing data exists in GitNexus's schema today, so this
degenerates to unweighted directed Forman curvature while keeping the
weighted formula's shape so real weights (e.g. call frequency) can be
plugged in later without an API change. Highly negative curvature flags a
"choke point": a single transition where many independent execution paths
(high in-degree at `u` and/or high out-degree at `v`) funnel through one
edge.

**Data source:** `topos.graphs.process.object.ProcessGraph`, built by
reusing `ModuleDependencyGraph`'s existing ladybug-loading machinery
(including `LadybugSchemaMismatchError` handling) rather than opening a
second connection to `.gitnexus/lbug` — a `ProcessGraph` is a full MDG load
filtered down to `Process` nodes and `STEP_IN_PROCESS` relationships, with
steps ordered by the relationship's `step` property (falling back to
discovery order if a given `.gitnexus` build doesn't carry that property —
see the docstring on `ProcessGraph.from_mdg` for the exact fallback logic).

**Params:** `filepath` (required), `gitnexus_dir` (optional, same
auto-detection as `dependencies`), `limit` (default 5, 1–50). Same
`gitnexus_available: false` graceful-degradation contract. Curvature is
computed only over the process paths that touch the requested file
(`ProcessGraph.paths_touching_file`), not the whole project's process
graph.

**Suggested action:** for a negative-curvature ("choke point") transition,
the suggestion text recommends an asynchronous decoupling boundary (message
queue / pub-sub) between the two steps — or, per the issue's framing,
consciously keeping the simpler synchronous coupling once the trade-off is
visible.

## Shared Rust engine (`topos/engine/src/functors/curvature.rs`)

Both curvature variants share one adjacency-indexing layer
(`AdjacencyIndex`): node ids are interned to dense indices, and neighbor
sets/in-out edge lists are built once per call from a raw
`Vec<WeightedEdge>`. No `petgraph` graph type is used here — unlike
`topos/engine/src/graphs/cfg/object.rs`, neither formula needs path/component algorithms, only
neighbor lookups, so a `HashMap`/`HashSet`-based adjacency index is simpler
and avoids `NodeIndex` bookkeeping. `balanced_forman_curvature` is
undirected (every edge populates both endpoints' neighbor sets; duplicate/
reciprocal input edges collapse to one result per unordered pair) and
**ignores edge weights** (the formula is purely combinatorial — degree,
triangle count, 4-cycle term). `directed_forman_curvature` keeps separate
in/out adjacency and **does** use edge weights (`w_e`) and optional node
weights.

Complexity: `balanced_forman_curvature` is O(deg(u)·deg(v)) per edge in the
worst case (triangle/4-cycle counting via neighbor-set intersection) —
noticeably more expensive than `directed_forman_curvature`'s O(deg(u) +
deg(v)) per edge (pure degree-sum, no triangle counting). The directed
engine cleared issue #86's acceptance bar (10k nodes / 50k edges in
<100ms) against the pre-migration Python implementation via
`tests/benchmarks/test_curvature_perf.py` (`TOPOS_BENCHMARK=1`); that
opt-in benchmark was not ported to the Rust `topos-core` test suite, and
the bound has not been re-verified against the (structurally identical,
but compiled) Rust port. The undirected engine has no equivalent hard
acceptance criterion but was benchmarked informally at similar scale during
development with no pathological blowup on non-hub-heavy graphs.

## Testing notes

- Rust (`cycles`/`dependencies`/`process`): inline `#[cfg(test)]` in
  `topos/engine/src/functors/curvature.rs` and
  `topos/engine/src/functors/probes/cfg/homology.rs` — includes a
  hand-derived exact curvature
  value (triangle graph, 2.0), bottleneck/choke-point detection on
  synthetic bowtie/bridge graphs, leaf-edge zero-curvature special case,
  and the betti_1/cyclomatic-complexity cross-check.
- All three targets are genuinely advisory: none of their probes are called
  from `ControlFlowGraph::metrics()` or `ModuleDependencyGraph::metrics()`,
  so nothing they compute can reach the scoring pipeline.

## Removed target: `graphify` (issue #325)

A fourth target, `graphify`, shipped in v0.4.0 under issue
[#150](https://github.com/Krv-Labs/topos/issues/150): orphan-node and
fragile-edge detection over a knowledge graph built by
[Graphify](https://github.com/Graphify-Labs/graphify), plus a
`topos_generate_graphify_graph` MCP tool and a `topos graphify
generate|orphans` CLI subcommand. It was removed in full under issue
[#325](https://github.com/Krv-Labs/topos/issues/325).

The reason is that it was not complementary to the graph Topos already
loads for COMPOSABLE — it was a strictly poorer subset of it. Every signal
Graphify provided has a GitNexus/MDG equivalent at equal or better fidelity:
full `startLine`/`endLine` spans rather than point anchors, a first-class
`Community` node label with `MEMBER_OF` edges rather than a bare Louvain
`community` field, a continuous `confidence: f64` plus `reason` rather than a
three-value `AST`/`INFERRED`/`AMBIGUOUS` enum, and symbol degree over
`CALLS`/`IMPORTS` rather than raw knowledge-graph degree. Its one genuine
differentiator — sub-file structure for non-code artifacts, such as markdown
headings as individual nodes — feeds no pillar and no plausible future pillar,
since all four generators are code-structural.

Supporting evidence for the removal: the parsed `source_location` field had no
production consumer; no CI workflow regenerated or validated a graph, so a
schema break would have shipped silently; and no agent-facing doc recommended
when to reach for it. The dependency was pre-1.0 with a documented history of
schema breaks (the `links`/`edges` key flip across 190+ releases), which the
parser had to defend against.

Removing it took the MCP tool surface from 18 tools / 39,584 chars to 17 tools
/ 38,668 chars, ratcheting the context budget in
`topos/mcp/src/context_budget.rs` **down** — every agent session pays for the
tool-definition wire format on every request, so a removed tool is a permanent
context saving.
