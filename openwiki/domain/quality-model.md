---
type: domain model
title: Quality Model & Categorical Engine
description: Category-theoretic domain model for code quality, graph representations, Heyting algebra subobject classifier, evaluation functors, metric probes, profunctors, and policy gates in Topos.
tags: [domain-model, quality, security, metrics, policies, rust, graph-primitives, functors]
verified:
  - by: openwiki/0.9.0
    at: 2026-11-25T22:07:40.661Z
sources:
  - id: openwiki-source-ba5fb5e64f76cdc661dea47e
    resource: repo://topos/cli/src/commands/coverage.rs
  - id: openwiki-source-c386b4993bec7b31315a096c
    resource: repo://topos/engine/src/config.rs
  - id: openwiki-source-44098145b30ab92fb4bf3c70
    resource: repo://topos/engine/src/core/characteristic_morphism.rs
  - id: openwiki-source-195e23ac57e29d66773b4152
    resource: repo://topos/engine/src/core/omega.rs
  - id: openwiki-source-65f248533e31febde31d40f9
    resource: repo://topos/engine/src/evaluation/advisory.rs
  - id: openwiki-source-f1579178803426a3efb389b3
    resource: repo://topos/engine/src/evaluation/policies/base.rs
  - id: openwiki-source-a3937f767b8ba9d5a5c1a0bc
    resource: repo://topos/engine/src/evaluation/policies/calibration.rs
  - id: openwiki-source-18d7b316c44f96018cba22ec
    resource: repo://topos/engine/src/evaluation/policies/composable.rs
  - id: openwiki-source-91e5f73e818cc0458b33933b
    resource: repo://topos/engine/src/evaluation/policies/desirability.rs
  - id: openwiki-source-fe927feb706cdcb99e08620e
    resource: repo://topos/engine/src/evaluation/policies/gates.rs
  - id: openwiki-source-84ff6fa568804226649607a5
    resource: repo://topos/engine/src/evaluation/policies/navigable.rs
  - id: openwiki-source-2f088eae38a9057854c427ef
    resource: repo://topos/engine/src/evaluation/policies/secure.rs
  - id: openwiki-source-a61af37fbe67577dec1a27f3
    resource: repo://topos/engine/src/evaluation/policies/simple.rs
  - id: openwiki-source-21b1292681706988930db50e
    resource: repo://topos/engine/src/evaluation/preferences.rs
  - id: openwiki-source-56387a0ca7b6f70787282c46
    resource: repo://topos/engine/src/evaluation/suppression.rs
  - id: openwiki-source-e99769a989da62cadb7b3f68
    resource: repo://topos/engine/src/functors/probes/ast/divergence.rs
  - id: openwiki-source-d2e755265f68157a7939a2d3
    resource: repo://topos/mcp/src/diagnostics.rs
  - id: openwiki-source-991cd0896a9f23615c66e82e
    resource: repo://topos/mcp/src/evaluation/analysis.rs
  - id: openwiki-source-95838d4cc7205bfd5c485808
    resource: repo://topos/mcp/src/tools/refactor.rs
generated: { by: "openwiki/0.9.0", at: "2026-11-25T22:07:40.661Z" }
---

Topos expresses source code quality through category-theoretic principles using four independent generator atoms: **SIMPLE**, **COMPOSABLE**, **SECURE**, and **NAVIGABLE**. In the category of programs $\mathbf{E} = \mathbf{Set}^{\mathbf{C} \times \mathbf{H}^{\text{op}}}$, source code functions as a `ProgramMorphism` between computational states. The evaluation lattice codomain $\Omega$ acts as a subobject classifier (a 16-element Heyting algebra), while `CharacteristicMorphism` computes the unique characteristic map $\chi_S : P \to \Omega$ that evaluates each program morphism against raw-metric gate thresholds.

An `EvaluationValue` represents the subset of generator atoms that pass their raw-metric gates. The satisfied subset forms a 16-element lattice rather than a continuous score scale. An `IDEAL` verdict indicates that all four generators are satisfied and corresponds to the **PLATINUM** medal tier. Satisfying three, two, one, or zero pillars yields **GOLD**, **SILVER**, **BRONZE**, and **SLOP** respectively. For example, satisfying `SIMPLE`, `COMPOSABLE`, and `SECURE` yields `SIMPLE_COMPOSABLE_SECURE` (GOLD), which is distinct from `IDEAL`.

