.. _concepts:

========
Concepts
========

This page is optional deeper reading — you don't need it to use Topos. It explains the mathematical inspiration behind the design and where the category-theoretic vocabulary comes from. For a practical breakdown of what we actually measure, see :doc:`measures`.

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
internal structure. By modeling programs as morphisms and analyzing their
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

The Universal AST (UAST)
-----------------------

Topos evaluates code through a **Universal Abstract Syntax Tree (UAST)**—a language-normalized representation that bridges the gap between raw, language-specific parsing and high-level cross-language analysis.

UAST follows a **"Native-first, Normalized-second"** architecture:

1. **The Tree-sitter Engine**: We use Tree-sitter to generate a **Concrete Syntax Tree (CST)**. Tree-sitter is industry-leading, highly maintained, and provides fast, incremental parsing.
2. **The Normalization Layer**: The UAST acts as a filter over the CST. It ignores surface-level noise (punctuation, whitespace) and maps language-specific nodes to a set of unified `UNodeKind` values (e.g., ``FunctionDecl``, ``IfStmt``, ``CallExpr``).
3. **Fidelity and Provenance**: Crucially, every UAST node retains a ``NativeRef``. This preserves the exact byte-offsets and native parser provenance, ensuring the representation remains faithful to established industry standards like Python ``ast``, ESTree (JS), Rust ``syn``, and the Clang AST (C++).

This unified layer allows Topos algorithms (like complexity analysis) to operate identically across multiple languages without losing the precision required for professional-grade static analysis.

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