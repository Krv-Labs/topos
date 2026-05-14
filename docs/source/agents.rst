.. _agents:

==========
For Agents
==========

Give any MCP-compatible agent — Claude Code, Cursor, Gemini CLI, Windsurf — a live feed of
Topos verdicts so it can evaluate and iterate on its own output.

Topos lets you set and manage the quality target while your agent handles the iteration.

.. code-block:: text

   Agent iteration 1: SLOP [simple: 41%, composable: -, secure: -]
     → Reduce cyclomatic complexity and normalize entropy toward 0.5

   Agent iteration 2: SIMPLE [simple: 72%, composable: -, secure: -]
     → ✓ SIMPLE target achieved.

   Agent iteration 3: SIMPLE_COMPOSABLE [simple: 72%, composable: 65%, secure: -] (with GitNexus)
     → ✓ Both SIMPLE and COMPOSABLE achieved.


Setting a Priority
------------------

A Priority is the quality target you set while the agent handles the iteration.
It shifts internal metric weights so each pass optimizes toward a concrete objective
rather than an open-ended target.

.. list-table::
   :widths: 15 35 50
   :header-rows: 1

   * - Priority
     - Directive
     - Optimizes toward
   * - ``simple``
     - *"Keep this module simple and easy to understand."*
     - Lower cyclomatic complexity, nesting depth, and entropy near 0.5. Tolerates higher coupling.
   * - ``composable``
     - *"Keep this module easy to integrate without fragile dependency chains."*
     - Clean inter-module coupling and balanced instability. Tolerates internal complexity.
   * - ``secure``
     - *"Minimize dangerous operations and taint exposure."*
     - Reduced reachable dangerous calls and taint flows. Tolerates higher complexity or coupling.
   * - ``balanced`` (default)
     - *"Balance all three generators."*
     - Equal weight on SIMPLE, COMPOSABLE, and SECURE.

Perfect code achieves all three generators, but agents operate under token and time budgets.
A concrete priority gives the agent a formula to execute instead of a vague goal.

When an agent evaluates code with a priority set, it receives:

- A **lattice element** — one of the 8 values in Ω: ``SLOP``, ``SIMPLE``,
  ``COMPOSABLE``, ``SECURE``, ``SIMPLE_COMPOSABLE``, ``SIMPLE_SECURE``,
  ``COMPOSABLE_SECURE``, or ``IDEAL``
- A **per-generator score** (0–100%) showing how close it is to each generator's threshold
- A **guidance hint** explaining what to change to improve


MCP Setup
---------

Step 1 — Build the dependency graph (optional but recommended)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. important::
   **Recommended.** Without a dependency graph, Topos scores only the SIMPLE generator —
   ``COMPOSABLE`` and ``SECURE`` become unreachable, and only ``SIMPLE`` or ``SLOP`` are possible verdicts.

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

   Or invoke the prompt directly: ``topos_refactor_until_ideal(filepath="path/to/file.py")``.

Smoke test
~~~~~~~~~~

   "Use topos to find the worst-scoring file in ``src/``, propose a refactor, and verify with ``topos_assess_improvement``."

A healthy response with GitNexus installed has ``generators: {simple: 72%, composable: 65%, secure: 45%}``.
If every response shows only ``{simple: ...}`` and no composable/secure, go back to Step 1.


MCP Tools
---------

All evaluation tools accept an optional ``priority`` parameter
(``"balanced"``, ``"simple"``, ``"composable"``, or ``"secure"``).

``topos_evaluate_code(code, language, priority)``
   Classifies a code string and returns the full evaluation response (SIMPLE generator only).

   Example response:

   .. code-block:: json

      {
        "is_parseable": true,
        "lattice_element": "SIMPLE",
        "lattice_symbol": "S",
        "lattice_description": "Simple code; structural quality achieved",
        "dimensions": { "simple": true, "composable": null, "secure": null },
        "scores": { "simple": 72.0, "composable": null, "secure": null },
        "priority": "simple",
        "guidance": "SIMPLE target achieved. Pass gitnexus_dir to evaluate COMPOSABLE and SECURE.",
        "raw_metrics": { "cfg.cyclomatic": 8.0, "ast.entropy": 0.48 }
      }

``topos_evaluate_file(filepath, priority, gitnexus_dir)``
   Same as ``topos_evaluate_code`` but reads from a file path. Pass ``gitnexus_dir`` to
   enable COMPOSABLE and SECURE generators and reach higher lattice values like ``IDEAL``.

``topos_assess_improvement(proposed_code, filepath, priority, gitnexus_dir)``
   Compares a proposed version against the current file. Returns ``IMPROVEMENT``,
   ``REGRESSION``, or ``LATERAL_MOVE``, plus per-generator score deltas. Prefer
   ``filepath`` over ``current_code`` to enable COMPOSABLE/SECURE scoring.

   Example response:

   .. code-block:: json

      {
        "status": "IMPROVEMENT",
        "current":  { "lattice_element": "SLOP",    "scores": { "simple": 41.0, "composable": null, "secure": null } },
        "proposed": { "lattice_element": "SIMPLE",  "scores": { "simple": 72.0, "composable": null, "secure": null } },
        "analysis": { "score_deltas": { "simple": 31.0 }, "evaluation_improved": true }
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

- ``topos://docs/workflows`` — canonical review → plan → refactor → re-measure loop (stop condition: ``IDEAL``)
- ``topos://docs/lattice`` — the 8-element lattice (SLOP / SIMPLE / COMPOSABLE / SECURE / SIMPLE_COMPOSABLE / SIMPLE_SECURE / COMPOSABLE_SECURE / IDEAL)
- ``topos://docs/metrics`` — every metric key, generator, and threshold
- ``topos://docs/priority`` — priority profiles (balanced / simple / composable / secure)