Because generator atoms are incomparable in the Heyting algebra, consumers requiring lattice comparisons must use `Omega::leq` rather than bitwise or integer operations. `Omega` also implements generic Heyting lattice operations (meet, join, relative pseudo-complement) and aggregate operations (`Omega::aggregate`), where an empty aggregate evaluates to `IDEAL` by empty-meet convention.

| Pillar | Canonical Input and Gate Threshold | Representation Availability |
| --- | --- | --- |
| **SIMPLE** | `ast.entropy` in `0.2..=0.8` and `ast.max_function_complexity <= 10` | Every parseable source file |
| **COMPOSABLE** | `mdg.fan_out <= 10` | When Module Dependency Graph (MDG) observations are attached |
| **SECURE** | `cpg.dangerous_calls == 0` and `cpg.taint_flows == 0` | When Code Property Graph (CPG) observations are attached |
| **NAVIGABLE** | `nav.max_function_divergence <= 10.0` | Every parseable source file |

For broader architecture context, see the [Architecture Overview](../architecture/overview.md), [Agent and CLI Workflows](../workflows/agent-and-cli.md), and [Testing & Release](../operations/testing-and-release.md).

## Subobject Classifier and Categorical Primitives

Categorical program evaluation structures the relationship between programs, subobject classifiers, and evaluation values:

- **Program Objects ($A, B \in \mathbf{E}$)**: Encapsulated by `ProgramObject`, representing the AST lifted into the category. Wraps tree-sitter parsed trees, raw source, target language, parser metadata (`parser_name`, `parser_version`), native root kind, and normalized UAST root node (`UASTNode`).
- **Program Morphism ($P$)**: Encapsulated by `ProgramMorphism`, viewing source code and its AST/graph representations as an arrow $f: A \to B$ between computational states. Contains raw source, language, path, primary `ProgramObject`, and additional `Box<dyn Representation>` adapters.
- **Subobject Classifier ($\Omega$)**: Implemented in `Omega`, representing truth values as a 16-element Heyting algebra lattice generated by the power set of $\{ \text{SIMPLE}, \text{COMPOSABLE}, \text{SECURE}, \text{NAVIGABLE} \}$.
- **Characteristic Morphism ($\chi_S : P \to \Omega$)**: Implemented by `CharacteristicMorphism`, mapping every program morphism $P$ to its characteristic evaluation value in $\Omega$.

### The 16 Truth States and Medal Tiers

The subobject classifier $\Omega$ consists of 16 distinct truth values (`EvaluationValue`), each representing a unique subset of satisfied quality generators:

| Discriminant / Bitmask | Name | Satisfied Count | Medal Tier | Symbol |
| --- | --- | --- | --- | --- |
| `0b0000` | `SLOP` | 0 | **SLOP** | ❌ |
| `0b0001` | `SIMPLE` | 1 | **BRONZE** | 🥉 |
| `0b0010` | `COMPOSABLE` | 1 | **BRONZE** | 🥉 |
| `0b0011` | `SIMPLE_COMPOSABLE` | 2 | **SILVER** | 🥈 |
| `0b0100` | `SECURE` | 1 | **BRONZE** | 🥉 |
| `0b0101` | `SIMPLE_SECURE` | 2 | **SILVER** | 🥈 |
| `0b0110` | `COMPOSABLE_SECURE` | 2 | **SILVER** | 🥈 |
| `0b0111` | `SIMPLE_COMPOSABLE_SECURE` | 3 | **GOLD** | 🥇 |
| `0b1000` | `NAVIGABLE` | 1 | **BRONZE** | 🥉 |
| `0b1001` | `SIMPLE_NAVIGABLE` | 2 | **SILVER** | 🥈 |
| `0b1010` | `COMPOSABLE_NAVIGABLE` | 2 | **SILVER** | 🥈 |
| `0b1011` | `SIMPLE_COMPOSABLE_NAVIGABLE` | 3 | **GOLD** | 🥇 |
| `0b1100` | `SECURE_NAVIGABLE` | 2 | **SILVER** | 🥈 |
| `0b1101` | `SIMPLE_SECURE_NAVIGABLE` | 3 | **GOLD** | 🥇 |
| `0b1110` | `COMPOSABLE_SECURE_NAVIGABLE` | 3 | **GOLD** | 🥇 |
| `0b1111` | `IDEAL` | 4 | **PLATINUM** | 🏆 |

### Classification Flow

