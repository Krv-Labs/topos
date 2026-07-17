---
type: Domain Model
title: Three-pillar quality model and verdict semantics
description: Defines Topos SIMPLE, COMPOSABLE, and SECURE dimensions, medal outcomes, policy boundaries, file-role exceptions, and disclosed security acknowledgements.
resource: /topos/evaluation/characteristic_morphism.py
tags: [domain-model, quality, security, metrics, policies]
---

# Three-pillar quality model and verdict semantics

Topos evaluates three independent structural qualities and combines achieved generators into an eight-outcome lattice. A file can pass one or two pillars without passing all three; GOLD/IDEAL requires SIMPLE, COMPOSABLE, and SECURE. This model is computed by the [architecture pipeline](../architecture/overview.md), then presented through [CLI and MCP workflows](../workflows/agent-and-cli.md).

| Pillar | Structural lens | Intended signal |
| --- | --- | --- |
| **SIMPLE** | AST and CFG | Avoid unnecessary structural/control-flow complexity |
| **COMPOSABLE** | MDG plus abstractness where supported | Keep module dependencies and abstraction relationships healthy |
| **SECURE** | CPG | Avoid dangerous API reachability and taint flows |

`README.md` defines medals: GOLD passes all three, SILVER passes two, BRONZE passes one, and SLOP passes none or cannot parse. The priority/preference setting changes remediation emphasis and output metadata; fixed policy thresholds determine pass/fail.

## Pillar behavior

### SIMPLE

SIMPLE combines AST entropy with CFG metrics such as cyclomatic complexity and maximum function complexity. It reflects structural complexity, not test coverage or runtime behavior. File-role predicates relax relevant gates for import/export-style entrypoint modules so lightweight package hubs are not penalized merely for their role (`topos/evaluation/file_roles.py`).

### COMPOSABLE

COMPOSABLE uses GitNexus-derived module relationships and metrics including coupling, fan-in/out, instability, and dependency depth. For Python, Rust, Go, TypeScript, and C++ where abstractness is available and coupling has real signal, policy uses Martin main-sequence distance `|A + I - 1|` rather than raw instability alone. JavaScript retains the instability-band behavior because its native syntax does not supply equivalent abstract/interface declarations.

The stable declaration-leaf exemption prevents a genuine concrete, low-instability declarations-only module from being treated as a design failure; executable calls and function/method declarations disqualify the exemption. These choices were introduced in 0.3.11 and the new thresholds are explicitly provisional (`CHANGELOG.md`). Any policy change must update `tests/evaluation/`, especially gate parity coverage.

COMPOSABLE requires a generated GitNexus store. Its availability, freshness, and schema status are a real runtime input, not a scoring afterthought; follow the [GitNexus runbook](../integrations/distribution.md#gitnexus-for-composable).

### SECURE

SECURE is a zero-findings gate over `cpg.dangerous_calls` and `cpg.taint_flows`. CPG metrics use Sighthound JSON findings when the executable is on `PATH`; otherwise local dangerous-API and taint-path probes run. This means the metric names remain stable while finding provenance can differ; review [Sighthound behavior](../integrations/distribution.md#sighthound-for-secure) when changing security diagnostics or baselines.

## Security acknowledgements are disclosed, not silent overrides

A nearest-ancestor `.topos.toml` can contain scoped `[secure.allow]` entries, and CLI `--allow` supplies one-run acknowledgements. Every persistent entry requires a non-empty `reason`. The canonical raw SECURE verdict is still computed from the complete registry; acknowledgements are shown as such and cap the grade below GOLD/IDEAL. Configuration is intentionally best-effort: malformed files do not crash evaluation.

This anti-gaming policy lives in `topos/config.py` and `topos/evaluation/suppression.py`. Preserve it when adding patterns, output fields, or MCP result shaping.

## Scoring versus advice

Topos has two related but distinct improvement paths:

- **Gate-failure targets** are derived from the scoring pipeline and can be returned by MCP evaluation (`refactor_targets`). They name source spans and recommended operations for failed gates.
- **Advisory refactor analyses** find CFG cycle bases, MDG dependency curvature, or process-graph choke points. They are useful guidance but intentionally do not feed SIMPLE, COMPOSABLE, or SECURE.

The [workflow page](../workflows/agent-and-cli.md#advisory-refactoring) explains how agents should use both without confusing advice with a verdict.

## Maintenance checklist

When modifying a metric or policy:

1. Locate the representation and its probe under `topos/graphs/` / `topos/functors/`.
2. Update the applicable policy and shared gate specification in `topos/evaluation/policies/`.
3. Confirm suggestions, security guidance, interpretations, and refactor targets remain consistent with the centralized gate/security tables.
4. Add regression tests in `tests/evaluation/`, `tests/functors/`, or `tests/mcp/` as appropriate.
5. Test both with and without optional integrations when changing COMPOSABLE or SECURE behavior.
