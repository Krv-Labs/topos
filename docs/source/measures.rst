.. _measures:

========
Measures
========

.. tip::
   Every program evaluated by Topos is measured along three independent **Quality Pillars**. These pillars are the generators for the **Quality Badges** you can earn. Topos never collapses these into a single number — you always see which pillar is the problem.

1. The SIMPLE Pillar (Code Complexity)
------------------------------------------

Evaluates the internal quality of the code by analyzing the Control Flow Graph (CFG) and Abstract Syntax Tree (AST). The SIMPLE pillar always runs and maps to the ``SIMPLE`` badge outcome.

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

2. The COMPOSABLE Pillar (Module Coupling)
----------------------------------------------

Evaluates how a file fits into the broader repository by analyzing the module dependency graph. *(Requires GitNexus)* The COMPOSABLE pillar maps to the ``COMPOSABLE`` badge outcome.

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

3. The SECURE Pillar (Vulnerability Analysis)
-------------------------------------------------

Evaluates whether the code flow can reach dangerous operations or untrusted data.  Computed from the Code Property Graph (CPG) — derived intrinsically from the UAST, no external tooling required.  The SECURE pillar maps to the ``SECURE`` badge outcome.


* **Dangerous Calls** (``cpg.dangerous_calls``)
  Count of reachable call sites matching a per-language registry of dangerous APIs (Python: ``eval``, ``exec``, ``pickle.loads``, …; C++: ``gets``, ``strcpy``, …).  Lower counts improve the SECURE score.

* **Taint Flows** (``cpg.taint_flows``)
  Source→sink data-flow paths along the CPG's data-dependence edges, from untrusted sources (e.g. ``input``, ``request.args``) to dangerous sinks. Longer taint chains increase risk.

Scoring and Manager Priorities
------------------------------

Topos produces a continuous normalized score ``[0.0, 1.0]`` for each pillar.
A pillar is **achieved** if its score is at or above the threshold (default **0.6**).
Scores are reported as percentages (0–100%) in all CLI and MCP output.

The weights (``w_*``) for each pillar's internal components are controlled by the **Priority** (part of the **Preference Ranking**):


.. list-table::
   :widths: 15 15 15 15 40
   :header-rows: 1

   * - Priority
     - ``simple``
     - ``composable``
     - ``secure``
     - Effect
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

The per-pillar scores map to an 8-valued Heyting algebra (free lattice on 3 generators), representing the **Quality Badges**:

* ``SLOP`` (⊥): No pillars achieved (all scores below threshold) or syntax error.
* ``SIMPLE``: Only SIMPLE achieved.
* ``COMPOSABLE``: Only COMPOSABLE achieved (requires GitNexus; unreachable from SIMPLE alone).
* ``SECURE``: Only SECURE achieved.
* ``SIMPLE_COMPOSABLE``: Both SIMPLE and COMPOSABLE achieved.
* ``SIMPLE_SECURE``: Both SIMPLE and SECURE achieved.
* ``COMPOSABLE_SECURE``: Both COMPOSABLE and SECURE achieved.
* ``IDEAL`` (⊤): All three pillars achieved. Perfectly simple, composable, and secure.

The three pillars ``SIMPLE``, ``COMPOSABLE``, and ``SECURE`` are **pairwise incomparable** — a
file can achieve any subset of them independently. The overall ``lattice_element`` in the
response is determined by which combination of pillars scored ≥ 0.6:

.. code-block:: text

   SIMPLE = 1, COMPOSABLE = 1, SECURE = 1  → IDEAL
   SIMPLE = 1, COMPOSABLE = 1, SECURE = 0  → SIMPLE_COMPOSABLE
   SIMPLE = 1, COMPOSABLE = 0, SECURE = 1  → SIMPLE_SECURE
   SIMPLE = 0, COMPOSABLE = 1, SECURE = 1  → COMPOSABLE_SECURE
   SIMPLE = 1, COMPOSABLE = 0, SECURE = 0  → SIMPLE
   SIMPLE = 0, COMPOSABLE = 1, SECURE = 0  → COMPOSABLE
   SIMPLE = 0, COMPOSABLE = 0, SECURE = 1  → SECURE
   SIMPLE = 0, COMPOSABLE = 0, SECURE = 0  → SLOP

Comparing Programs (Profunctors)
--------------------------------

While the three quality pillars define a program's absolute placement on the evaluation lattice (the characteristic morphism), Topos also provides relational tools to measure the "distance" or "overlap" between two programs. In our category-theoretic model, these are **Profunctors**.

.. note::
   **Important:** Profunctors are comparative metrics. They are highly useful for agent workflows (e.g., "did this refactor actually change the structure?") but they **do not** influence the Quality Badges or the evaluation lattice.

Topos supports several relational metrics across its different graph representations:

*   **CFG Comparison:** Measures changes in cyclomatic complexity and edge distribution. (e.g., detecting if an agent added a new conditional branch).
*   **CPG Comparison:** Measures changes in dangerous API usage and taint flows, as well as general node-type overlap (Jaccard similarity).
*   **MDG Comparison:** Measures changes in coupling, fan-in/fan-out, and dependency depth.
*   **PDG Comparison:** Computes the Jaccard similarity of control and data dependencies between two versions of a function.
*   **AST Edit Distance:** Measures the topological drift between two programs using UAST edit distance.

Structural Test Coverage
~~~~~~~~~~~~~~~~~~~~~~~~

Topos can also use these comparative techniques to estimate how much of a **program-under-test (PUT)** appears in a **test suite** at the level of normalized UAST ``kind`` structure.

This is not line or branch coverage and does not prove that tests call production code; it answers a narrower question: *does the test code contain similar structural shapes (kinds, control-flow nodes, short kind paths) as the PUT?*

The CLI command is:

.. code-block:: bash

   topos structural-test-coverage --tests tests/test_mod.py src/mod.py

**Definitions (v0)**

Let :math:`n_P(k)` and :math:`n_T(k)` be raw counts of UAST kind :math:`k` in the PUT and in the aggregated test corpus.

*   **Kind recall:**
    .. math::

       R_{\text{kind}} = \frac{\sum_k \min\bigl(n_P(k), n_T(k)\bigr)}{\sum_k n_P(k)}

*   **Control-flow recall:** The same multiset recall formula is applied to the vector of counts returned by ``control_flow_profile``.

*   **Composite v0:**
    .. math::

       C_0 = \tfrac{1}{2} R_{\text{kind}} + \tfrac{1}{2} R_{\text{cf}}

**Definition (v1) — path recall**

For each source file, take the DFS pre-order sequence of kinds (same order as UAST edit distance). Build the multiset of length-:math:`k` consecutive kind tuples (*k-grams*).

Let :math:`c_P(g)` and :math:`c_T(g)` be counts of k-gram :math:`g` in PUT and tests.

*   **Path recall:**
    .. math::

       R_{\text{path}} = \frac{\sum_g \min\bigl(c_P(g), c_T(g)\bigr)}{\sum_g c_P(g)}

**Interpretation**

- Higher recalls mean more of the PUT’s counted structure is *also present* in tests.
- A **low** score suggests tests may be missing classes of syntax.
- A **high** score is **not** sufficient for quality: boilerplate, fixtures, or framework-heavy tests can overlap kinds without exercising semantics.