`CharacteristicMorphism::classify_detailed` is the canonical entry point for program classification. If source code fails to parse or the `ProgramMorphism` is invalid (`is_valid() == false`), the classifier returns a default non-parseable result (`is_parseable = false`) with value $\bot = \text{SLOP}$.

For valid source code, `classify_detailed` automatically constructs `AstRepresentation` and `NavigableRepresentation` from the UAST. It merges any caller-supplied optional representations (such as MDG or CPG graphs) by dimension, executes policy translators ($\Phi_{\text{SIMPLE}}$, $\Phi_{\text{COMPOSABLE}}$, $\Phi_{\text{SECURE}}$, $\Phi_{\text{NAVIGABLE}}$), records each policy's `achieved` state, assesses advisory metrics against language priors, computes band scores $S = \text{band\_score}(G, A)$, and joins only the satisfied generator bits into the final lattice verdict.

```mermaid
flowchart TD
    Morphism["ProgramMorphism and optional representations"] --> Valid{"AST present and valid morphism?"}
    Valid -->|No| SLOP["Default result: non-parseable SLOP"]
    Valid -->|Yes| BuildReps["Build AST and NAVIGABLE representations"]
    BuildReps --> Merge["Merge representations by dimension"]
    Merge --> Policies["Evaluate policy translators: SIMPLE, COMPOSABLE, SECURE, NAVIGABLE"]
    Policies --> Achieved["Filter satisfied generators where achieved is true"]
    Achieved --> Lattice["Omega lattice element and medal tier"]
```
*Quality evaluation flow: valid source constructs local representations, merges optional graph metrics by dimension, and joins satisfied generator bits into the final Omega lattice verdict.*

`SIMPLE` and `NAVIGABLE` are measured locally for every parseable file. If MDG representations are omitted, `composable` remains unmeasured; if CPG representations are omitted, `secure` remains unmeasured. **Unmeasured is strictly distinct from failed or passed**: missing optional evidence leaves a pillar unmeasured without granting a pass or forcing a failure bit.

## Categorical Program Representations (Graphs)

Topos represents program structural views as typed objects implementing the `Representation` protocol (`topos/engine/src/graphs/base.rs`). Each graph representation captures a distinct architectural or semantic slice of a program:

```mermaid
flowchart TD
    Source["Source Code File"] --> AST["AstRepresentation (tree-sitter)"]
    Source --> UAST["UastRepresentation (mapper_*)"]
    UAST --> CFG["ControlFlowGraph (basic blocks & control edges)"]
    UAST --> PDG["ProgramDependenceGraph (DDG + CDG dataflow)"]
    AST --> CPG["CodePropertyGraph (AST + CFG + PDG edge synthesis)"]
    CFG --> CPG
    PDG --> CPG
    GitNexus[".gitnexus / LadybugDB"] --> MDG["ModuleDependencyGraph (imports & calls)"]
    ProcessLogs["Git Churn & Authors"] --> ProcessG["ProcessGraph (temporal dynamics)"]

    AST --> ProbesSIMPLE["SIMPLE Probes (entropy, complexity, divergence)"]
    CFG --> ProbesCFG["CFG Probes (cyclomatic, homology)"]
    CPG --> ProbesSECURE["SECURE Probes (dangerous_calls, taint_flows)"]
    MDG --> ProbesCOMPOSABLE["COMPOSABLE Probes (fan_out, fan_in, instability, curvature)"]
    ProcessG --> ProbesProc["Process Probes (curvature)"]
```
*Categorical graph representations and probe flow: program representations synthesize concrete ASTs, UASTs, CFGs, PDGs, CPGs, MDGs, and Process graphs to feed single-program probes.*

