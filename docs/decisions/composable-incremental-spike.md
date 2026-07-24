# COMPOSABLE incremental scoring — spike

Status: **DEFERRED** (gated for Task #13). This document records why the
"recompute the proposed file's own outbound edges from its AST" approach
was investigated and *not* implemented, plus the concrete path to a full
implementation.

> **Migration note (v0.4.0, PR [#159](https://github.com/Krv-Labs/topos/pull/159)):**
> this spike was written against the pre-migration Python codebase, and the
> `topos/....py:LINE` citations below are historical — that code no longer
> exists. Re-verified after the Rust rewrite: the two gaps that drove the
> DEFER decision still hold in `topos-core` today — there is no import/
> outbound-reference extraction probe anywhere under
> `crates/topos-core/src/functors/probes/`, and `ModuleDependencyGraph`
> (`crates/topos-core/src/graphs/mdg/object.rs`) still has no per-node edge
> **replace** API (only `add_node`/`add_relationship`, which append). The
> Rust equivalents of the modules cited below are: `topos/mcp/tools/assess.py`
> → `crates/topos-mcp/src/tools/assess.rs`; `topos/mcp/evaluation.py`
> (`load_dep_graph`) → `crates/topos-mcp/src/evaluation.rs`;
> `topos/evaluation/policies/composable.py` (`score_coupling`) →
> `crates/topos-core/src/evaluation/policies/composable.rs`;
> `topos/graphs/mdg/object.py` → `crates/topos-core/src/graphs/mdg/object.rs`;
> `topos/functors/probes/mdg/{coupling,fan}.py` →
> `crates/topos-core/src/functors/probes/mdg/{coupling,fan}.rs`;
> `topos/graphs/uast/mapper_python.py` →
> `crates/topos-core/src/graphs/uast/mapper_python.rs`. The conclusion —
> **DO NOT BUILD** until the measurement gate below clears — is unchanged.

## Goal that was investigated (Task #11)

`topos_assess_improvement` (`topos/mcp/tools/assess.py`) scores the proposed
code's COMPOSABLE generator against a **static** `.gitnexus`
`ModuleDependencyGraph` (MDG) snapshot loaded via
`load_dep_graph(gitnexus_dir, filepath)` (`topos/mcp/evaluation.py`). The
docstring already admits this is an approximation: the proposed file's coupling
is read from the *pre-refactor* dep graph. The aim was to make COMPOSABLE
accurate for the common case — an agent editing imports **within** the file
under test — by recomputing only the proposed file's own outbound edges from
its already-parsed AST and scoring against a patched MDG (inbound edges /
sibling files left from the snapshot — that is the deferred cross-file case).

## STEP 1 — how COMPOSABLE is actually scored

The chain, end to end:

1. `classify_morphism(morphism, priority, dep_graph)` (`evaluation.py`) appends
   the `ModuleDependencyGraph` to the representation list and calls
   `CharacteristicMorphism.classify_detailed`.
2. The classifier reads `mdg.metrics()` and passes the three values to
   `score_coupling(instability, fan_in, fan_out)`
   (`topos/evaluation/policies/composable.py`) — see
   `characteristic_morphism.py:103-106`.
3. `ModuleDependencyGraph.metrics()` (`topos/graphs/mdg/object.py:379`) finds
   the target File node, gathers all contained symbol IDs, and computes:
   - `mdg.coupling` / `mdg.instability` via
     `calculate_coupling` + `calculate_instability_from_result`
     (`topos/functors/probes/mdg/coupling.py`).
   - `mdg.fan_in` / `mdg.fan_out` via `calculate_fan_in_out`
     (`topos/functors/probes/mdg/fan.py`).
   - `mdg.dep_depth` via `calculate_dependency_depth`.

Edge-type semantics (this is the crux):

- **instability** = `Ce / (Ca + Ce)`. `Ce` (efferent) = count of **distinct
  target `File` nodes** reached by **`IMPORTS`** edges out of this file's
  symbols. So instability *is* import-driven.
- **fan_out** = count of distinct external symbols reached by **`CALLS`**
  edges. **fan_out is NOT import-driven** — editing `import` lines does not move
  fan_out unless the call sites change and are re-resolved.
- **fan_in** (inbound `CALLS`) and `Ca` (inbound `IMPORTS`) are the cross-file
  inbound edges that this task explicitly leaves to the snapshot.

The `score_coupling` policy gates COMPOSABLE on three independent AND'd
thresholds (`ComposablePolicyThresholds` in
`topos/evaluation/policies/calibration.py`): `instability ∈ [0.3, 0.7]`,
`fan_in ≤ 15`, `fan_out ≤ 15`.

## STEP 1 — what import-extraction / MDG-patch APIs exist

Both halves needed for a correct in-place patch are **absent**:

### (a) No import / outbound-reference extraction from the UAST

The UAST mappers do not recognize imports. `topos/graphs/uast/mapper_python.py`
maps `function_definition`, `class_definition`, calls, etc., but Python
tree-sitter's `import_statement` / `import_from_statement` are in **none** of
`_DECLARATION_TYPES` / `_STATEMENT_TYPES` / `_EXPRESSION_TYPES`, so they fall
through `map_node_kind` to `kind="Unknown"`. The same is true for the other
language mappers. There is no `extract_imports`, no import probe under
`topos/functors/probes/`, and no AST-level outbound-reference collector
anywhere in `topos/`.

### (b) No per-node edge override on the MDG, and no internal dep-graph builder

`ModuleDependencyGraph` (`topos/graphs/mdg/object.py`) exposes
`add_node` / `add_relationship` and lookup helpers, but **no** API to *replace*
a single file's outbound edge set. More importantly, the entire MDG is
**consumed from external GitNexus output** — `topos depgraph generate`
(`topos/cli/commands/system.py:128`) shells out to `gitnexus analyze` via
`subprocess`. Topos has **no internal dependency-graph builder** and no
symbol-resolution layer of its own.

## STEP 2 — DECISION: DEFER

A correct in-place patch is **not** achievable with a minimum diff using
existing APIs. To recompute even the import-driven half (instability), a patch
would have to:

1. **Build import extraction that does not exist.** Add import recognition to
   every UAST mapper (or a separate AST probe), covering `import`, `from … import`,
   relative imports, aliases, and re-exports, for every supported language.
2. **Reimplement cross-file symbol resolution** — the hard part GitNexus owns.
   `Ce` counts distinct **target `File` nodes**, so an extracted import name
   (`from topos.x.y import Z`) must be resolved to the MDG `File` node ID for
   `topos/x/y.py`. That requires a module-path → File-node resolver that
   accounts for packages, `__init__.py`, namespace packages, src layouts, and
   third-party vs. first-party (third-party imports have no File node, so they
   must be excluded from `Ce` exactly as GitNexus does). Getting this subtly
   wrong yields a **wrong instability** — and a wrong COMPOSABLE verdict is
   worse than today's honest, documented approximation.
3. **fan_out would still be stale regardless.** fan_out is `CALLS`-based, not
   import-based. Recomputing it from the AST needs call-target resolution
   across files — strictly harder than import resolution and squarely in the
   deferred cross-file domain. So the task's framing of "fan-out / instability"
   as same-file-editable is only half right: editing imports moves instability
   (via `Ce`), but does **not** move fan_out.

This is the "don't build infra for an unmeasured problem" call. Reimplementing
GitNexus's import + symbol resolution inside topos to patch one file's edges is
non-trivial new infrastructure, not a minimum diff. The current approximation
is already honestly disclosed in the `topos_assess_improvement` docstring, the
`coupling_available_for_proposed` flag, and the `gitnexus_warnings` (including
the stale-index warning). It stays in place.

## Path to a full implementation (for Task #13)

If/when this is prioritized, the correct shape is an **incremental MDG patch**,
not an AST re-derivation bolted onto scoring:

1. **Import probe.** Add a first-class import/outbound-reference extractor —
   either by mapping import nodes in the UAST mappers to a new `ImportDecl`
   kind, or a dedicated `topos/functors/probes/<lang>/imports.py`. Return raw
   module specifiers per file. (This is independently useful.)
2. **Module-path resolver.** A first-party module-path → `File`-node-ID
   resolver over the existing MDG `File` nodes (match on `filePath`
   properties, like `ModuleDependencyGraph.file_node_id`). Third-party
   specifiers that resolve to no File node are dropped (mirrors `_owning_file`
   returning `None`).
3. **Targeted edge override on the MDG.** Add a supported method, e.g.
   `mdg.replace_outgoing(file_node_id, rel_type, target_file_ids)`, that
   removes this file's existing outgoing edges of that type and inserts the
   recomputed ones — keeping `_outgoing` / `_incoming` indexes consistent.
   Patch only **outbound** `IMPORTS` (and, if call resolution is added,
   `CALLS`); leave all inbound edges and sibling files untouched so `Ca` /
   fan_in stay from the snapshot.
4. **Wire into the proposed path only.** In `assess.py`, after parsing the
   proposed AST, clone-or-patch the dep graph and score the proposed morphism
   against the patched MDG. The baseline (`current`) must keep scoring against
   the unpatched snapshot. Guard so the patch never touches the baseline path
   and `coupling_available_for_proposed` semantics are preserved.
5. **Tests.** Prove that editing the proposed file's imports changes its
   `mdg.instability` (and `Ce`) scoring versus the stale snapshot, that
   `fan_in` / `Ca` are unchanged, and that third-party imports do not inflate
   `Ce`.

Acceptance bar for shipping: the patched instability must equal what a full
`gitnexus analyze` would produce for the edited file's outbound imports.
Until that correctness can be demonstrated, the approximation stays.

## Should we build this? — measurement gate (Task #13)

The path above is real work (a first-party import probe + a re-implementation
of GitNexus's module resolution). Before committing to it, prove the problem is
worth the infrastructure. Build only if the data clears this gate:

1. **Quantify the error, don't assume it.** Take a handful of real
   multi-file refactor sessions. After each edit, score the proposed file two
   ways: (a) against the stale snapshot (today's behavior), and (b) against a
   freshly regenerated `gitnexus analyze`. Record how often the **COMPOSABLE
   verdict bit** differs (not just the instability float — a 0.02 drift that
   never crosses the [0.3, 0.7] gate is harmless).
2. **Gate threshold.** Greenlight the full build only if verdict-flips occur in
   a non-trivial fraction of edits (suggested bar: ≥ ~10% of assess calls in a
   typical session, or any flip that misleads an agent into a wrong refactor).
   A drift that almost never crosses a gate boundary does not justify
   reimplementing symbol resolution.
3. **Cheaper alternatives to weigh first** (likely better ROI than the patch):
   - **Staleness nudge.** When the `.gitnexus` snapshot mtime predates the
     proposed file's edits, emit a warning suggesting `topos depgraph generate`
     — cheap, honest, already half-present via `gitnexus_warnings`.
   - **Fast full regen.** If `gitnexus analyze` is fast enough on the repo,
     just regenerate on demand instead of patching in-memory — correctness for
     free, no resolver to maintain.

**Current recommendation: DO NOT BUILD.** The cost (owning a module/symbol
resolver that must track GitNexus exactly) is high, the error is unmeasured, and
the approximation is already disclosed to the agent. Run the measurement above
first; if it clears the gate, prefer the "fast full regen" alternative before
the in-memory patch.
