.. _measures:

========
Measures
========

.. tip::
   Every program evaluated by Topos is measured along four independent **Quality Pillars**. These pillars are the generators for the **Quality Medals** you can earn. Topos never collapses these into a single number — you always see which pillar is the problem.

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

2. The COMPOSABLE Pillar (Outward Dependency Burden)
-----------------------------------------------------

Evaluates how much external behavior a file coordinates by analyzing the dependency graph. *(Requires GitNexus)* The COMPOSABLE pillar maps to the ``COMPOSABLE`` badge outcome.

* **Coupling** (``mdg.coupling``)
  The total number of afferent (incoming) and efferent (outgoing) dependencies. High total coupling negatively impacts the COMPOSABLE score.

* **Instability** (``mdg.instability``)
  Calculated as ``Efferent / (Afferent + Efferent)``.

  - Near 0: The module is a rigid dependency for many others and is hard to change safely.
  - Near 1: The module is highly unstable because it depends on many other parts of the system.
  - A balanced range (0.3 – 0.7) helps achieve a higher COMPOSABLE score.

* **Fan-out** (``mdg.fan_out``)
  Counts distinct external symbols called by the file. This is the v0.5 file-level gate: ``fan_out <= 10``.

* **Fan-in** (``mdg.fan_in``)
  Counts incoming call edges. It remains scored and actionable as a responsibility/change-impact signal, but is advisory because a stable interface or shared utility can legitimately have many callers.

* **Dependency Depth** (``mdg.dep_depth``)
  The longest dependency chain from this module. Shallow chains are easier to understand and refactor.

3. The SECURE Pillar (Vulnerability Analysis)
-------------------------------------------------

Evaluates whether the code flow can reach dangerous operations or untrusted data.  Computed from the Code Property Graph (CPG) — derived intrinsically from the UAST, no external tooling required.  The SECURE pillar maps to the ``SECURE`` badge outcome.


* **Dangerous Calls** (``cpg.dangerous_calls``)
  Count of reachable call sites matching a per-language registry of dangerous APIs (Python: ``eval``, ``exec``, ``pickle.loads``, …; C++: ``gets``, ``strcpy``, …).  Lower counts improve the SECURE score.

* **Taint Flows** (``cpg.taint_flows``)
  Source→sink data-flow paths along the CPG's data-dependence edges, from untrusted sources (e.g. ``input``, ``request.args``) to dangerous sinks. Longer taint chains increase risk.

.. note::
   The embedded `Sighthound <https://github.com/Corgea/Sighthound>`_ SAST
   engine supplies supplementary ``security_findings`` detail (per-finding
   callee, line, taint source/sink) for Python/JavaScript/TypeScript/Go —
   but ``cpg.dangerous_calls``/``cpg.taint_flows`` above, and therefore the
   SECURE score itself, always come from the native CPG probes. Sighthound
   never feeds SECURE.

4. The NAVIGABLE Pillar (Agentic Cognitive Load)
-------------------------------------------------

.. versionadded:: 0.5.0

Evaluates how expensive the code is for an LLM agent to read, reason over,
and safely change. Computed from the AST scope tree — no external tooling
required, so NAVIGABLE is always available.

* **Worst-function nesting divergence** (``nav.max_function_divergence``)
  The maximum **Semantic Compositional Divergence** over the file's
  functions:

  .. math::

     SCD(f) = \sum_{u \in f} \operatorname{depth}(u) \cdot \ln(1 + \operatorname{fanout}(u))

  where :math:`\operatorname{depth}(u)` is a scope's nesting level relative
  to the function body and :math:`\operatorname{fanout}(u)` its count of
  immediate child scopes.

**Why nesting, and not complexity.** Once code length is controlled for,
classical complexity metrics stop correlating with LLM task accuracy —
nesting depth keeps correlating. Each level is another hierarchical state
the model must hold open while reading forward. Two properties of the
formula follow deliberately from that:

* A leaf scope contributes :math:`\ln(1) = 0`, so a **perfectly flat
  function scores 0.0** no matter how many branches it has. Flat *is*
  maximally navigable; branch count is SIMPLE's concern. Deep code is still
  fully counted — the weight lands on the ancestors doing the nesting.
* Ternaries and short-circuit boolean operators are **excluded**.
  Expression-level branching opens no block, so it costs no reader state,
  and counting it would just re-measure SIMPLE.

Like ``ast.max_function_complexity``, the gate is the **per-function
maximum** rather than a file-wide sum — a long file of short, flat
functions must not fail merely for its length. When the gate fails, Topos
reports the offending functions worst-first with real line spans, so the
failure becomes an actionable refactor target: extract the deepest nested
block into a top-level helper.