- **AST (`AstRepresentation`)**: Derived directly via tree-sitter parsers (`topos/engine/src/graphs/ast/`). Serves as the base representation for token distribution, syntax node counts, AST entropy, and function-level complexity (`ast.max_function_complexity`).
- **UAST (`UastRepresentation` / `UASTNode`)**: Universal AST mapper normalizing concrete language semantics across supported languages (Rust, Python, JavaScript, TypeScript, C++, Go) into unified node kinds and declaration semantics (`topos/engine/src/graphs/uast/`). Shared mapper logic (`mapper_common`) handles node traversal, test node filtering, and node identifier generation via BLAKE2b 8-byte hashes over `(lang, native.node_kind, span.start_byte, span.end_byte, parent_id)`.
- **CFG (`ControlFlowGraph`)**: Models basic blocks and directed control edges (branches, loops, jumps, matches, exception handlers) constructed from UAST (`topos/engine/src/graphs/cfg/`). Provides the topology for whole-file cyclomatic complexity ($\mathcal{E} - \mathcal{N} + 2\mathcal{P}$) and 1-dimensional Betti homology ($\beta_1$).
- **PDG (`ProgramDependenceGraph`)**: Academic Program Dependence Graph integrating Data Dependence Graph (DDG) and Control Dependence Graph (CDG) edges between statements and expressions (`topos/engine/src/graphs/pdg/`). Consumed by the CPG builder and profunctor comparison engines.
- **CPG (`CodePropertyGraph`)**: Synthesizes AST, CFG, DDG, and CDG edge families into a unified multigraph (`topos/engine/src/graphs/cpg/`). Used for static security analysis to detect dangerous API reachability and trace source-to-sink taint flows. Feeds the `SECURE` generator.
- **MDG (`ModuleDependencyGraph`)**: Models file/module coupling, imports, calls, exports, and inheritance (`topos/engine/src/graphs/mdg/`). Uses language-specific stdlib edge filters (`Rust`, `Python`, `JS`, `JSONC`) to isolate project dependencies. Feeds the `COMPOSABLE` generator.
- **Process Graph (`ProcessGraph`)**: Captures temporal commit churn, author interaction patterns, and execution path step dynamics (`topos/engine/src/graphs/process/`) for process-level curvature evaluation.

### LadybugDB Native Graph Loading

For large codebases, `ModuleDependencyGraph` can load natively from `.gitnexus/lbug` using the embedded `lbug` Rust client (`topos/engine/src/graphs/mdg/ladybug.rs`).

To guarantee store integrity and prevent process panics:
1. **Safety Inspection (`unfinished_write`)**: Before opening the database, `unfinished_write` checks whether `lbug.wal` exists (indicating an un-checkpointed Write-Ahead Log) or if `meta.json` still has `incrementalInProgress` set. If an unfinished write is detected, loading halts with `MdgError::UnfinishedWrite`.
2. **Read-Only Fallback**: Topos opens the database in read-only mode (`SystemConfig::default().read_only(true)`). If read-only opening fails, it retries in read-write mode before returning an error.
3. **Table & Schema Discovery**: Topos queries `CALL show_tables() RETURN *` to discover node tables, maps properties to JSON values, and queries `CodeRelation` edges (including edge confidence, reason, and optional step markers).

```mermaid
flowchart TD
    Path[".gitnexus/lbug directory path"] --> SafetyCheck{"unfinished_write check: lbug.wal exists or incrementalInProgress set?"}
    SafetyCheck -->|Yes| Fail["Return MdgError::UnfinishedWrite"]
    SafetyCheck -->|No| OpenRO["Try Database::new in read-only mode"]
    OpenRO -->|Success| Conn["Create lbug Connection"]
    OpenRO -->|Error| OpenRW["Retry Database::new in read-write mode"]
    OpenRW --> Conn
    Conn --> Tables["CALL show_tables to discover node tables"]
    Tables --> LoadNodes["Query MATCH n RETURN n for all node tables"]
    LoadNodes --> LoadEdges["Query MATCH CodeRelation edges"]
    LoadEdges --> MDG["Populated ModuleDependencyGraph"]
```
*LadybugDB native loading lifecycle: safety checks guard against un-checkpointed WAL files before reading nodes and CodeRelation edges into the MDG.*

## Evaluation Functors: Probes and Profunctors

Topos structures metric extraction and structural comparison through categorical functors defined in `topos/engine/src/functors`:

### Probes ($P : \mathbf{E} \to \mathbb{R}$)

Probes are single-program functors mapping a program representation object to a real metric value (`topos/engine/src/functors/probes/`):

- **AST Probes**: `ast.entropy` (Shannon entropy across node types), `ast.max_function_complexity` (highest per-function cyclomatic complexity), `ast.divergence` (Semantic Compositional Divergence $SCD$), and `ast.scopes`.
- **UAST Probes**: `uast.abstractness` (ratio of abstract contracts to concrete implementations) and structural signature hashes.
- **CFG Probes**: `cfg.cyclomatic` (whole-file complexity) and `cfg.homology` (1D Betti number / cycle basis dimension).
- **CPG Probes**: `cpg.dangerous_calls` (reachable dangerous API sinks) and `cpg.taint_flows` (source-to-sink flow paths).
- **MDG Probes**: `mdg.fan_out` (outward module coupling count), `mdg.fan_in` (inward dependent count), `mdg.instability` ($I = \frac{\text{fan\_out}}{\text{fan\_in} + \text{fan\_out}}$), and `mdg.curvature` (Forman-Ricci curvature over coupling topologies).
- **Process Probes**: `process.curvature` (Ricci curvature on commit/author interaction graphs).

