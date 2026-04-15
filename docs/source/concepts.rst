.. _concepts:

========
Concepts
========

This page explains how verdicts connect to metrics and where the
category-theoretic vocabulary comes from. You can use Topos without reading
past the first section; the later sections add precision for extending
policies, debugging a verdict, or auditing results.

The Evaluation Lattice
----------------------

Topos classifies code against a six-valued **Heyting algebra** — a partial
order that captures *degrees of structural confidence* rather than a single
pass/fail score. Each label describes what the metrics detect, not an
abstract quality judgment.

``COUPLED`` and ``COMPLEX`` are *incomparable*: a function can be
branching-heavy (``COMPLEX``) while having normal entropy, or structurally
anomalous with tight coupling (``COUPLED``). The partial order preserves this
distinction — Topos never collapses different failure modes without reason.

Metric verdicts within a dimension are combined via **meet** — the
greatest lower bound. The dimension verdict is only as strong as the weakest
signal. Policy thresholds live in ``topos.logic.policies``.

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
In Topos, that lattice is the six-valued Heyting algebra, and the map
sends any program to its structural class — one verdict per dimension.

.. code-block:: python

   from topos import ProgramMorphism, SubobjectClassifier

   morphism = ProgramMorphism.from_file("module.py")
   result = SubobjectClassifier().classify_detailed(morphism, [depgraph])
   print(result.dimensions)   # {"structural": SOUND, "coupling": COMPLEX}
   print(result.summary())    # worst across dimensions

Dimensions are never automatically collapsed — call ``result.summary()``
only when a single scalar is required. Use ``combine_dimensions(results)``
to fold a directory scan into a per-dimension overall verdict.

Representations
---------------

Topos evaluates programs through pluggable **representations**, each
contributing metrics to a named dimension.

**ASTRepresentation** (``structural`` dimension)
   Built automatically from the ``ProgramMorphism``. Parses source into a
   tree-sitter AST and computes cyclomatic complexity and entropy.

**DependencyGraph** (``coupling`` dimension)
   Built from GitNexus output. Computes coupling metrics for the target
   file against the repository dependency graph. Supplied via
   ``--gitnexus-dir`` on the CLI, or passed directly to
   ``classify_detailed``.

Representations on the same dimension are aggregated via meet;
representations on different dimensions produce independent verdicts.

Metrics
-------

Each representation emits namespaced metric keys that are mapped to lattice
values through policy thresholds in ``topos.logic.policies`` — see
``structural.py`` (AST metrics) and ``coupling.py`` (dependency-graph
metrics).

**Structural dimension** — ``ASTRepresentation``

- ``ast.complexity`` — cyclomatic complexity: number of linearly independent
  paths through the code. Higher values always produce a lower verdict.

- ``ast.entropy`` — Kolmogorov-proxy entropy via compression ratio.
  Very low entropy signals repetitive/boilerplate code; very high signals
  unusual structure (generated or incoherent). The healthy range is near 0.5.

**Coupling dimension** — ``DependencyGraph``

- ``depgraph.coupling`` — total coupling (afferent + efferent). More
  coupling always produces a lower verdict.

- ``depgraph.instability`` — ``Ce / (Ca + Ce)``. Both extremes are bad:
  near 0 is hard to evolve, near 1 depends on everything. The balanced
  range (0.3--0.7) produces ``SOUND``.

- ``depgraph.fan_in``, ``depgraph.fan_out``, ``depgraph.dep_depth`` —
  diagnostic metrics visible in ``topos inspect`` output but not used in
  verdicts.
