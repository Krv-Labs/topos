.. _concepts:

========
Concepts
========

Topos applies category theory and intuitionistic logic to code quality evaluation.
You don't need to understand the math to use Topos — this page explains the
ideas behind the design.

The Evaluation Lattice
----------------------

At the heart of Topos is a **Heyting algebra** of evaluation values. Unlike a
simple pass/fail test, this algebra captures *degrees of confidence* about
code quality.

The values form a partial order:

.. mermaid::

   graph BT
      INVALID["⊥ INVALID"]
      HALLUCINATED["○ HALLUCINATED"]
      NOISY["◑ NOISY"]
      WEAK["◒ WEAK"]
      COMMODITY["◐ COMMODITY"]
      VERIFIED["⊤ VERIFIED"]

      INVALID --> HALLUCINATED
      INVALID --> NOISY
      INVALID --> WEAK
      INVALID --> COMMODITY
      HALLUCINATED --> VERIFIED
      NOISY --> COMMODITY
      WEAK --> COMMODITY
      COMMODITY --> VERIFIED

This is a **partial order**, not a total order. ``NOISY`` and ``WEAK`` are
*incomparable* — they represent qualitatively different concerns. A function
might be branching-heavy (high complexity) but well-structured, or simple but
repetitive. A partial order lets us track distinct failure modes without
collapsing them onto a single axis.

When combining multiple metric evaluations, Topos uses **meet (∧)** — the
greatest lower bound. The overall evaluation is only as good as the weakest
signal, so no single metric can hide a problem revealed by another.

Programs as Morphisms
---------------------

In category theory, a **morphism** is an arrow ``f: A → B`` that transforms
objects. Topos views programs the same way: source code is a morphism that
transforms inputs into outputs.

The ``ProgramMorphism`` class captures this:

.. code-block:: python

   from topos import ProgramMorphism

   morphism = ProgramMorphism.from_file("transform.py")
   print(morphism.ast.node_count)
   print(morphism.ast.depth)

Two programs may compute the same function but have dramatically different
internal structure. By modeling programs as morphisms and parsing their ASTs,
Topos can reason about *structural invariants* that input-output testing cannot reveal.

The Subobject Classifier
------------------------

In a Topos, the **subobject classifier** ``Ω`` is the object that answers
membership questions. For any subobject ``S ⊆ X``, there is a unique
**characteristic map** ``χ: X → Ω`` that classifies membership.

In the category of Sets, ``Ω = {true, false}``. In our Topos of Programs,
``Ω`` is the six-valued Heyting algebra, and ``χ`` captures *degrees* of
membership in the class of well-structured code.

The ``SubobjectClassifier`` implements this map:

.. code-block:: python

   from topos import ProgramMorphism, SubobjectClassifier

   morphism = ProgramMorphism.from_file("module.py")
   result = SubobjectClassifier().classify(morphism)
   print(result)  # e.g., "◐ COMMODITY"

The map factors through two stages:

1. **Metrics** — extract complexity and entropy from the AST
2. **Policies** — map metric values to evaluation values via threshold bins
3. **Aggregation** — combine metric verdicts using lattice meet

This separates *what we measure* from *how we interpret it*, making the
system transparent and extensible.
