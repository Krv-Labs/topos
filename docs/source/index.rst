.. _index:

.. rst-class:: topos-sphinx-title

=====
Topos
=====

.. raw:: html

   <div class="topos-docs-hero">
     <h1 class="topos-docs-heading">
       <img src="_static/topos-logo.svg" alt="" aria-hidden="true" />
       <span>Topos</span>
     </h1>
   </div>

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card::
      :columns: 12
      :shadow: md
      :class-header: sd-bg-light sd-text-black sd-font-weight-bold

      **Topos** gives you structural code quality metrics your agents can act on.
      Pick a preference ranking and Topos measures program structure — not just syntax — giving agents
      concrete metrics to optimize toward on every pass. You set the target; agents handle the iteration.

.. admonition:: Philosophy: Correctness is expected. Quality is the new currency.
   :class: philosophy-box

   Passing unit tests only proves that your code is a solution to a finite set of requirements. Agents have proved to be exceptional at this and will continue to improve. We believe the new currency is the quality of these solutions. Topos provides the structural evaluations that empower coding agents to find higher quality solutions.
   `View Topos on GitHub <https://github.com/Krv-Labs/topos>`_.

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card:: 🚀 Installation
      :link: installation
      :link-type: doc

      Get started with the CLI, MCP server, or build from source.

   .. grid-item-card:: 🤖 For Agents
      :link: agents
      :link-type: doc

      How AI coding agents use Topos to iteratively optimize code and hit quality targets.

   .. grid-item-card:: 💻 CLI Reference
      :link: cli
      :link-type: doc

      Detailed overview of the Topos command-line interface and available tools.

   .. grid-item-card:: 📐 Measures
      :link: measures
      :link-type: doc

      A breakdown of the structural and coupling metrics used to evaluate code quality.

.. hint::
   **The scary maths are optional.** Topos is grounded in some very abstract fields (category & topos theory). Don't be alarmed! It's not required to understand (or appreciate) the maths to evaluate code quality with Topos. We find the formalism elegant, but know this isn't everyone's cup of tea. If you're curious about what we're building under the hood, check out :doc:`concepts`.

Beyond Correctness
-------------------

**Assume you passed the tests. How good is your solution?**

Current code evaluations focus heavily on *correctness* — does the code pass the unit tests we created? But passing tests doesn't guarantee that you've written good, secure, or maintainable code. 

Topos fills this gap by measuring structural quality, ensuring that your code isn't just correct, but built to last. It provides well-principled evaluations of a programs structure that agents can use to find better solutions.

The Medal Podium
----------------

Topos measures each file along three independent quality pillars. Each pillar is pass or fail on its own:

- **SIMPLE** — The code avoids unnecessary complexity.
- **COMPOSABLE** — The module is cleanly decoupled from other modules.
- **SECURE** — The code is free of operations that are known to expose security vulnerabilities.

Run ``topos evaluate`` or ``topos inspect`` on a file; Topos checks all three pillars and awards a **Code Quality Medal** from how many you pass. *Which* pillars you pass matters for diagnosis; the medal tier depends only on the count:

.. list-table::
   :header-rows: 1
   :widths: 18 18 44

   * - Pillars passed
     - Medal
     - Example (any combination with this count)
   * - **3 of 3**
     - 🥇 **GOLD**
     - SIMPLE + COMPOSABLE + SECURE
   * - **2 of 3**
     - 🥈 **SILVER**
     - e.g. SIMPLE + SECURE, or COMPOSABLE + SECURE
   * - **1 of 3**
     - 🥉 **BRONZE**
     - e.g. SIMPLE only, or SECURE only
   * - **0 of 3**
     - ❌ **NONE**
     - Fails every pillar (or the file could not be parsed)

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
   topos coverage src/ --language python          # measure test code coverage (scope to modules)
   topos compare before.py after.py                               # AST edit distance

Each file gets a verdict per quality generator. You always see which generator is the problem, not a single blended number.

How it works
------------

Topos measures code along the three independent quality generators and maps them to an 8-element evaluation lattice:

- **SIMPLE** — Built from the `abstract syntax tree <https://en.wikipedia.org/wiki/Abstract_syntax_tree>`_ (AST) and `control-flow graph <https://en.wikipedia.org/wiki/Control-flow_graph>`_ (CFG). We calculate cyclomatic complexity of the CFG and entropy of the AST to assess complexity.
- **COMPOSABLE** — Built from the `module dependency graph <https://en.wikipedia.org/wiki/Module_dependency_graph>`_ (MDG) using `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_, to capture inter-module dependencies. This is slightly different than the usual `program dependence graph <https://en.wikipedia.org/wiki/Program_dependence_graph>`_ (PDG) which is used to capture intra-function dependencies. We calculate Martin Instability and Fanning metrics for the MDG to assess coupling.
- **SECURE** — Built from the `code property graph <https://en.wikipedia.org/wiki/Code_property_graph>`_ (CPG). We calculate dangerous-API reachability and taint paths from the CPG to assess security.

.. mermaid::

   graph BT
       SLOP["❌ SLOP<br/>No Medal"]
       SIMPLE["🥉 BRONZE<br/>Simple"]
       COMPOSABLE["🥉 BRONZE<br/>Composable"]
       SECURE["🥉 BRONZE<br/>Secure"]
       SC["🥈 SILVER<br/>S ∧ C"]
       SSc["🥈 SILVER<br/>S ∧ Sc"]
       CSc["🥈 SILVER<br/>C ∧ Sc"]
       IDEAL["🥇 GOLD<br/>Quality Code"]

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
   cli
   For Agents <agents>
   Measures <measures>
   concepts
