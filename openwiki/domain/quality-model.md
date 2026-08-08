---
type: Domain Model
title: Four-pillar quality model and verdict semantics
description: Defines Topos SIMPLE, COMPOSABLE, SECURE, and NAVIGABLE dimensions, 16-element lattice outcomes, gates, security acknowledgements, and advisory-analysis separation.
resource: /topos/engine/src/core/characteristic_morphism.rs
tags: [domain-model, quality, security, metrics, policies, rust]
openwiki:
  roles: [domain, architecture, testing]
  change_kinds: [policy, threshold, lattice, security]
  source_paths: [topos/engine/src/core/omega.rs, topos/engine/src/core/characteristic_morphism.rs, topos/engine/src/evaluation/policies/gates.rs, topos/engine/src/evaluation/policies/navigable.rs]
  symbols: [Generator, EvaluationValue, CharacteristicMorphism, score_navigable, evaluate_gates]
  test_paths: [topos/engine/src/evaluation/policies/navigable.rs, topos/engine/src/functors/probes/ast/divergence.rs]
  invariants: [IDEAL requires all four generators, priority changes guidance rather than gate thresholds, acknowledged security findings retain their raw score.]
  validation_commands: [cargo test -p topos-engine]
---

# Four-pillar quality model and verdict semantics

Topos evaluates four independent structural qualities. `Generator::ALL` contains SIMPLE, COMPOSABLE, SECURE, and NAVIGABLE; `EvaluationValue` encodes every satisfied subset as a four-bit value. That gives 16 outcomes: SLOP for no satisfied generator, BRONZE for one, SILVER for two, GOLD for three, and IDEAL for all four. The [analysis architecture](../architecture/overview.md) computes these values; [CLI and MCP workflows](../workflows/agent-and-cli.md) present and assess them.

| Pillar | Structural lens | Gate and intended signal |
| --- | --- | --- |
| **SIMPLE** | AST/UAST and CFG | Keep individual functions structurally understandable rather than merely keeping a file small. |
| **COMPOSABLE** | GitNexus MDG plus abstractness where supported | Keep module dependencies and abstraction relationships healthy. |
| **SECURE** | CPG | Avoid dangerous API reachability and taint flows. |
| **NAVIGABLE** | UAST scope tree | Keep the worst function’s block nesting within an agent-readable structural budget. |

`EvaluationValue::Ideal` is the top element only when all four bits are satisfied. The former three-pillar intersection is now `SimpleComposableSecure`, a deliberate v0.5.0 re-grade; do not label it IDEAL in output or tests. Priority and preference ordering steer guidance and target walks, not individual thresholds.

## SIMPLE

SIMPLE combines AST entropy with `ast.max_function_complexity`. The latter is a per-function/method UAST walk across all supported languages; it counts normal decisions, short-circuit logical operators, ternaries, Python comprehension clauses, `with`, `assert`, try handlers, and match/switch arms. A richer UAST mapping can cause failure at an unchanged threshold, so mapper and complexity tests move together.

Whole-file `cfg.cyclomatic` is surfaced for reporting and suggestions but does not independently gate SIMPLE: it grows with the number of simple functions. CFG semantics and the cross-language edge contract are maintained in the [architecture overview](../architecture/overview.md#uast-identity-and-traversal-contracts).

## COMPOSABLE

COMPOSABLE uses GitNexus-derived module relationships and metrics including coupling, fan-in/out, instability, and dependency depth. Where abstractness and actual coupling are available, policy uses Martin main-sequence distance `|A + I - 1|`; JavaScript retains instability-band behavior because its syntax does not offer equivalent abstract/interface declarations.

The stable declaration-leaf exemption prevents a genuinely concrete, low-instability declarations-only module from being treated as a design failure; executable calls and function/method declarations disqualify it. COMPOSABLE needs generated GitNexus state. Missing or rejected state leaves it unmeasured rather than failing the other three pillars; follow the [GitNexus runbook](../integrations/distribution.md#gitnexus-for-composable).

## SECURE

SECURE is a zero-findings gate over CPG dangerous-call and taint-flow evidence. The CPG joins AST, CFG, and PDG dependence families; PDG dataflow is deliberately a coarse textual-order approximation, not a full alias- or flow-sensitive proof. The MCP distribution embeds Sighthound for supplementary findings, while native CPG probes remain the local scoring path. See [Embedded Sighthound for SECURE](../integrations/distribution.md#embedded-sighthound-for-secure) when changing diagnostics or baselines.

### Security acknowledgements are disclosed, not silent overrides

A nearest-ancestor `.topos.toml` can contain scoped `[secure.allow]` entries, and CLI `--allow` supplies one-run acknowledgements. Every persistent entry requires a non-empty reason. The raw SECURE verdict and numeric score remain visible; an acknowledgement adjusts only the achieved state and lattice verdict, then caps the grade below PLATINUM. In MCP project rows, `scores.secure` and `pillars.secure.score` remain the raw measurement even when `pillars.secure.achieved` reflects an acknowledgement. Malformed configuration must not crash evaluation.

## NAVIGABLE

NAVIGABLE measures **Semantic Compositional Divergence** (SCD), not cyclomatic complexity. For scope-forming UAST nodes inside a callable, `function_divergence` adds `depth(u) * ln(1 + fanout(u))`; `calculate_max_function_divergence` reports the maximum callable value as `nav.max_function_divergence`. A flat function has divergence `0.0`; deep nested block scopes raise it. Sequential `if` statements can therefore have the same SIMPLE complexity as nested branches but remain more NAVIGABLE.

The scope set is `IfStmt`, loops, `MatchStmt`, `TryStmt`, `WithStmt`, and nested functions/methods. Conditional expressions and short-circuit `BinaryExpr` deliberately do not count because they open no block and would duplicate SIMPLE. `score_navigable` gates the max value at `NAVIGABLE.max_function_divergence` (currently `10.0`) and normalizes a reporting score separately. It is always measured for parseable source because `NavigableRepresentation` reads the UAST directly; no GitNexus or CPG prerequisite exists.

When changing this seam, update `functors/probes/ast/divergence.rs`, scope classification, `evaluation/policies/navigable.rs`, and gate metadata together as warranted. Keep the flat-versus-nested regression and the exact-threshold policy test passing. Do not replace the per-function max with a file-wide sum: a long file of flat functions is not the target of this pillar.

## Scoring versus advice

Gate-failure targets belong to scoring output. `topos coverage`, Graphify, compare, and refactor analysis supply useful structural evidence but deliberately do not add generators or change lattice membership. The [workflow page](../workflows/agent-and-cli.md#advisory-and-non-lattice-analysis) owns that public boundary.

## Policy change recipe

1. Identify the representation/probe under `topos/engine/src/{graphs,functors}/`.
2. Update a scorer in `evaluation/policies/` only for score shaping; use `gates.rs` for a decisive comparison, canonical interpretation, and operations.
3. Follow `CharacteristicMorphism::classify_detailed` if the input is always available or needs an external representation.
4. Keep output guidance, suggestions, and MCP formatting coherent, then run focused Rust regressions.
5. Run `cargo test --workspace` only when the shared result contract or public CLI/MCP surface changes; use [testing operations](../operations/testing-and-release.md) for conditional package checks.
checks.