### Profunctors ($D : \mathbf{E} \times \mathbf{E}^{\text{op}} \to \mathbb{R}$)

Profunctors compute structural comparisons and distances between two program objects ($A, B \in \mathbf{E}$) (`topos/engine/src/functors/profunctors/`):

- **AST Profunctors**: Tree edit distance and Gromov-Wasserstein alignment over AST nodes.
- **UAST Profunctors**: Cross-language kind-histogram deltas, structural summary deltas, inline test ledger matching, and structural test coverage mapping.
- **CFG Profunctors**: Cyclomatic complexity deltas, control edge-kind deltas, and longest-path deltas.
- **PDG Profunctors**: DDG and CDG edge-set Jaccard distance metrics.
- **MDG Profunctors**: Coupling, instability, fan-in/out, and dependency-depth deltas.
- **CPG Profunctors**: Per-family edge Jaccard distances alongside dangerous call and taint flow deltas used by automated refactoring engines.

## Gates, Scores, and Policy Translators

Each policy translator ($\Phi_{\text{SIMPLE}}$, $\Phi_{\text{COMPOSABLE}}$, $\Phi_{\text{SECURE}}$, $\Phi_{\text{NAVIGABLE}}$) returns a `ScoredDecision` structured into separate concerns:

- `achieved`: A Boolean indicating whether all registered raw-metric gates passed (`GateResult::passed`). The classifier uses this flag to set the pillar's bit in the lattice verdict.
- `score`: A normalized floating-point value ($[0.0, 1.0]$) derived from metric desirability functions and advisory readings (`band_score(G, A)`). Reporting scores do not override raw gates or manipulate lattice bits.
- `interpretation` and suggested actions: Textual explanations provided for CLI/MCP display.

Gate evaluation is centralized in `evaluate_gates` (`topos/engine/src/evaluation/policies/gates.rs`), serving as a single source of truth for classifiers, diagnostic tools, and refactor target generators. The gate registry includes `NaN` guards that fail closed, ensuring non-numeric or missing metric values never pass bounded gate comparisons.

```mermaid
flowchart TD
    Metrics["Raw Metric Values"] --> Gates["evaluate_gates and NaN Guard"]
    Gates --> PassCheck{"All registered gates passed?"}
    PassCheck -->|Yes| Pass["Gate Pass: achieved = true"]
    PassCheck -->|No| Fail["Gate Fail: achieved = false"]
    Pass --> SetBit["Set Generator Bit in Omega"]
    Fail --> ClearBit["Clear Generator Bit in Omega"]
    SetBit --> Join["Lattice Join (Omega)"]
    ClearBit --> Join
```
*Gate evaluation architecture: evaluate_gates enforces NaN guards and sets or clears generator bits prior to lattice join.*

### SIMPLE: Local Code Structure

`SIMPLE` measures function-level complexity and AST entropy:
- `ast.entropy`: Must fall within the inclusive band `0.2..=0.8`.
- `ast.max_function_complexity`: Must not exceed `10`.

Import/export entrypoint modules (`is_entrypoint_module` in `file_roles.rs`) are exempted from entropy gate failures. Whole-file cyclomatic complexity (`cfg.cyclomatic`, threshold `15`) is evaluated as a codebase-relative advisory rather than a gate: because whole-file complexity aggregates across all functions, gating on it would unfairly penalize files containing many small, simple functions. Advisory scores are surfaced for context but do not clear the `SIMPLE` gate bit.

### COMPOSABLE: Outward Dependency Burden

`COMPOSABLE` evaluates structural coupling and module dependencies:
- Decisive gate: `mdg.fan_out <= 10`.

Module instability (`mdg.instability`) and fan-in (`mdg.fan_in`) are measured as codebase-relative advisories when coupling is non-trivial ($\text{coupling} \ge 2.0$). Advisory readings do not alter the `mdg.fan_out` gate result. When no MDG is provided, `COMPOSABLE` is marked unavailable rather than failed.

### SECURE: Zero-Tolerance and Suppression Overlay

