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

The Badge Metaphor
------------------

Topos measures code along three independent quality pillars. Think of these as generators for **Code Quality Badges**:

- **SIMPLE** — The code is readable and structurally predictable.
- **COMPOSABLE** — The module is cleanly decoupled from the rest of the system.
- **SECURE** — The data flow is safe from dangerous operations and taint.

A program can earn any combination of these badges (e.g., earning just ``SIMPLE``, or earning ``SIMPLE_COMPOSABLE``). The ultimate badge is ``IDEAL``, where all three pillars are achieved.

Manager Priorities & Agent Iteration
------------------------------------

In a perfect world, every file would earn the ``IDEAL`` badge. In reality, managers and developers have a finite budget of time and tokens. 

Topos allows you to set **Preferences** — an ordering of these badges based on your immediate priorities. Coding agents use this ranking to aim for ``IDEAL``. If achieving ``IDEAL`` isn't feasible within the budget, the preference ranking tells the agent exactly how to *relax* its goals, ensuring it still delivers the highest possible quality badge aligned with your priorities.

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
       SLOP["⊥ SLOP<br/>No badges met"]
       SIMPLE["S<br/>Simple"]
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
   **Three Independent Pillars:** ``SIMPLE``, ``COMPOSABLE``, and ``SECURE`` are
   **pairwise incomparable**. A file can achieve any subset of {S, C, Sc} independently.
   ``IDEAL`` is the intersection of all three. The **Preferences** (ranking) determine the order
   in which an agent traverses through the lattice, attempting to earn the highest possible badge.

.. toctree::
   :maxdepth: 1
   :caption: Documentation
   :hidden:

   installation
   For Agents <agents>
   Measures <measures>
   concepts
