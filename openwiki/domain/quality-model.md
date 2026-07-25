---
type: Domain Model
title: Three-pillar quality model and verdict semantics
description: Defines Topos SIMPLE, COMPOSABLE, and SECURE dimensions, medal outcomes, policy boundaries, security acknowledgements, and advisory-analysis separation.
resource: /topos/engine/src/core/characteristic_morphism.rs
tags: [domain-model, quality, security, metrics, policies, rust]
---

# Three-pillar quality model and verdict semantics

Topos evaluates three independent structural qualities and combines achieved generators into an eight-outcome lattice. A file can pass one or two pillars without passing all three; GOLD/IDEAL requires SIMPLE, COMPOSABLE, and SECURE. The [analysis architecture](../architecture/overview.md) computes this model, then [CLI and MCP workflows](../workflows/agent-and-cli.md) present it.

| Pillar | Structural lens | Intended signal |
| --- | --- | --- |
| **SIMPLE** | AST/UAST and CFG | Avoid unnecessary structural/control-flow complexity |
| **COMPOSABLE** | MDG plus abstractness where supported | Keep module dependencies and abstraction relationships healthy |
| **SECURE** | CPG | Avoid dangerous API reachability and taint flows |

`README.md` defines medals: GOLD passes all three, SILVER passes two, BRONZE passes one, and SLOP passes none or cannot parse. Preferences alter remediation priority and output metadata, not fixed pass/fail thresholds.

## Pillar behavior

### SIMPLE

SIMPLE combines AST entropy with `ast.max_function_complexity`. The latter is a per-function/method UAST walk across all supported languages; it counts normal decision nodes, short-circuit logical operators, ternaries, Python comprehension `for`/filter clauses, `with`, `assert`, try handlers, and match/switch arms. A richer UAST mapping can therefore make code fail an unchanged max-function threshold; update mapper and complexity tests together.

Whole-file `cfg.cyclomatic` remains a scored/suggestion signal but does not independently gate SIMPLE because it grows with the number of otherwise-simple functions. CFG semantics and the cross-language edge contract are maintained in the [architecture overview](../architecture/overview.md#uast-and-graph-identity-contract).

### COMPOSABLE

COMPOSABLE uses GitNexus-derived module relationships and metrics including coupling, fan-in/out, instability, and dependency depth. Where abstractness and actual coupling are available, policy uses Martin main-sequence distance `|A + I - 1|`; JavaScript retains instability-band behavior because its syntax does not offer equivalent abstract/interface declarations.

The stable declaration-leaf exemption prevents a genuinely concrete, low-instability declarations-only module from being treated as a design failure; executable calls and function/method declarations disqualify it. COMPOSABLE requires generated GitNexus state, whose availability and freshness are runtime inputs; follow the [GitNexus runbook](../integrations/distribution.md#gitnexus-for-composable).

### SECURE

SECURE is a zero-findings gate over CPG dangerous-call and taint-flow evidence. The CPG joins AST, CFG, and PDG dependence families; PDG dataflow is deliberately a coarse textual-order approximation, so it is not a full alias- or flow-sensitive proof. The MCP distribution embeds Sighthound for supplementary findings; native CPG probes remain the stable local scoring path. Review [Sighthound behavior](../integrations/distribution.md#sighthound-for-secure) when changing diagnostics or baselines.

## Security acknowledgements are disclosed, not silent overrides

A nearest-ancestor `.topos.toml` can contain scoped `[secure.allow]` entries, and CLI `--allow` supplies one-run acknowledgements. Every persistent entry requires a non-empty reason. The canonical raw SECURE verdict remains visible; acknowledgements are disclosed and cap the grade below GOLD/IDEAL. Malformed configuration should not crash evaluation.

## Scoring versus advice

Gate-failure targets belong to scoring output. In contrast, Graphify/refactor analysis identifies structural hotspots or orphans but deliberately does not change SIMPLE, COMPOSABLE, or SECURE. The [workflow page](../workflows/agent-and-cli.md#advisory-analysis) describes this boundary.

## Maintenance checklist

1. Locate the representation and probe under `topos/engine/src/{graphs,functors}/`.
2. Update the policy and centralized gate/calibration behavior in `topos/engine/src/evaluation/`.
3. Keep suggestions, interpretations, security guidance, and refactor targets consistent.
4. Add focused Rust regression tests, then run the workspace checks in [testing operations](../operations/testing-and-release.md).
5. Test integration-present and graceful-degradation paths when COMPOSABLE or SECURE behavior changes.