`SECURE` enforces zero-tolerance security requirements:
- Gates: `cpg.dangerous_calls == 0` and `cpg.taint_flows == 0`.

Any nonzero dangerous call or taint flow clears the `SECURE` generator bit. Desirability scoring for security is reporting-only and cannot compensate for active vulnerabilities.

#### Security Diagnostic Overlay and Suppression Mechanics

In MCP tool execution (`topos/mcp/src/diagnostics.rs`), security evidence is evaluated lazily through a diagnostic overlay. The overlay checks `overlay_applies` (calling `security_evidence_needed`) before reparsing source or constructing a full `ProgramMorphism`:

```rust
pub(crate) fn security_evidence_needed(result: &ClassificationResult) -> bool {
    result.is_parseable
        && (result.raw_metrics.get("cpg.dangerous_calls").copied().unwrap_or(0.0) > 0.0
            || result.raw_metrics.get("cpg.taint_flows").copied().unwrap_or(0.0) > 0.0)
}
```

If `security_evidence_needed` returns true, the overlay loads project configuration (`.topos.toml`), constructs the CPG, extracts un-filtered raw security findings (`allow: None`), and passes both raw findings and the CPG to `apply_allowlist` (`topos/engine/src/evaluation/suppression.rs`). Supplying raw findings ensures `apply_allowlist` partitions active versus acknowledged findings correctly without bypassing disclosures or grade caps.

```mermaid
sequenceDiagram
    participant Tool as Tool or Agent Execution
    participant Morphism as ProgramMorphism
    participant Diag as Security Diagnostic Overlay
    participant Engine as apply_allowlist

    Tool->>Morphism: Classify source metrics
    Morphism-->>Tool: ClassificationResult
    Tool->>Diag: Check security_evidence_needed(result)
    alt Security evidence needed (parseable and positive raw CPG risk)
        Diag->>Morphism: build_cpg()
        Diag->>Diag: Extract full raw security findings
        Diag->>Engine: apply_allowlist(result, raw_findings, config, file_path, cpg)
        Engine-->>Diag: AdjustedVerdict (active vs acknowledged, grade_capped)
        Diag-->>Tool: SecurityOverlay with active findings and capped verdict
    else No security evidence needed
        Diag-->>Tool: Return None (skip overlay)
    end
```
*Diagnostic mapping flow: the security overlay guards against unnecessary CPG construction and applies allowlists to raw findings without blinding risk disclosures or bypassing grade caps.*

Project allowlists in `.topos.toml` use `[[secure.allow]]` tables. To enforce mandatory anti-gaming friction:
- Each allowlist entry requires non-empty `pattern` and `reason` strings; entries lacking a reason are dropped during configuration parsing (`parse_allow_entries`).
- Entries support posix-style `scope` glob patterns evaluated relative to the configuration root (`ToposConfig::entries_for`).
- Malformed configuration files or entries drop silently to default settings without crashing evaluation.
- Ephemeral CLI flags (`--allow`) merge with scope `**` under the reason `CLI --allow (ephemeral)`.

#### Exact Metric Recomputation and Mathematical Grade Capping

The suppression overlay in `suppression.rs` (`apply_allowlist`) computes an adjusted verdict on top of the canonical raw result while upholding explicit anti-gaming invariants:

1. **Findings Partitioning**: Raw findings are partitioned into `active` and `acknowledged` based on pattern matching against callee names (`entry_for_callee`).
2. **Exact Metric Recomputation on CPG**: To prevent upstream findings display caps (such as truncation to 20 findings) from corrupting evaluation, `apply_allowlist` recomputes exact `cpg.dangerous_calls` and `cpg.taint_flows` counts directly on the `CodePropertyGraph` using `dangerous_api_reachable(cpg, &allow_patterns)` and `taint_flow_paths(cpg, &allow_patterns)`.
3. **Adjusted Pass Determination**: `adjusted_secure_pass` becomes `true` if and only if exact recomputed counts reach zero (`dangerous == 0 && taint == 0`).
4. **Mathematical Grade Capping**: Acknowledging a risk via an allowlist can never buy an `IDEAL` (PLATINUM) verdict. If findings were acknowledged (`!acknowledged.is_empty()`) and the recomputed adjusted element satisfies all four pillars (`adjusted_element == EvaluationValue::Ideal`), `apply_allowlist` mathematically caps the grade by clearing the `SECURE` generator bit:
   $$\text{adjusted\_element} = \text{EvaluationValue::from\_bits}(\text{adjusted\_element.bits()} \land \neg \text{Generator::Secure.value().bits()})$$
   This forces the capped verdict to `SIMPLE_COMPOSABLE_NAVIGABLE` (GOLD) and sets `grade_capped = true`. An acknowledged risk is capped at GOLD.
