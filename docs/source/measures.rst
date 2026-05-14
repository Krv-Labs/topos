.. _measures:

========
Measures
========

Every program evaluated by Topos is measured along three independent generators. Topos never collapses these into a single number — you always see which generator is the problem.

1. The SIMPLE Generator (Code Complexity)
------------------------------------------

Evaluates the internal quality of the code by analyzing the Control Flow Graph (CFG) and Abstract Syntax Tree (AST). The SIMPLE generator always runs and maps to the ``SIMPLE`` lattice outcome.

* **Cyclomatic Complexity** (``cfg.cyclomatic``)
  Measures the number of linearly independent paths through the code. Branches, loops, and conditionals increase complexity. Higher values negatively impact the SIMPLE score.

* **Essential Complexity** (``cfg.essential``)
  Counts "structured" vs. unstructured control flow. Complex nested conditions reduce this metric.

* **Nesting Depth** (``cfg.nesting_depth``)
  Maximum nesting level of control structures. Deeper nesting is harder to reason about.

* **Longest Path** (``cfg.longest_path``)
  Longest acyclic execution path through the CFG. Long paths correlate with high cognitive load.

* **Entropy** (``ast.entropy``)
  A Kolmogorov-complexity proxy using compression ratios. It measures how predictable the code is. Very low entropy suggests excessive boilerplate; very high entropy signals chaotic or highly unusual structure (often seen in hallucinated code). The healthy range sits around 0.5.

2. The COMPOSABLE Generator (Module Coupling)
----------------------------------------------

Evaluates how a file fits into the broader repository by analyzing the module dependency graph. *(Requires GitNexus)* The COMPOSABLE generator maps to the ``COMPOSABLE`` lattice outcome.

* **Coupling** (``mdg.coupling``)
  The total number of afferent (incoming) and efferent (outgoing) dependencies. High total coupling negatively impacts the COMPOSABLE score.

* **Instability** (``mdg.instability``)
  Calculated as ``Efferent / (Afferent + Efferent)``.

  - Near 0: The module is a rigid dependency for many others and is hard to change safely.
  - Near 1: The module is highly unstable because it depends on many other parts of the system.
  - A balanced range (0.3 – 0.7) helps achieve a higher COMPOSABLE score.

* **Fan-in / Fan-out** (``mdg.fan_in``, ``mdg.fan_out``)
  Diagnostic metrics tracking explicit call edges. These are visible in detailed inspections but don't strictly set the final verdict.

* **Dependency Depth** (``mdg.dep_depth``)
  The longest dependency chain from this module. Shallow chains are easier to understand and refactor.

3. The SECURE Generator (Vulnerability Analysis)
-------------------------------------------------

Evaluates whether the code flow can reach dangerous operations or untrusted data.  Computed from the Code Property Graph (CPG) — derived intrinsically from the UAST, no external tooling required.  The SECURE generator maps to the ``SECURE`` lattice outcome.

* **Dangerous Calls** (``cpg.dangerous_calls``)
  Count of reachable call sites matching a per-language registry of dangerous APIs (Python: ``eval``, ``exec``, ``pickle.loads``, …; C++: ``gets``, ``strcpy``, …).  Lower counts improve the SECURE score.

* **Taint Flows** (``cpg.taint_flows``)
  Source→sink data-flow paths along the CPG's data-dependence edges, from untrusted sources (e.g. ``input``, ``request.args``) to dangerous sinks. Longer taint chains increase risk.

Scoring and Priorities
----------------------

Topos produces a continuous normalized score ``[0.0, 1.0]`` for each generator.
A score at or above the threshold (default **0.6**) means that generator is achieved.
Scores are reported as percentages (0–100%) in all CLI and MCP output.

**SIMPLE score** (determines if SIMPLE is achieved):

.. code-block:: text

   cfg_quality    = weighted avg of: 1 - (cyclomatic / 30), essential_ratio, nesting_penalty, path_length_penalty
   entropy_quality = max(0, 1 - 2 × |entropy - 0.5|)     ← peak at 0.5
   simple_score   = w_cfg × cfg_quality + (1 - w_cfg) × entropy_quality

**COMPOSABLE score** (determines if COMPOSABLE is achieved, requires GitNexus):

.. code-block:: text

   coupling_quality    = 1 - min(coupling / 35, 1.0)
   instability_quality = tent function over [0.3, 0.7]        ← 1.0 in optimal range
   fan_balance_quality = reward balanced fan-in/out
   composable_score    = w_c × coupling_quality + w_i × instability_quality + w_f × fan_balance_quality

**SECURE score** (determines if SECURE is achieved):

.. code-block:: text

   danger_quality = exp(-dangerous_calls / DANGER_SCALE)
   taint_quality  = exp(-taint_flows     / TAINT_SCALE)
   secure_score   = w_t × taint_quality + (1 - w_t) × danger_quality
   secure_score            = w_d × dangerous_calls_quality + w_t × taint_flow_quality

The weights (``w_*``) are controlled by the **Priority**:

.. list-table::
   :widths: 15 15 15 15 40
   :header-rows: 1

   * - Priority
     - ``simple``
     - ``composable``
     - ``secure``
     - Effect
   * - ``balanced``
     - 0.33
     - 0.33
     - 0.33
     - Equal emphasis on all three generators
   * - ``simple``
     - 0.7
     - 0.15
     - 0.15
     - Upweights SIMPLE; rewards low-complexity code
   * - ``composable``
     - 0.15
     - 0.7
     - 0.15
     - Upweights COMPOSABLE; rewards tightly-bounded modules
   * - ``secure``
     - 0.15
     - 0.15
     - 0.7
     - Upweights SECURE; rewards low-risk data flows

Changing the priority does not change what is measured — it changes the weights
within each generator's scoring function.

Verdicts
--------

The per-generator scores map to an 8-valued Heyting algebra (free lattice on 3 generators):

* ``SLOP`` (⊥): No generators achieved (all scores below threshold) or syntax error.
* ``SIMPLE``: Only SIMPLE achieved.
* ``COMPOSABLE``: Only COMPOSABLE achieved (requires GitNexus; unreachable from SIMPLE alone).
* ``SECURE``: Only SECURE achieved.
* ``SIMPLE_COMPOSABLE``: Both SIMPLE and COMPOSABLE achieved.
* ``SIMPLE_SECURE``: Both SIMPLE and SECURE achieved.
* ``COMPOSABLE_SECURE``: Both COMPOSABLE and SECURE achieved.
* ``IDEAL`` (⊤): All three generators achieved. Perfectly simple, composable, and secure.

The three generators ``SIMPLE``, ``COMPOSABLE``, and ``SECURE`` are **pairwise incomparable** — a
file can achieve any subset of them independently. The overall ``lattice_element`` in the
response is determined by which combination of generators scored ≥ 0.6:

.. code-block:: text

   SIMPLE = 1, COMPOSABLE = 1, SECURE = 1  → IDEAL
   SIMPLE = 1, COMPOSABLE = 1, SECURE = 0  → SIMPLE_COMPOSABLE
   SIMPLE = 1, COMPOSABLE = 0, SECURE = 1  → SIMPLE_SECURE
   SIMPLE = 0, COMPOSABLE = 1, SECURE = 1  → COMPOSABLE_SECURE
   SIMPLE = 1, COMPOSABLE = 0, SECURE = 0  → SIMPLE
   SIMPLE = 0, COMPOSABLE = 1, SECURE = 0  → COMPOSABLE
   SIMPLE = 0, COMPOSABLE = 0, SECURE = 1  → SECURE
   SIMPLE = 0, COMPOSABLE = 0, SECURE = 0  → SLOP