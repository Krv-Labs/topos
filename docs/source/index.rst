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

   topos evaluate src/ -r --priority self_contained   # classify a directory
   topos inspect module.py                             # detailed metrics
   topos compare before.py after.py                    # AST edit distance

Each file gets a verdict per quality dimension — **structural** (complexity, entropy)
and optionally **coupling** (dependency graph). You always see which axis is the
problem, not a single blended number.

How it works
------------

Topos measures code along two orthogonal axes and maps them to a diamond evaluation lattice:

- **Structure** — how many decision paths run through the code (complexity) and how
  predictable its pattern is (entropy).
- **Coupling** — how tightly the file is wired to the rest of the codebase (optional, requires `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_).

.. mermaid::

   graph BT
       BROKEN["⊥ BROKEN<br/>Fails both targets"]
       COMPOSABLE["◑ COMPOSABLE<br/>Good coupling"]
       SELF_CONTAINED["◐ SELF_CONTAINED<br/>Good structure"]
       SOUND["⊤ SOUND<br/>Both targets met"]

       BROKEN --> COMPOSABLE
       BROKEN --> SELF_CONTAINED
       COMPOSABLE --> SOUND
       SELF_CONTAINED --> SOUND

       style BROKEN         fill:#f8d7da,stroke:#842029,color:#000
       style COMPOSABLE     fill:#d1ecf1,stroke:#0c5460,color:#000
       style SELF_CONTAINED fill:#d4edda,stroke:#155724,color:#000
       style SOUND          fill:#fff3cd,stroke:#856404,color:#000

.. hint::
   **Non-Total Order:** ``COMPOSABLE`` and ``SELF_CONTAINED`` sit
   side-by-side and are **incomparable**. A file can meet one target without meeting
   the other. ``SOUND`` is the join of both.

.. toctree::
   :maxdepth: 1
   :caption: Documentation
   :hidden:

   installation
   For Agents <agents>
   Measures <measures>
   concepts