5. **Score Lifting**: Calling `secure_lifted` lifts the SECURE pillar score to $\tau$ ($0.50$) when `adjusted_secure_pass` is true so passing pillars do not score below gate threshold, while keeping the grade cap enforced on the lattice element.

```mermaid
stateDiagram-v2
    [*] --> RawFinding: CPG security analysis detects risk
    RawFinding --> ActiveFinding: No allowlist match or missing reason
    RawFinding --> AcknowledgedFinding: Pattern match in [[secure.allow]] with valid reason
    
    ActiveFinding --> GateFailed: Active dangerous calls or taint flows > 0
    GateFailed --> RawSecureFailed: cpg.dangerous_calls > 0 OR cpg.taint_flows > 0
    
    AcknowledgedFinding --> ExactRecomputation: Recompute CPG metrics without allowlisted patterns
    
    ExactRecomputation --> AdjustedPass: Adjusted dangerous = 0 AND adjusted taint = 0
    ExactRecomputation --> AdjustedFail: Remaining unsuppressed active risks > 0
    
    AdjustedFail --> SECURE_Cleared: Adjusted SECURE bit cleared (0)
    AdjustedPass --> GradeCapCheck: Check all other pillars satisfied (IDEAL candidate)
    
    GradeCapCheck --> GradeCapped: Un-capped verdict is IDEAL (0b1111) and acknowledged is non-empty
    GradeCapCheck --> UncappedVerdict: Un-capped verdict is not IDEAL or acknowledged is empty
    
    GradeCapped --> GOLD_Verdict: Clear SECURE bit (0b1011 SIMPLE_COMPOSABLE_NAVIGABLE) and set grade_capped = true
    UncappedVerdict --> FinalVerdict: Retain computed lattice element
```
*Suppression state transitions: raw security findings are evaluated against scoped allowlist entries, exact CPG counts are recomputed, and acknowledged findings enforce mathematical grade capping below IDEAL.*

```mermaid
flowchart TD
    Raw["Raw ClassificationResult (e.g. SECURE bit cleared)"] --> Allow["apply_allowlist(result, findings, config, path, cpg)"]
    Allow --> Partition["Partition raw findings into active vs acknowledged"]
    Partition --> Recompute["Recompute CPG counts: dangerous_api_reachable and taint_flow_paths"]
    Recompute --> PassCheck{"Adjusted counts zero? (dangerous == 0 and taint == 0)"}
    PassCheck -->|No| KeepFailed["adjusted_secure_pass = false (SECURE bit = 0)"]
    PassCheck -->|Yes| SetPass["adjusted_secure_pass = true (set SECURE bit)"]
    SetPass --> Rebuild["recompute_element: join satisfied bits (e.g. 0b1111 = IDEAL)"]
    CapCheck{"acknowledged is non-empty AND adjusted_element == IDEAL?"}
    Rebuild --> CapCheck
    CapCheck -->|No| KeepElement["adjusted_element remains uncapped (e.g. PLATINUM)"]
    CapCheck -->|Yes| BitClear["Clear SECURE bit: 0b1111 AND NOT 0b0100 = 0b1011"]
    BitClear --> SetFlag["grade_capped = true (verdict capped at GOLD: SIMPLE_COMPOSABLE_NAVIGABLE)"]
```
*Verdict mathematical grade capping: clearing the SECURE bit (0b0100) from an otherwise IDEAL verdict (0b1111) mathematically caps suppressed results at GOLD, preventing acknowledged risks from ever achieving PLATINUM.*

### NAVIGABLE: Semantic Compositional Divergence

`NAVIGABLE` quantifies nesting load using per-callable **Semantic Compositional Divergence** (SCD):

$$SCD(\text{fn}) = \sum_{u} \text{depth}(u) \cdot \ln(1 + \text{fanout}(u))$$

The summation iterates over nested block scopes (`IfStmt`, loops, `MatchStmt`, `TryStmt`, `WithStmt`, and nested functions). `fanout(u)` counts child scopes within block $u$, while the outer callable root resides at depth zero. Conditional expressions and short-circuit boolean operations are excluded from SCD, isolating nesting load from `SIMPLE`'s branch complexity.

