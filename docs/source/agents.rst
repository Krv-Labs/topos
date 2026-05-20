.. _agents:

==========
For Agents
==========

.. admonition:: The Agent Loop
   :class: philosophy-box

   Give any MCP-compatible agent — Claude Code, Cursor, Gemini CLI, Windsurf — a live feed of
   Topos verdicts so it can evaluate and iterate on its own output.
   
   Topos lets you set and manage the quality target while your agent handles the iteration.


Setting Preferences
-------------------

A **Preference Ranking** is the quality target you set while the agent handles the iteration.
It is a strict total order (permutation) of the three quality pillars. Topos uses this
ranking to guide the agent along a **relaxation walk** — a sequence of achievable 
**Quality Badges** that move toward your ideal target.

This allows a **Manager** to set priorities based on a finite budget of time and tokens,
while the **Agent** works autonomously to earn the highest possible badge within those constraints.

.. list-table::
   :widths: 15 35 50
   :header-rows: 1

   * - Rank
     - Primary Focus
     - Optimizes toward
   * - 1 (Top)
     - Mandatory
     - The property that must be achieved first.
   * - 2 (Middle)
     - Aspirational
     - The secondary goal; forms the "ideal intersection" with Rank 1.
   * - 3 (Bottom)
     - Pragmatic
     - The final property needed to reach ``IDEAL``.

Example Ranking: ``(SIMPLE, COMPOSABLE, SECURE)``

1. **Aspirational Target**: The agent first tries to reach ``IDEAL`` (all three badges achieved).
2. **Pragmatic Fallback**: If progress stalls, the agent diverts to ``SIMPLE_COMPOSABLE`` 
   (the intersection of the top two).

MCP Setup
---------

Step 1 — Build the dependency graph (optional but recommended)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. important::
   **Recommended.** Without a dependency graph, Topos cannot score the COMPOSABLE pillar —
   any badge requiring it (including ``IDEAL``) becomes unreachable.

   .. code-block:: bash

      npm install -g gitnexus        # one-time per machine (installed automatically with the CLI binary)
      cd /path/to/your/repo
      topos depgraph generate        # one-time per repo; writes .gitnexus/

   Re-run when imports change (new modules, renames, restructures).

.. tip::
   Verify the binary before wiring it into editors:

   .. code-block:: bash

      topos mcp   # prints the FastMCP banner and waits on stdin; Ctrl-C to exit

Step 2 — Register with your agent
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Run from your project root — Topos auto-detects its file-access root by walking up for
``.git`` or ``pyproject.toml``.

.. dropdown:: Claude Code

   .. code-block:: bash

      claude mcp add topos topos mcp

.. dropdown:: Gemini CLI

   .. code-block:: bash

      gemini mcp add topos topos mcp

.. dropdown:: Cursor

   `➕ Install topos in Cursor <cursor://anysphere.cursor-deeplink/mcp/install?name=topos&config=eyJjb21tYW5kIjogInRvcG9zIG1jcCJ9>`_

   Or edit ``.cursor/mcp.json``:

   .. code-block:: json

      { "mcpServers": { "topos": { "command": "topos mcp" } } }

.. dropdown:: Windsurf and others

   For most other MCP clients, use:

   .. code-block:: json

      { "mcpServers": { "topos": { "command": "topos mcp" } } }

Step 3 — Launch from the project root
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. important::
   Topos refuses to read files outside a trusted root. If you must launch from elsewhere,
   set it explicitly:

   .. code-block:: json

      {
        "command": "topos mcp",
        "env": { "TOPOS_MCP_FILE_ROOT": "/absolute/path/to/repo" }
      }

.. tip::
   On the agent's first turn, point it at the workflow doc:

      "Fetch ``topos://docs/workflows`` and follow the Topos refactor loop."

   Or invoke the prompt directly: ``topos_refactor_until_ideal(filepath="path/to/file.py")``.

Smoke test
~~~~~~~~~~

   "Use topos to find the worst-scoring file in ``src/``, propose a refactor, and verify with ``topos_assess_improvement``."

A healthy response with GitNexus installed has ``generators: {simple: 72%, composable: 65%, secure: 45%}``.
If every response shows only ``{simple: ...}`` and no composable/secure, go back to Step 1.


MCP Tools
---------

All evaluation tools accept an optional ``preferences`` parameter which includes a ``ranking`` 
(e.g., ``["simple", "composable", "secure"]``).

Core Evaluation
~~~~~~~~~~~~~~~

``topos_evaluate_file(filepath, preferences, gitnexus_dir, include_security_findings)``
   Classifies a file on disk. Pass ``gitnexus_dir`` to enable the COMPOSABLE pillar and 
   reach higher badges like ``IDEAL``. Missing or rejected GitNexus configuration is
   reported in ``warnings`` and in the COMPOSABLE pillar interpretation.

``topos_evaluate_code(code, language, preferences)``
   Classifies a raw code string (SIMPLE and SECURE only).

``topos_evaluate_project(path, preferences, gitnexus_dir, limit, offset, include_security_findings)``
   Project-wide rollup. Returns worst-scoring files first. Use
   ``aggregate_floor_verdict`` for the codebase floor and ``worst_files`` /
   ``guidance`` for the next action.

``topos_inspect_code(code, filepath, language, preferences, top_n_functions)``
   Detailed metric breakdown: top-N functions by complexity and line number,
   entropy details, and full metric table. Provide exactly one of ``code`` or
   ``filepath``.

Refactor & Iterate
~~~~~~~~~~~~~~~~~~

``topos_assess_improvement(proposed_code, proposed_filepath, filepath, preferences, gitnexus_dir)``
   Compares a proposed version against the current file. Returns an ``IMPROVEMENT`` status
   if the quality badge or scores have improved according to the preferences. Provide
   exactly one of ``proposed_code`` or ``proposed_filepath``.

   Anti-gaming check: if scores improved but AST edit distance is near zero, it returns 
   ``SUSPICIOUS_NO_STRUCTURAL_CHANGE``.

   When SECURE fails, file-level assessment includes ``security_findings`` with the
   dangerous callee, line, and source snippet.

``topos_preference_walk(ranking, target, current)``
   Returns the concrete relaxation walk (sequence of Quality Badges) the agent should
   follow to reach the target from its current state.

Structure & Coverage
~~~~~~~~~~~~~~~~~~~~

``topos_compare_files(source, target)``
   AST edit distance (topological drift) between two files on disk.

``topos_compare_code(source_code, target_code, language)``
   AST edit distance (topological drift) between two code strings.

``topos_calculate_coverage(put_files, test_files, k)``
   Calculates structural test coverage using UAST k-gram path recall.

Agent Knowledge
~~~~~~~~~~~~~~~

``topos_get_doc(topic)``
   Retrieves Topos documentation (``workflows``, ``lattice``, ``metrics``, or ``priority``) 
   as Markdown. Useful for agents in environments where MCP resources are not directly accessible.


MCP Resources
-------------

Read these on the agent's first turn to orient it:

- ``topos://docs/workflows`` — canonical review → plan → refactor → re-measure loop (stop condition: ``IDEAL``)
- ``topos://docs/lattice`` — the 8-element Quality Badge lattice
- ``topos://docs/metrics`` — every metric key, pillar, and threshold
- ``topos://docs/priority`` — priority profiles (simple / composable / secure)
