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
      Pick a preference ranking and Topos measures program structure — not just syntax — giving agents
      concrete metrics to optimize toward on every pass. You set the target; agents handle the iteration.

.. admonition:: Philosophy: Quality over Correctness
   :class: philosophy-box

   Passing unit tests only proves that your code does what it says. It doesn't prove it's good code. 
   Topos provides the structural "truth values" that agents need to move beyond mere correctness
   and toward high-quality, maintainable software.

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

Beyond Correctness
------------------

**Assume you passed the tests. How good is your solution?**

Current code evaluations focus heavily on *correctness* — does the code pass the unit tests we created? But passing tests doesn't guarantee that you've written good, secure, or maintainable code. 

Topos fills this gap by measuring structural quality, ensuring that your code isn't just correct, but built to last. It provides the objective "truth values" that agents need to move beyond mere correctness and toward high-quality, maintainable software.

The Medal Podium
----------------

Topos measures code along three independent quality pillars. Think of these as generators for **Code Quality Medals**:

- **SIMPLE** — The code is readable and structurally predictable.
- **COMPOSABLE** — The module is cleanly decoupled from the rest of the system.
- **SECURE** — The data flow is safe from dangerous operations and taint.

A program can earn any combination of these medals (e.g., earning just a ``BRONZE`` medal, or a ``SILVER`` medal). The ultimate medal is ``🥇 GOLD``, where all three pillars are achieved.

Manager Priorities & Agent Iteration
------------------------------------

In a perfect world, every file would earn a ``🥇 GOLD`` medal. In reality, managers and developers have a finite budget of time and tokens. 

Topos allows you to set **Preferences** — an ordering of these medals based on your immediate priorities. Coding agents use this ranking to aim for ``🥇 GOLD``. If achieving ``🥇 GOLD`` isn't feasible within the budget, the preference ranking tells the agent exactly how to *relax* its goals, ensuring it still delivers the highest possible quality medal aligned with your priorities.

Quick look
----------

Pick a preference ranking, then let your agent evaluate and iterate on its own output.

.. code-block:: bash

   topos evaluate src/ -r --preferences simple,composable,secure  # classify a directory
   topos inspect module.py --preferences simple,composable,secure # detailed metrics
   topos structural-test-coverage src/ --language python          # measure test code coverage
   topos compare before.py after.py                               # AST edit distance

Each file gets a verdict per quality generator. You always see which generator is the problem, not a single blended number.

How it works
------------

Topos measures code along the three independent quality generators and maps them to an 8-element evaluation lattice:

- **SIMPLE** — Built from the control flow graph (cyclomatic complexity, entropy).
- **COMPOSABLE** — Built from the module dependency graph (requires `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_).
- **SECURE** — Built from the Code Property Graph (AST ∪ CFG ∪ DDG ∪ CDG); always available, no external tooling required.

.. mermaid::

   graph BT
       SLOP["❌ SLOP<br/>No Medal"]
       SIMPLE["🥉 BRONZE<br/>Simple"]
       COMPOSABLE["🥉 BRONZE<br/>Composable"]
       SECURE["🥉 BRONZE<br/>Secure"]
       SC["🥈 SILVER<br/>S ∧ C"]
       SSc["🥈 SILVER<br/>S ∧ Sc"]
       CSc["🥈 SILVER<br/>C ∧ Sc"]
       IDEAL["🥇 GOLD<br/>The Ideal Morphism"]

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
       style SIMPLE     fill:#cd7f32,stroke:#5c3a1e,color:#fff
       style COMPOSABLE fill:#cd7f32,stroke:#5c3a1e,color:#fff
       style SECURE     fill:#cd7f32,stroke:#5c3a1e,color:#fff
       style SC         fill:#c0c0c0,stroke:#4a4a4a,color:#000
       style SSc        fill:#c0c0c0,stroke:#4a4a4a,color:#000
       style CSc        fill:#c0c0c0,stroke:#4a4a4a,color:#000
       style IDEAL      fill:#ffd700,stroke:#856404,color:#000

.. hint::
   **Three Independent Pillars:** ``SIMPLE``, ``COMPOSABLE``, and ``SECURE`` are
   **pairwise incomparable**. A file can achieve any subset of {S, C, Sc} independently.
   ``🥇 GOLD`` is the intersection of all three. The **Preferences** (ranking) determine the order
   in which an agent traverses through the lattice, attempting to earn the highest possible medal.

.. toctree::
   :maxdepth: 1
   :caption: Documentation
   :hidden:

   installation
   For Agents <agents>
   Measures <measures>
   concepts
