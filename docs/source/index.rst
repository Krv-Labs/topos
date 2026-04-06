.. _index:

=====
Topos
=====

**Treating programs as morphisms in a world of commodity code.**

Topos is a code quality evaluation tool that maps every Python program to one of six
evaluation values using cyclomatic complexity and entropy metrics. Instead of a numeric
score, you get a lattice position that encodes structural quality with partial confidence.

The six evaluation values form a `Heyting algebra <https://en.wikipedia.org/wiki/Heyting_algebra>`_:

.. list-table::
   :widths: 10 20 70
   :header-rows: 1

   * - Symbol
     - Value
     - Meaning
   * - ⊥
     - ``INVALID``
     - Fails to parse; syntactically broken
   * - ○
     - ``HALLUCINATED``
     - Parses but appears structurally pathological
   * - ◑
     - ``NOISY``
     - Valid code with high repetition or unstable structure
   * - ◒
     - ``WEAK``
     - Functional with elevated structural risk
   * - ◐
     - ``COMMODITY``
     - Functional with recoverable concerns
   * - ⊤
     - ``VERIFIED``
     - Maintainable, well-structured, and human-aligned

.. toctree::
   :maxdepth: 1
   :caption: Documentation
   :hidden:

   installation
   getting_started
   concepts
