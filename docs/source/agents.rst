.. _agents:

==========
For Agents
==========

Give any MCP-compatible agent — Claude Code, Cursor, Gemini CLI, Windsurf — a live feed of
Topos verdicts so it can evaluate and iterate on its own output.

Topos lets you set and manage the quality target while your agent handles the iteration.

.. code-block:: text

   Agent iteration 1: structural: ⊥ BROKEN [41%]
     → Reduce cyclomatic complexity and normalize entropy toward 0.5

   Agent iteration 2: structural: ◐ SELF_CONTAINED [72%]
     → ✓ Target achieved.


Setting a Priority
------------------

A Priority is the quality target you set while the agent handles the iteration.
It shifts internal metric weights so each pass optimizes toward a concrete objective
rather than an open-ended target.

.. list-table::
   :widths: 20 40 40
   :header-rows: 1

   * - Priority
     - Directive
     - Optimizes toward
   * - ``self_contained``
     - *"Keep this module self-contained and dependency-light."*
     - Lower cyclomatic complexity and entropy, with minimal external dependencies
   * - ``composable``
     - *"Keep this module easy to integrate without fragile dependency chains."*
     - Clean inter-module coupling and balanced instability, with more tolerance for internal path complexity or lower compressibility when integration improves
   * - ``balanced``
     - *"Balance structure and coupling."* (default)
     - Equal weight on all metrics

Perfect code satisfies both targets, but agents operate under token and time budgets.
A concrete priority gives the agent a formula to execute instead of a vague goal.

When an agent evaluates code with a priority set, it receives:

- A **lattice element** (``BROKEN``, ``COMPOSABLE``, ``SELF_CONTAINED``, or ``SOUND``)
- A **per-dimension score** (0–100%) showing how close it is to each target
- A **guidance hint** explaining what to change to improve


MCP Setup
---------

Step 1 — Build the dependency graph
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. important::
   **Do this first.** Without a dependency graph, Topos scores the structural dimension only —
   ``COMPOSABLE`` and ``SOUND`` become unreachable.

   .. code-block:: bash

      npm install -g gitnexus        # one-time per machine (installed automatically with the CLI binary)
      cd /path/to/your/repo
      topos depgraph generate        # one-time per repo; writes .gitnexus/

   Re-run when imports change (new modules, renames, restructures).

.. tip::
   Verify the binary before wiring it into editors:

   .. code-block:: bash

      topos-mcp   # prints the FastMCP banner and waits on stdin; Ctrl-C to exit

Step 2 — Register with your agent
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Run from your project root — Topos auto-detects its file-access root by walking up for
``.git`` or ``pyproject.toml``.

**Claude Code:**

.. code-block:: bash

   claude mcp add topos topos-mcp

**Gemini CLI:**

.. code-block:: bash

   gemini mcp add topos topos-mcp

**Cursor** — `➕ Install topos in Cursor <cursor://anysphere.cursor-deeplink/mcp/install?name=topos&config=eyJjb21tYW5kIjogInRvcG9zLW1jcCJ9>`_

For Cursor (``.cursor/mcp.json``), Windsurf, and most other MCP clients, use:

.. code-block:: json

   { "mcpServers": { "topos": { "command": "topos-mcp" } } }

Step 3 — Launch from the project root
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. important::
   Topos refuses to read files outside a trusted root. If you must launch from elsewhere,
   set it explicitly:

   .. code-block:: json

      {
        "command": "topos-mcp",
        "env": { "TOPOS_MCP_FILE_ROOT": "/absolute/path/to/repo" }
      }

.. tip::
   On the agent's first turn, point it at the workflow doc:

      "Fetch ``topos://docs/workflows`` and follow the Topos refactor loop."

   Or invoke the prompt directly: ``topos_refactor_until_sound(filepath="path/to/file.py")``.

Smoke test
~~~~~~~~~~

   "Use topos to find the worst-scoring file in ``src/``, propose a refactor, and verify with ``topos_assess_improvement``."

A healthy response has ``coupling_available: true``. If every response shows
``coupling_available: false``, go back to Step 1.


MCP Tools
---------

All evaluation tools accept an optional ``priority`` parameter
(``"balanced"``, ``"composable"``, or ``"self_contained"``).

``topos_evaluate_code(code, language, priority)``
   Classifies a code string and returns the full evaluation response.

   Example response:

   .. code-block:: json

      {
        "is_parseable": true,
        "lattice_element": "SELF_CONTAINED",
        "lattice_symbol": "◐",
        "lattice_description": "Stands alone cleanly; structural quality achieved",
        "dimensions": { "structural": "SELF_CONTAINED" },
        "scores": { "structural": 72.0 },
        "priority": "self_contained",
        "guidance": "SELF_CONTAINED target achieved. Consider coupling improvements to reach SOUND.",
        "raw_metrics": { "ast.complexity": 8.0, "ast.entropy": 0.48 }
      }

``topos_evaluate_file(filepath, priority, gitnexus_dir)``
   Same as ``topos_evaluate_code`` but reads from a file path. Pass ``gitnexus_dir`` to
   enable coupling scoring and reach ``COMPOSABLE`` or ``SOUND``.

``topos_assess_improvement(proposed_code, filepath, priority)``
   Compares a proposed version against the current file. Returns ``IMPROVEMENT``,
   ``REGRESSION``, or ``LATERAL_MOVE``, plus per-dimension score deltas. Prefer
   ``filepath`` over ``current_code`` to enable coupling scoring.

   Example response:

   .. code-block:: json

      {
        "status": "IMPROVEMENT",
        "current":  { "lattice_element": "BROKEN",         "scores": { "structural": 41.0 } },
        "proposed": { "lattice_element": "SELF_CONTAINED", "scores": { "structural": 72.0 } },
        "analysis": { "score_deltas": { "structural": 31.0 }, "evaluation_improved": true }
      }

``topos_evaluate_project(path, priority, gitnexus_dir, limit, offset)``
   Project-wide rollup. Returns worst-scoring files first.

``topos_inspect_code(code, language, priority, top_n_functions)``
   Detailed metric breakdown: top-N functions by complexity, entropy details, full metric table.

``topos_compare_code(source_code, target_code, language)``
   AST edit distance (topological drift) between two code strings.

``topos_compare_files(source, target)``
   Same as ``topos_compare_code`` but reads from file paths.


MCP Resources
-------------

Read these on the agent's first turn to orient it:

- ``topos://docs/workflows`` — canonical review → plan → refactor → re-measure loop
- ``topos://docs/lattice`` — the diamond lattice (BROKEN / COMPOSABLE / SELF_CONTAINED / SOUND)
- ``topos://docs/metrics`` — every metric key and threshold
- ``topos://docs/priority`` — priority profiles (balanced / composable / self_contained)
