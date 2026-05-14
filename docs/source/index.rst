.. _index:

=====
Topos
=====

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card::
      :columns: 12
      :shadow: md
      :class-header: sd-bg-light sd-text-black sd-font-weight-bold

      **Topos** gives you structural code quality metrics your agents can act on.
      Pick a priority and Topos measures program structure — not just syntax — giving agents
      concrete metrics to optimize toward on every pass. You set the target; agents handle the iteration.

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card:: 🤖 For Agents
      :link: agents
      :link-type: doc

      How AI coding agents use Topos to iteratively optimize code and hit quality targets.

   .. grid-item-card:: 📐 Measures
      :link: measures
      :link-type: doc

      A breakdown of the structural and coupling metrics used to evaluate morphisms.

   .. grid-item-card:: 🚀 Installation
      :link: installation
      :link-type: doc

      Get started with the CLI, MCP server, or build from source.

   .. grid-item-card:: 🧠 Concepts
      :link: concepts
      :link-type: doc

      Optional deeper reading: the category-theoretic ideas behind the lattice and why it's structured this way.

Quick look
----------

Pick a priority, then let your agent evaluate and iterate on its own output.

.. code-block:: bash

   topos evaluate src/ -r --priority simple           # classify a directory
   topos inspect module.py                             # detailed metrics
   topos structural-test-coverage src/ --language python  # measure test code coverage
   topos compare before.py after.py                    # AST edit distance

Each file gets a verdict per quality generator — **simple** (complexity,
entropy), **composable** (dependency graph, requires GitNexus), and
**secure** (taint and dangerous-API analysis on the CPG, always
available).  You always see which generator is the problem, not a
single blended number.

How it works
------------

Topos measures code along three independent quality generators and maps them to an 8-element evaluation lattice:

- **SIMPLE** — how many decision paths run through the code (cyclomatic complexity) and how
  predictable its pattern is (entropy). Built from control flow graph.
- **COMPOSABLE** — how tightly the file is wired to the rest of the codebase (optional, requires `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_). Built from module dependency graph.
- **SECURE** — whether the code flow can reach dangerous operations or untrusted data. Built from the Code Property Graph (AST ∪ CFG ∪ DDG ∪ CDG); always available, no external tooling required.

.. mermaid::

   graph BT
       SLOP["⊥ SLOP<br/>No generators met"]
       SIMPLE["S<br/>Simple code"]
       COMPOSABLE["C<br/>Composable"]
       SECURE["Sc<br/>Secure"]
       SC["S∧C"]
       SSc["S∧Sc"]
       CSc["C∧Sc"]
       IDEAL["⊤ IDEAL<br/>All three"]

       SLOP --> SIMPLE
       SLOP --> COMPOSABLE
       SLOP --> SECURE
       SIMPLE --> SC
       SIMPLE --> SSc
       COMPOSABLE --> SC
       COMPOSABLE --> CSc
       SECURE --> SSc
       SECURE --> CSc
       SC --> IDEAL
       SSc --> IDEAL
       CSc --> IDEAL

       style SLOP       fill:#f8d7da,stroke:#842029,color:#000
       style SIMPLE     fill:#d4edda,stroke:#155724,color:#000
       style COMPOSABLE fill:#d1ecf1,stroke:#0c5460,color:#000
       style SECURE     fill:#d1f1dc,stroke:#0c5460,color:#000
       style SC         fill:#e2f5eb,stroke:#155724,color:#000
       style SSc        fill:#e2f5eb,stroke:#155724,color:#000
       style CSc        fill:#e2f5eb,stroke:#155724,color:#000
       style IDEAL      fill:#fff3cd,stroke:#856404,color:#000

.. hint::
   **Three Independent Generators:** ``SIMPLE``, ``COMPOSABLE``, and ``SECURE`` are
   **pairwise incomparable**. A file can achieve any subset of {S, C, Sc} independently.
   ``IDEAL`` is the join of all three. The 8 possible combinations form a free Heyting algebra.

.. toctree::
   :maxdepth: 1
   :caption: Documentation
   :hidden:

   installation
   For Agents <agents>
   Measures <measures>
   concepts
