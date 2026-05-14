.. _structural-test-coverage:

============================
Structural test coverage
============================

Topos can estimate how much of a **program-under-test (PUT)** appears in a **test suite** at the level of normalized UAST ``kind`` structure.

A Markdown version of this material (suitable for repo browsing outside Sphinx) lives at ``docs/structural-test-coverage.md``. This is not line or branch coverage and does not prove that tests call production code; it answers a narrower question: *does the test code contain similar structural shapes (kinds, control-flow nodes, short kind paths) as the PUT?*

The implementation lives in ``topos.functors.profunctors.uast.structural_test_coverage``. The CLI command is:

.. code-block:: bash

   topos structural-test-coverage --tests tests/test_mod.py src/mod.py

Definitions (v0)
----------------

Let :math:`n_P(k)` and :math:`n_T(k)` be raw counts of UAST kind :math:`k` in the PUT and in the aggregated test corpus (multiple files are merged by summing counts). Unknown kinds may be excluded; the CLI defaults to excluding them for stability.

**Kind recall**

.. math::

   R_{\text{kind}} = \frac{\sum_k \min\bigl(n_P(k), n_T(k)\bigr)}{\sum_k n_P(k)}

If the denominator is zero (no counted nodes in the PUT), the recall is defined as **1.0** (vacuous).

**Control-flow recall**

The same multiset recall formula is applied to the vector of counts returned by ``control_flow_profile`` — only kinds in ``CONTROL_FLOW_KINDS`` (loops, branches, calls, returns, etc.). Again, an empty PUT control-flow multiset yields **1.0**.

**Composite v0**

.. math::

   C_0 = \tfrac{1}{2} R_{\text{kind}} + \tfrac{1}{2} R_{\text{cf}}

Definition (v1) — path recall
-----------------------------

For each source file, take the DFS pre-order sequence of kinds (same order as UAST edit distance). Build the multiset of length-:math:`k` consecutive kind tuples (*k-grams*). Aggregate k-grams across files by summing counts; **k-grams never span file boundaries**.

Let :math:`c_P(g)` and :math:`c_T(g)` be counts of k-gram :math:`g` in PUT and tests.

**Path recall**

.. math::

   R_{\text{path}} = \frac{\sum_g \min\bigl(c_P(g), c_T(g)\bigr)}{\sum_g c_P(g)}

If the PUT has no k-grams (e.g. sequence shorter than :math:`k`), the score is **1.0** (vacuous).

Interpretation
--------------

- Higher recalls mean more of the PUT’s counted structure is *also present* in tests — useful as a **sanity signal** or to compare two test strategies on the same PUT.
- A **low** score suggests tests may be missing classes of syntax (e.g. few loops vs a loop-heavy PUT).
- A **high** score is **not** sufficient for quality: boilerplate, fixtures, or framework-heavy tests can overlap kinds without exercising semantics. There is **no** static or dynamic call linkage in v0/v1.

Diagnostics on the CLI and in ``StructuralTestCoverageReport`` (node masses, k-gram totals) help spot size asymmetry and boilerplate dominance.

Further reading
---------------

- Evaluation script and field notes: ``demos/structural_test_coverage/run_evaluation.py`` and ``demos/structural_test_coverage/EVALUATION.md``.
- Related symmetric distances between two programs: :doc:`measures` (lattice metrics) and ``compare_uast`` in ``topos.functors.profunctors.uast``.