.. note::
   The NAVIGABLE threshold is calibrated from a balanced 6,390-file
   multi-ecosystem leaderboard corpus. The achieved gate is ``10.0`` (p95
   ``10.37``, ~5.2% failure rate). The score cap is ``12.0`` (spans p99 across
   Rust ``10.40``, Go ``13.64``, and Python ``12.31``). Topos's 176 Rust
   sources remain a reference ECDF (p95
   ``5.65``, p99 ``8.62``, max ``12.19``).

Scoring and Manager Priorities
------------------------------

Topos produces a continuous normalized score ``[0.0, 1.0]`` for each pillar.
A pillar is **achieved** when its independent raw gate or gates pass. Score
floors are used by aggregate morphism paths and continuous scores preserve
advisory detail; they are not the live file-level verdict rule.

.. list-table::
   :widths: 20 20 60
   :header-rows: 1

   * - Pillar
     - Threshold
     - Raw Requirement (Policy Φᵢ)
   * - **SIMPLE**
     - ``0.40``
     - ``max_func <= 10`` AND ``entropy in [0.2, 0.8]``; cyclomatic is advisory
   * - **COMPOSABLE**
     - ``0.80``
     - ``fan_out <= 10``; fan-in and stability readings remain advisory
   * - **SECURE**
     - ``1.00``
     - Zero ``dangerous_calls`` AND zero ``taint_flows``
   * - **NAVIGABLE**
     - ``0.40``
     - ``max_function_divergence <= 10.0``

Scores are reported as percentages (0–100%) in all CLI and MCP output.
Note that while the thresholds are used for score-floor aggregation, the
authoritative achievement of a pillar is determined by the independent
AND of the raw metric requirements defined in each generator's policy.

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

Calibration against real corpora
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The thresholds above are not arbitrary. They are tuned so that the medal tiers
track how mature, widely-trusted Python libraries actually score. Below, three
reference codebases — ``requests``, ``numpy``, and ``pandas`` — measured pillar
by pillar and by the resulting medal mix.

.. raw:: html

   <figure class="topos-figure">
     <img class="only-light" src="_static/figures/topos-library-profiles.svg" alt="Average SIMPLE, COMPOSABLE, and SECURE scores for the requests, numpy, and pandas libraries." />
     <img class="only-dark" src="_static/figures/topos-library-profiles-dark.svg" alt="" aria-hidden="true" />
     <figcaption>Average pillar scores per library. Security clears its bar consistently; simplicity is the pillar most codebases leave on the table.</figcaption>
   </figure>

.. raw:: html

   <figure class="topos-figure">
     <img class="only-light" src="_static/figures/topos-medal-mix.svg" alt="Distribution of GOLD, SILVER, BRONZE, and SLOP medals across files in each reference library." />
     <img class="only-dark" src="_static/figures/topos-medal-mix-dark.svg" alt="" aria-hidden="true" />
     <figcaption>The per-file medal distribution that those thresholds produce.</figcaption>
   </figure>

Verdicts
--------

The per-pillar scores map to a 16-valued Heyting algebra (free lattice on 4 generators), representing the **Quality Medals**:

The medal tier is the **count** of pillars achieved; *which* ones you
achieved is named by the ``lattice_element`` and matters for diagnosis:

.. list-table::
   :header-rows: 1
   :widths: 20 15 65

   * - Pillars
     - Medal
     - Verdicts
   * - 4 of 4
     - 🏆 ``PLATINUM``
     - ``IDEAL``
   * - 3 of 4
     - 🥇 ``GOLD``
     - ``SIMPLE_COMPOSABLE_SECURE``, ``SIMPLE_COMPOSABLE_NAVIGABLE``, ``SIMPLE_SECURE_NAVIGABLE``, ``COMPOSABLE_SECURE_NAVIGABLE``
   * - 2 of 4
     - 🥈 ``SILVER``
     - ``SIMPLE_COMPOSABLE``, ``SIMPLE_SECURE``, ``SIMPLE_NAVIGABLE``, ``COMPOSABLE_SECURE``, ``COMPOSABLE_NAVIGABLE``, ``SECURE_NAVIGABLE``
   * - 1 of 4
     - 🥉 ``BRONZE``
     - ``SIMPLE``, ``COMPOSABLE``, ``SECURE``, ``NAVIGABLE``
   * - 0 of 4
     - ❌ none
     - ``SLOP`` (all gates failed, or a syntax error)