- Gate threshold: Maximum $SCD \le 10.0$ (inclusive).
- Normalized score: Declines linearly from $1.0$ at divergence $10.0$ to $0.0$ at divergence $20.0$.

The divergence probe isolates specific function spans and names, allowing diagnostics and refactor targets to identify exact source locations responsible for high nesting.

## Multi-File Roll-up, Preferences, and Severity

### Codebase Roll-up ($\bigwedge_f \chi_S(f)$)

`CharacteristicMorphism::combine_dimensions` computes multi-file codebase quality through pointwise meet aggregation:
- A generator holds for the entire codebase if and only if every parseable file that evaluated the dimension achieved it.
- Files lacking evidence for an optional dimension (e.g., missing MDG) are excluded from that dimension's rollup.
- Any unparseable file fails every measured dimension, reducing the rollup to `SLOP`.

### User Preferences and Lexicographic Ordering

While priority labels provide guidance without altering gate thresholds, `UserPreferences` defines a strict lexicographic preference order across the four generators (`topos/engine/src/evaluation/preferences.rs`). Configuration loading accepts a complete valid four-generator ranking only; malformed or legacy three-entry rankings fall back to `default_preferences()`:

$$\text{SIMPLE} \succ \text{NAVIGABLE} \succ \text{SECURE} \succ \text{COMPOSABLE}$$

`UserPreferences` maps generator subsets to scalar ranks using weights $(8, 4, 2, 1)$. The primary target is `IDEAL`, with `SIMPLE_NAVIGABLE` serving as a two-pillar fallback. Priority indicators steer agent guidance but do not alter per-metric pass/fail thresholds.

### PR Recap Severity Levels and Waivers

PR recap reporting (`topos pr-recap`, configured in `topos/engine/src/config/pr_gate.rs`) classifies rule violations into four explicit severity levels configured via preset profiles (`relaxed`, `recommended`, `strict`) or per-gate overrides in `.topos.toml`:

- `off`: Rule disabled.
- `info`: Diagnostic finding reported without failing checks.
- `warn`: Warning surfaced in recap output.
- `block`: Hard violation that fails PR checks.

The 16 configurable PR recap rules (such as `pillar_lost`, `pillar_inherited`, `moved_pillar`, `score_drop`, `new_file_insecure`, `new_file_slop`, `import_cycle`, and `fan_in_growth`) evaluate file diffs against base branch baselines. Disclosed waivers parsed in `topos/engine/src/evaluation/waivers.rs` (`[[pr_recap.waive]]`) allow explicit temporary exemptions for specific gate rules with mandatory `reason` disclosures.

## Refactor Target Mapping and Auxiliary Boundaries

Diagnostic engines map evaluation findings into concrete refactor targets (`topos/engine/src/evaluation/suggestions.rs`). `build_refactor_targets` orders candidate edits using gate-score ascent and metric status:

1. **Gate Failures ("fix")**: Metric failures that directly clear a pillar's `achieved` bit are prioritized ahead of advisory findings.
2. **Advisory Issues ("improve")**: Metrics lacking a registered gate (such as whole-file cyclomatic complexity) are labeled `"improve"` and sorted behind real gate failures, preventing agents from prioritizing advisory edits over hard failures.
3. **Pillar Priority & Desirability**: Candidates are sorted by active goal alignment, lowest desirability score, and user preference rank ties.

Auxiliary analyses—including structural coverage, clone detection, AST diffing, and MCP process metrics—operate outside the 4-generator lattice. While they produce actionable diagnostics, they never alter generator bits or medal verdicts.

## Operational Guidelines and Safe Extensions

When modifying or extending the quality model:

1. **Gate Definitions**: Maintain threshold logic, exemptions, and `NaN` guards in `topos/engine/src/evaluation/policies/gates.rs`. Store score curves and calibration weights in respective policy/calibration modules.
2. **Classifier Synchronization**: When adding or altering metrics, update policy scorers, classifier representation grouping in `characteristic_morphism.rs`, and MCP/CLI renderers.
3. **Verdict Invariants**: Preserve distinction between parse failures, gate failures, and unmeasured optional dimensions. Never allow advisory scores or allowlist acknowledgements to pass a gate.
4. **Testing Standards**: Validate changes using `cargo test -p topos-engine`. Include diagnostic overlay, suppression, and refactor target test suites when modifying MCP integration boundaries.
