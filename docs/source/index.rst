.. _index:

.. rst-class:: topos-sphinx-title

=====
Topos
=====

.. raw:: html

   <div class="topos-hero">
     <p class="topos-eyebrow">Code quality evaluation</p>
     <div class="topos-hero-row">
       <img class="topos-hero-logo only-light" src="_static/topos-logo.svg" alt="Topos" />
       <img class="topos-hero-logo only-dark" src="_static/topos-logo-dark.svg" alt="" aria-hidden="true" />
     </div>
     <p class="topos-tagline">Structural code quality your agents can measure&nbsp;&mdash;&nbsp;and optimize toward.</p>
     <p class="topos-lead"><strong>Stop paying agents to rediscover and repair their own structural mess.</strong>
       Topos gives coding agents concrete quality targets before complexity, coupling, and risky data paths compound into expensive context archaeology.
       Pick a preference ranking and Topos measures program structure&nbsp;&mdash;&nbsp;not just syntax&nbsp;&mdash;&nbsp;so agents can optimize toward the code shape you want on every pass.</p>
     <div class="topos-cta-row">
       <a class="topos-btn" href="quickstart.html">Get started →</a>
       <a class="topos-btn ghost" href="https://github.com/Krv-Labs/topos">View on GitHub</a>
     </div>
   </div>

.. admonition:: Correctness is expected. Quality is the new currency.
   :class: philosophy-box

   Passing unit tests only proves that your code is a solution to a finite set of requirements. Agents have proved to be exceptional at this and will continue to improve. We believe the new currency is the quality of these solutions. Topos provides the structural evaluations that empower coding agents to find higher quality solutions.

.. grid:: 1 1 2 2
   :gutter: 3
   :class-container: topos-reveal

   .. grid-item-card:: Installation
      :link: installation
      :link-type: doc

      Get started with the CLI, MCP server, or build from source.

   .. grid-item-card:: For Agents
      :link: agents
      :link-type: doc

      MCP setup, the official registry listing, and how agents iterate toward quality targets.

   .. grid-item-card:: CLI Reference
      :link: cli
      :link-type: doc

      Detailed overview of the Topos command-line interface and available tools.

   .. grid-item-card:: Preferences
      :link: preferences
      :link-type: doc

      Tell agents how to trade off SIMPLE, COMPOSABLE, and SECURE when GOLD stalls.

   .. grid-item-card:: Measures
      :link: measures
      :link-type: doc

      A breakdown of the structural and coupling metrics used to evaluate code quality.

.. hint::
   **Built on category theory.** Topos models code quality as a structural property of programs using topos theory — the formalism is precise by design, not decoration. You don't need the math to use Topos day to day; see :doc:`concepts` for the foundations.

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
     - ``🥇 GOLD``
     - SIMPLE + COMPOSABLE + SECURE
   * - **2 of 3**
     - ``🥈 SILVER``
     - e.g. SIMPLE + SECURE, or COMPOSABLE + SECURE
   * - **1 of 3**
     - ``🥉 BRONZE``
     - e.g. SIMPLE only, or SECURE only
   * - **0 of 3**
     - ``❌ NONE``
     - Fails every pillar (or the file could not be parsed)

Manager Priorities & Agent Iteration
------------------------------------

In a perfect world, every file would earn a ``🥇 GOLD`` medal. In reality, managers and developers have a finite budget of time and tokens. 

Topos allows you to set **Preferences** — an ordering of these medals based on your immediate priorities. Coding agents use this ranking to aim for ``🥇 GOLD``. If achieving ``🥇 GOLD`` isn't feasible within the budget, the preference ranking tells the agent exactly how to *relax* its goals, ensuring it still delivers the highest possible quality medal aligned with your priorities.

Quick look
----------

Pick a preference ranking, then let your agent evaluate and iterate on its own output.

.. code-block:: bash

   topos evaluate src/ -r --preferences simple,composable,secure
   topos inspect module.py --preferences simple,composable,secure
   topos coverage src/logic.py --tests tests/test_logic.py
   topos compare before.py after.py

Each file gets a verdict per quality generator. You always see which generator is the problem, not a single blended number.

How it works
------------

Topos measures code along the three independent quality generators and maps them to an 8-element evaluation lattice:

- **SIMPLE** — Built from the `abstract syntax tree <https://en.wikipedia.org/wiki/Abstract_syntax_tree>`_ (AST) and `control-flow graph <https://en.wikipedia.org/wiki/Control-flow_graph>`_ (CFG). We calculate cyclomatic complexity of the CFG and entropy of the AST to assess complexity.
- **COMPOSABLE** — Built from the `module dependency graph <https://en.wikipedia.org/wiki/Module_dependency_graph>`_ (MDG) using `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_, to capture inter-module dependencies. This is slightly different than the usual `program dependence graph <https://en.wikipedia.org/wiki/Program_dependence_graph>`_ (PDG) which is used to capture intra-function dependencies. We calculate Martin Instability and Fanning metrics for the MDG to assess coupling.
- **SECURE** — Built from the `code property graph <https://en.wikipedia.org/wiki/Code_property_graph>`_ (CPG). We calculate dangerous-API reachability and taint paths from the CPG to assess security.

.. raw:: html

   <figure class="topos-figure">
     <img class="only-light" src="_static/figures/topos-methods.svg" alt="AST, CFG, PDG, and MDG graph lenses glued over a shared source-coordinate base, amalgamated into a single code property graph." />
     <img class="only-dark" src="_static/figures/topos-methods-dark.svg" alt="" aria-hidden="true" />
     <figcaption>Each lens reads the same source coordinates; Topos amalgamates them into one code property graph, then measures structure over the unified space.</figcaption>
   </figure>

.. raw:: html

   <figure class="topos-figure topos-figure--framed">
     <img class="only-light" src="_static/figures/topos-lattice.svg" alt="The Topos quality lattice — SLOP at the bottom, three single-pillar BRONZE states, three two-pillar SILVER states, and IDEAL (GOLD) at the top." />
     <img class="only-dark" src="_static/figures/topos-lattice-dark.svg" alt="" aria-hidden="true" />
     <figcaption>The eight-element evaluation lattice. Climbing the order means satisfying more independent quality generators; GOLD is the meet of all three.</figcaption>
   </figure>

.. hint::
   **Three Independent Pillars:** ``SIMPLE``, ``COMPOSABLE``, and ``SECURE`` are
   **pairwise incomparable**. A file can achieve any subset of {S, C, Sc} independently.
   ``🥇 GOLD`` is the intersection of all three. The **Preferences** (ranking) determine the order
   in which an agent traverses through the lattice, attempting to earn the highest possible medal.

.. toctree::
   :maxdepth: 1
   :caption: Getting Started
   :hidden:

   Quick Start <quickstart>
   installation

.. toctree::
   :maxdepth: 1
   :caption: Guides
   :hidden:

   For Agents <agents>
   Preferences <preferences>
   cli

.. toctree::
   :maxdepth: 1
   :caption: Concepts
   :hidden:

   Measures <measures>
   concepts

.. toctree::
   :maxdepth: 1
   :caption: Case Studies
   :hidden:

   Agentic Cost Savings <agent-cost-savings-case-study>

.. toctree::
   :maxdepth: 2
   :caption: Reference
   :hidden:

   API Reference <api>