.. versionchanged:: 0.5.0
   ``IDEAL`` now requires all four pillars and awards ``PLATINUM``. The
   verdict formerly called ``IDEAL`` — the top of the three-generator
   algebra — is now ``SIMPLE_COMPOSABLE_SECURE`` and bands as ``GOLD``.

The four pillars ``SIMPLE``, ``COMPOSABLE``, ``SECURE``, and ``NAVIGABLE``
are **pairwise incomparable** — a file can achieve any subset of them
independently. The overall ``lattice_element`` is determined by which
combination of pillars passed their calibrated gates:

.. code-block:: text

   SIMPLE  COMPOSABLE  SECURE  NAVIGABLE  → verdict
        1           1       1          1  → IDEAL                        🏆
        1           1       1          0  → SIMPLE_COMPOSABLE_SECURE     🥇
        1           1       0          1  → SIMPLE_COMPOSABLE_NAVIGABLE  🥇
        1           1       0          0  → SIMPLE_COMPOSABLE            🥈
        1           0       0          1  → SIMPLE_NAVIGABLE             🥈
        1           0       0          0  → SIMPLE                       🥉
        0           0       0          0  → SLOP                         ❌

``COMPOSABLE`` requires a GitNexus dependency graph and ``SECURE`` requires
a CPG; either is reported as *not measured* rather than *failed* when its
input is unavailable. ``SIMPLE`` and ``NAVIGABLE`` need only the file
itself, so they are always evaluated.

Comparing Programs (Profunctors)
--------------------------------

While the four quality pillars define a program's absolute placement on the evaluation lattice (the characteristic morphism), Topos also provides relational tools to measure the "distance" or "overlap" between two programs. In our category-theoretic model, these are **Profunctors**.

.. note::
   **Important:** Profunctors are comparative metrics. They are highly useful for agent workflows (e.g., "did this refactor actually change the structure?") but they **do not** influence the Quality Badges or the evaluation lattice.

Topos supports several relational metrics across its different graph representations:

*   **CFG Comparison:** Measures changes in cyclomatic complexity and edge distribution. (e.g., detecting if an agent added a new conditional branch).
*   **CPG Comparison:** Measures changes in dangerous API usage and taint flows, as well as general node-type overlap (Jaccard similarity).
*   **MDG Comparison:** Measures changes in coupling, fan-in/fan-out, and dependency depth.
*   **PDG Comparison:** Computes the Jaccard similarity of control and data dependencies between two versions of a function.
*   **AST Edit Distance:** Measures the topological drift between two programs using UAST edit distance.

Refactor Suite (also not scored)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Beyond profunctor comparisons, the ``topos_refactor`` MCP tool (and, for
Graphify, the ``topos graphify`` CLI subcommand) surfaces ranked structural
hotspots from four more engines — CFG cycle basis, MDG/process-graph
curvature, and Graphify knowledge-graph degree/confidence. Like the
profunctors above, **none of these feed the evaluation lattice**; they're
refactoring guidance layered on top. See :doc:`agents` and the repository's
``docs/decisions/refactor-suite.md`` for the full design.

Structural Test Coverage
~~~~~~~~~~~~~~~~~~~~~~~~

Topos uses **Declaration-level Bipartite Coverage** to estimate how much of a
**program-under-test (PUT)** appears in a **test suite** at the level of
normalized UAST structure.

Unlike line or branch coverage, this method does not require code execution.
It answers: *does the test code contain similar structural shapes (kinds,
control-flow nodes, kind paths) as the declarations in the PUT?*

The CLI command is:

.. code-block:: bash

   topos coverage --tests tests/test_mod.py src/mod.py

**How it works**

1. **Extraction:** Every ``FunctionDecl`` and ``MethodDecl`` is extracted from
   both the PUT and the test suite.
2. **Fingerprinting:** Each declaration is fingerprinted by the multiset of
   UAST kinds (excluding the root declaration kind itself) in its body.
3. **Bipartite Matching:** Each PUT declaration is matched against the
   best-matching declaration in the test suite using multiset recall.
4. **Scoring:**

   - **Mean Declaration Coverage:** The average best-match recall across all
     PUT declarations.
   - **F2 Score:** A harmonic mean that combines declaration recall with
     **test precision**, biased heavily toward recall (F2). This penalizes
     bloated test suites that contain large amounts of code unrelated to the PUT.
   - **Uncovered Declarations:** The tool identifies specific locations in the
     source code that lack corresponding structural representation in the tests.

**Interpretation**

- Higher mean coverage indicates more of the PUT’s structural declarations have matches in the test suite.
- An F2 score significantly lower than mean coverage indicates a bloated test suite.
- A **low** score suggests tests may be missing classes of syntax present in the PUT.
