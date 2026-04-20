.. _measures:

========
Measures
========

Every program evaluated by Topos is measured along two independent dimensions, or pillars. Topos never collapses these dimensions into a single number — you always see which axis is the problem.

1. The Structural Pillar (Complexity)
-------------------------------------

Evaluates the internal quality of the code by analyzing the Abstract Syntax Tree (AST). The structural pillar maps to the ``SELF_CONTAINED`` lattice target.

* **Cyclomatic Complexity** (``ast.complexity``)
  Measures the number of linearly independent paths through the code. Branches, loops, and conditionals increase complexity. Higher values negatively impact the structural score.

* **Entropy** (``ast.entropy``)
  A Kolmogorov-complexity proxy using compression ratios. It measures how predictable the code is. Very low entropy suggests excessive boilerplate; very high entropy signals chaotic or highly unusual structure (often seen in hallucinated code). The healthy range sits around 0.5.

2. The Coupling Pillar
----------------------

Evaluates how a file fits into the broader repository by analyzing the dependency graph. *(Requires GitNexus)* The coupling pillar maps to the ``COMPOSABLE`` lattice target.

* **Coupling** (``depgraph.coupling``)
  The total number of afferent (incoming) and efferent (outgoing) dependencies. High total coupling negatively impacts the coupling score.

* **Instability** (``depgraph.instability``)
  Calculated as ``Efferent / (Afferent + Efferent)``.

  - Near 0: The module is a rigid dependency for many others and is hard to change safely.
  - Near 1: The module is highly unstable because it depends on many other parts of the system.
  - A balanced range (0.3 – 0.7) helps achieve a higher coupling score.

* **Fan-in / Fan-out** (``depgraph.fan_in``, ``depgraph.fan_out``)
  Diagnostic metrics tracking explicit call edges. These are visible in detailed inspections but don't strictly set the final verdict.

Scoring and Priorities
----------------------

Topos produces a continuous normalized score ``[0.0, 1.0]`` for each dimension.
A score at or above the threshold (default **0.6**) means the lattice target for
that dimension is achieved. Scores are reported as percentages (0–100%) in all
CLI and MCP output.

**Structural score** (determines ``SELF_CONTAINED``):

.. code-block:: text

   complexity_quality = 1 - min(complexity / 40, 1.0)
   entropy_quality    = max(0, 1 - 2 × |entropy - 0.5|)     ← peak at 0.5
   structural_score   = w_c × complexity_quality + (1 - w_c) × entropy_quality

**Coupling score** (determines ``COMPOSABLE``, requires GitNexus):

.. code-block:: text

   coupling_quality    = 1 - min(coupling / 35, 1.0)
   instability_quality = tent function over [0.3, 0.7]        ← 1.0 in optimal range
   coupling_score      = w_k × coupling_quality + (1 - w_k) × instability_quality

The weights ``w_c`` and ``w_k`` are controlled by the **Priority**:

.. list-table::
   :widths: 20 20 20 60
   :header-rows: 1

   * - Priority
     - w_c (structural)
     - w_k (coupling)
     - Effect
   * - ``balanced``
     - 0.5
     - 0.5
     - Equal emphasis on complexity and coupling quality
   * - ``self_contained``
     - 0.7
     - 0.3
     - Upweights complexity quality; rewards low-complexity code
   * - ``composable``
     - 0.3
     - 0.7
     - Upweights coupling quality; rewards tightly-bounded interfaces

Changing the priority does not change what is measured — it changes what the
agent is rewarded for optimizing within each dimension.

Verdicts
--------

The per-dimension scores map to a four-valued diamond lattice (Heyting algebra):

* ``BROKEN`` (⊥): Fails both targets (scores below threshold), or syntax error.
* ``COMPOSABLE`` (◑): Coupling score ≥ threshold. Composes well with other modules.
  Requires a DependencyGraph — unreachable from AST metrics alone.
* ``SELF_CONTAINED`` (◐): Structural score ≥ threshold. Stands alone cleanly.
* ``SOUND`` (⊤): Both targets achieved. Clean, composable, and self-contained.

``COMPOSABLE`` and ``SELF_CONTAINED`` are **incomparable** in the lattice — a
file can achieve one without the other. The overall ``lattice_element`` in the
response is determined by which combination of targets was reached:

.. code-block:: text

   Both achieved        → SOUND
   Structural only      → SELF_CONTAINED
   Coupling only        → COMPOSABLE
   Neither              → BROKEN