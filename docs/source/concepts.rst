.. _concepts:

========
Concepts
========

This page explains the mathematical inspiration behind Topos and where the category-theoretic vocabulary comes from. For a practical breakdown of what we actually measure, see :doc:`measures`.

The Evaluation Lattice
----------------------

Topos classifies code against a four-valued **Heyting algebra** (a diamond lattice) — a partial
order that captures *degrees of structural confidence* rather than a single
pass/fail score. Each label describes what the metrics detect, not an
abstract quality judgment.

``COMPOSABLE`` and ``SELF_CONTAINED`` are *incomparable*: a function can be
structurally sound but highly coupled (``SELF_CONTAINED`` fails, but ``COMPOSABLE`` is reached), or entirely
self-contained with poor coupling (``SELF_CONTAINED`` reached, but ``COMPOSABLE`` fails). The partial order preserves this
distinction — Topos never collapses different failure modes without reason.

Metric verdicts within a dimension are combined via **meet** — the
greatest lower bound. The overall verdict is determined by combining the achievements of the independent pillars. Policy thresholds live in ``topos.logic.policies``.

Programs as Morphisms
---------------------

In category theory, a **morphism** is an arrow ``f: A -> B``. Topos models
source code the same way: a program is a morphism that transforms inputs
into outputs.

.. code-block:: python

   from topos import ProgramMorphism

   morphism = ProgramMorphism.from_file("transform.py")

Two programs may compute the same function but have dramatically different
internal structure. By modelling programs as morphisms and analysing their
ASTs, Topos can reason about *structural invariants* that input-output
testing cannot reveal.

The Subobject Classifier
------------------------

The **subobject classifier** answers membership questions: for any
subobject ``S`` of ``X`` there is a characteristic map into the lattice.
In Topos, that lattice is the four-valued diamond Heyting algebra, and the map
sends any program to its structural class — determining if it meets its targets per dimension.

.. code-block:: python

   from topos import ProgramMorphism, SubobjectClassifier

   morphism = ProgramMorphism.from_file("module.py")
   result = SubobjectClassifier().classify_detailed(morphism, [depgraph])
   print(result.dimensions)   # {"structural": SELF_CONTAINED, "coupling": BROKEN}
   print(result.summary())    # meet across dimensions (e.g., SELF_CONTAINED)

Dimensions are never automatically collapsed — call ``result.summary()``
only when a single scalar is required. Use ``combine_dimensions(results)``
to fold a directory scan into a per-dimension overall verdict.

Representations
---------------

Topos evaluates programs through pluggable **representations**, each
contributing metrics to a named dimension.

**ASTRepresentation** (``structural`` dimension)
   Built automatically from the ``ProgramMorphism``. Parses source into a
   tree-sitter AST and computes cyclomatic complexity and entropy. Targets the ``SELF_CONTAINED`` evaluation value.

**DependencyGraph** (``coupling`` dimension)
   Built from GitNexus output. Computes coupling metrics for the target
   file against the repository dependency graph. Supplied via
   ``--gitnexus-dir`` on the CLI, or passed directly to
   ``classify_detailed``. Targets the ``COMPOSABLE`` evaluation value.

Representations on the same dimension are aggregated via meet;
representations on different dimensions produce independent verdicts.