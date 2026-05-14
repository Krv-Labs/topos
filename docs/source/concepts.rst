.. _concepts:

========
Concepts
========

This page is optional deeper reading — you don't need it to use Topos. It explains the mathematical inspiration behind the design and where the category-theoretic vocabulary comes from. For a practical breakdown of what we actually measure, see :doc:`measures`.

The Evaluation Lattice
----------------------

Topos classifies code against an eight-valued **free Heyting algebra** (on three generators) — a partial
order that captures *degrees of independent quality* rather than a single
pass/fail score. Each label describes what the metrics detect, not an
abstract quality judgment.

The three generators — ``SIMPLE`` (code complexity), ``COMPOSABLE`` (module coupling), and ``SECURE`` (data flow safety) — are *pairwise incomparable*. A program can achieve any subset of {S, C, Sc}:

- ``SIMPLE`` only: low complexity but high coupling or security risk
- ``COMPOSABLE`` only: good module design but high complexity or taint exposure
- ``SECURE`` only: minimal dangerous operations but hard to understand or integrate
- ``SIMPLE`` + ``COMPOSABLE``: both structure and coupling good, but vulnerable
- ``SIMPLE`` + ``SECURE``: simple and secure, but tightly coupled
- ``COMPOSABLE`` + ``SECURE``: well-designed and secure, but complex
- ``IDEAL``: all three achieved

The partial order preserves these distinctions — Topos never collapses different failure modes without reason.

Metric verdicts within a generator are combined via **meet** — the greatest lower bound.
The overall verdict is determined by which generators scored ≥ 0.6. Policy thresholds and scoring live in ``topos.evaluation.policies``.

Programs as Morphisms
---------------------

In category theory, a **morphism** is an arrow ``f: A -> B``. Topos models
source code the same way: a program is a morphism that transforms inputs
into outputs.

.. code-block:: python

   from topos import ProgramMorphism

   morphism = ProgramMorphism.from_file("transform.py")

Two programs may compute the same function but have dramatically different
internal structure. By modeling programs as morphisms and analyzing their
ASTs, Topos can reason about *structural invariants* that input-output
testing cannot reveal.

The Subobject Classifier
------------------------

The **subobject classifier** answers membership questions: for any
subobject ``S`` of ``X`` there is a characteristic map into the lattice.
In Topos, that lattice is the eight-valued free Heyting algebra on three generators, and the map
sends any program to its quality class — determining if it meets its targets per generator.

.. code-block:: python

   from topos import CharacteristicMorphism, ModuleDependencyGraph, ProgramMorphism

   morphism = ProgramMorphism.from_file("module.py")
   mdg = ModuleDependencyGraph.from_gitnexus_dir(".gitnexus", "module.py")
   # CFG / PDG / CPG are derived intrinsically from the morphism's UAST:
   cfg = morphism.build_cfg()
   cpg = morphism.build_cpg()
   result = CharacteristicMorphism().classify_detailed(morphism, [cfg, cpg, mdg])
   print(result.dimensions)   # {"simple": SIMPLE, "composable": COMPOSABLE, "secure": SLOP}
   print(result.summary())    # SIMPLE_COMPOSABLE (both S and C achieved, not Sc)

Generators are never automatically collapsed — call ``result.summary()``
to get the combined 8-element lattice verdict. Use
``CharacteristicMorphism.combine_dimensions(results)`` to fold a
directory scan into a per-generator overall verdict.

Representations
---------------

Topos evaluates programs through pluggable **representations**, each
contributing metrics to a named generator.

**ControlFlowGraph** (``simple`` generator)
   Built automatically from the AST via tree-sitter. Computes cyclomatic complexity, essential complexity,
   nesting depth, longest path, and entropy. Targets the ``SIMPLE`` evaluation value. Always available.

**ModuleDependencyGraph** (``composable`` generator)
   Built from GitNexus output. Computes coupling, instability,
   fan-in/out, and dependency depth for the target file against the
   repository dependency graph. Supplied via ``--gitnexus-dir`` on the
   CLI, or passed directly to ``classify_detailed``. Targets the
   ``COMPOSABLE`` evaluation value. Requires a ``.gitnexus/``
   directory.

**CodePropertyGraph** (``secure`` generator)
   Fuses AST ∪ CFG ∪ DDG ∪ CDG into a single labeled multigraph
   (Yamaguchi et al., arxiv:1909.03496).  Built intrinsically from the
   UAST — no external tooling required.  Computes dangerous-API
   reachability and source→sink taint paths.  Targets the ``SECURE``
   evaluation value.  Always available.

**ProgramDependenceGraph** (diagnostic only)
   Intra-procedural control and data dependence graph. Computes dependence density. Not counted toward verdict.

Representations on the same generator are aggregated via meet (greatest lower bound);
representations on different generators produce independent verdicts.