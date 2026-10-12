.. _measures:

========
Measures
========

.. tip::
   Every program evaluated by Topos is measured along four independent **Quality Pillars**. These pillars are the generators for the **Quality Medals** you can earn. Topos never collapses these into a single number — you always see which pillar is the problem.

1. The SIMPLE Pillar (Code Complexity)
------------------------------------------

Evaluates the internal quality of the code by analyzing the Control Flow Graph (CFG) and Abstract Syntax Tree (AST). The SIMPLE pillar always runs and maps to the ``SIMPLE`` badge outcome.

* **Max Function Complexity** (``ast.max_function_complexity``)
  The highest McCabe complexity of any single function. **Gate:** ``<= 10``.

* **Cyclomatic Complexity** (``cfg.cyclomatic``)
  Measures the number of linearly independent paths through the whole file. *Advisory*, read relative to the codebase (see :ref:`scoring`): it is a whole-file sum that grows with function count, so ``ast.max_function_complexity`` gates the per-function concern instead.

* **Essential Complexity** (``cfg.essential``)
  Counts unstructured control flow that cannot be reduced to single-entry, single-exit blocks. *Advisory*, codebase-relative.

* **Nesting Depth** (``cfg.nesting_depth``)
  Maximum nesting level of control structures. Deeper nesting is harder to reason about. *Advisory*, codebase-relative.

* **Longest Path** (``cfg.longest_path``)
  Longest acyclic execution path through the CFG. Long paths correlate with high cognitive load.

* **Entropy** (``ast.entropy``)
  A Kolmogorov-complexity proxy using compression ratios. It measures how predictable the code is. Very low entropy suggests excessive boilerplate; very high entropy signals chaotic or highly unusual structure (often seen in hallucinated code). The healthy range sits around 0.5. **Gate:** ``0.2 <= entropy <= 0.8``.

2. The COMPOSABLE Pillar (Outward Dependency Burden)
-----------------------------------------------------

Evaluates how much external behavior a file coordinates by analyzing the dependency graph. *(Requires GitNexus)* The COMPOSABLE pillar maps to the ``COMPOSABLE`` badge outcome.

* **Coupling** (``mdg.coupling``)
  The total number of afferent (incoming) and efferent (outgoing) dependencies. Reported for diagnosis.

* **Instability** (``mdg.instability``)
  Calculated as ``Efferent / (Afferent + Efferent)``.

  - Near 0: The module is a rigid dependency for many others and is hard to change safely.
  - Near 1: The module depends on many other parts of the system.

  *Advisory*, codebase-relative. There is no fixed "good" band: a file is
  compared with the rest of its codebase and the language prior. Only files
  with ``coupling >= 2`` get a reading, because below that the ratio carries
  no information.

* **Fan-out** (``mdg.fan_out``)
  Counts distinct external symbols called by the file. **Gate:** ``fan_out <= 10``.

* **Fan-in** (``mdg.fan_in``)
  Counts incoming call edges. *Advisory*, codebase-relative: a responsibility and change-impact signal, never a gate, because a stable interface or shared utility can legitimately have many callers.

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
   The NAVIGABLE gate is calibrated from a balanced 6,390-file
   multi-ecosystem leaderboard corpus: ``max_function_divergence <= 10.0``
   (p95 ``10.37``, ~5.2% failure rate, matching the ~5.5% target used for
   SIMPLE and SECURE).

.. _scoring:

How Scores Work
---------------

.. versionchanged:: 0.10.0
   Scores are gate-anchored and advisory metrics are read relative to the
   codebase. Score floors and fixed advisory bands are retired, and priority
   no longer reweights scores.

Each pillar gets a score from 0 to 100 that is **anchored at its gates**:
**50 is exactly at the gate, and a pillar passes if and only if its score is
at least 50.** A score and its verdict can never disagree.

1. **Gated metrics** map to a desirability ``d`` in ``[0, 1]`` with
   ``d = 0.5`` exactly at the gate. Lower-is-better gates fall linearly from
   1 at zero, to 0.5 at the gate, to 0 at twice the gate. The entropy band is
   1 at 0.5 and 0.5 at its edges. Zero-tolerance counts (SECURE) are 1 at zero
   and below 0.5 otherwise.
2. The pillar's **gate score** ``G`` (reported as ``gate_scores``) is the
   *minimum* over its gates, so the worst gate decides.
3. **Advisory metrics** (cyclomatic, essential and nesting complexity,
   fan-in, instability) never fail a pillar. Each is read as a percentile
   among the files in the same scan, shrunk toward a per-language prior
   derived from top open-source packages:

   .. math::

      \tilde F(x) = w \, F_{\text{local}}(x) + (1 - w) \, F_{\text{lang}}(x),
      \qquad w = \frac{n}{n + k}

   where :math:`n` is the number of files in the scan and :math:`k` is a
   per-metric shrinkage constant from the prior table. The geometric mean
   :math:`A` of the advisory qualities moves the score only **within the half
   its gates chose**: :math:`S = 50 + (G - 50)\,A^{1/3}` when passing and
   :math:`S = G\,A^{1/3}` when failing.

Each reading in the ``advisories`` field reports ``value``,
``relative_percentile``, ``global_percentile`` (against the language prior),
``local_weight``, ``quality``, and ``flagged``. A flagged advisory means
"atypical for this codebase and for top open-source code", not "buggy".
Project scans (``topos evaluate -r``, ``topos_evaluate_project``) use the
scanned files as the population; single-file calls read advisories against
the language prior alone. A language without its own prior table uses the
pooled ``_all`` prior.

.. list-table::
   :widths: 20 80
   :header-rows: 1

   * - Pillar
     - Gates (all must pass; ``achieved`` is their AND)
   * - **SIMPLE**
     - ``ast.max_function_complexity <= 10`` AND ``0.2 <= ast.entropy <= 0.8``
   * - **COMPOSABLE**
     - ``mdg.fan_out <= 10``
   * - **SECURE**
     - ``cpg.dangerous_calls = 0`` AND ``cpg.taint_flows = 0``
   * - **NAVIGABLE**
     - ``nav.max_function_divergence <= 10.0``

Scores are reported as percentages in all CLI and MCP output.

**Priority** (``--priority``) only orders the pillars. It names the pillar to
work on next and breaks ties; it does not change what is measured or how
scores are computed. See :doc:`preferences`.

Calibration
~~~~~~~~~~~

The gates live in one place,
``topos/engine/src/evaluation/policies/calibration.rs``, which records the
corpus behind each value: SIMPLE, COMPOSABLE and SECURE were calibrated on
top PyPI packages, and NAVIGABLE on a balanced six-ecosystem leaderboard
corpus. The advisory priors sit beside them in
``policies/advisory_priors.json``. That file is a pinned snapshot generated
from the Topos leaderboard (top packages across PyPI, Cargo, npm, Go and MCP
servers) and embedded in the binary, so scores are reproducible and work
offline. Topos never downloads it at run time.

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

Beyond profunctor comparisons, the ``topos_refactor`` MCP tool surfaces
ranked structural hotspots from three more engines — CFG cycle basis and
MDG/process-graph curvature. Like the
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
