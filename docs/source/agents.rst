.. _agents:

==========
For Agents
==========

Topos is a tool for **project managers directing AI coding agents**. Code quality
is contextual — you shouldn't tune raw numeric thresholds. Instead you set a
**Priority** (a qualitative direction), and the agent evaluates its own code to
hit the corresponding **lattice target**. When the output doesn't match your
goals, you can see exactly which quality axis failed and why.

.. admonition:: The core idea

   You are the project manager. The agent is the developer. Topos translates
   your intent into a scoring target the agent can measure itself against.


Setting a Priority
------------------

A Priority is a directive you give to your agent about the structural style you
want. It shifts internal metric weights so the agent optimises toward either the
``SELF_CONTAINED`` or ``COMPOSABLE`` lattice target.

.. list-table::
   :widths: 20 40 40
   :header-rows: 1

   * - Priority
     - Directive
     - Optimises toward
   * - ``self_contained``
     - *"Write from scratch. Keep it self-contained even if it's slightly complex."*
     - Low cyclomatic complexity; entropy near 0.5; minimal external dependencies
   * - ``composable``
     - *"Lean on libraries. Keep your own code paths simple and readable."*
     - Low coupling count; balanced instability (0.3–0.7); thin glue code
   * - ``balanced``
     - *"Balance structure and coupling."* (default)
     - Equal weight on all metrics

When an agent evaluates code with a priority set, it receives:

- A **lattice element** (``BROKEN``, ``COMPOSABLE``, ``SELF_CONTAINED``, or ``SOUND``)
- A **per-dimension score** (0–100%) showing how close it is to each target
- A **guidance hint** explaining what to change to improve

If the code fails to reach the threshold on a dimension, the agent sees ``BROKEN``
for that axis and the guidance points directly at the metric to fix.


The Self-Improvement Loop
-------------------------

The standard agent workflow:

1. **Receive directive** — Project manager sets a priority and describes the task.
2. **Generate code** — Agent writes an initial implementation.
3. **Evaluate** — Agent calls Topos with the priority; receives a verdict and scores.
4. **Refactor** — If the verdict is not at the target lattice element, the agent adjusts
   based on the guidance field and re-evaluates.
5. **Compare** — Agent uses ``assess_improvement`` to confirm the new version is
   strictly better before submitting.

.. code-block:: text

   Project manager: "write a data pipeline module, priority: self_contained"

   Agent iteration 1:
     structural: ⊥ BROKEN  [41%]
     guidance: Reduce cyclomatic complexity and normalize entropy toward 0.5

   Agent iteration 2 (refactored):
     structural: ◐ SELF_CONTAINED  [72%]
     ✓ Target achieved


Using the CLI
-------------

Agents can execute CLI commands to evaluate code directly.

**Evaluate a directory with a priority:**

.. code-block:: bash

   topos evaluate src/ -r --priority self_contained
   topos evaluate src/ -r --priority composable

**Evaluate with detailed metrics:**

.. code-block:: bash

   topos evaluate src/ -r --priority balanced -v

**Inspect a single file:**

.. code-block:: bash

   topos inspect module.py

**Compare two versions to measure structural drift:**

.. code-block:: bash

   topos compare before.py after.py

**Include coupling metrics (requires GitNexus):**

.. code-block:: bash

   topos evaluate src/ -r --gitnexus-dir .gitnexus --priority composable

**JSON output (for programmatic use):**

.. code-block:: bash

   topos evaluate src/ -r --priority self_contained --json

The JSON response includes ``lattice_element``, per-dimension ``scores`` (as percentages),
and a ``priority`` field confirming which profile was used.


Using the MCP Server
--------------------

The MCP server connects Topos directly to AI tools (Claude Desktop, Cursor, Windsurf,
Claude Code, etc.) so agents can evaluate their own output without leaving the
conversation.

**Start the server:**

.. code-block:: bash

   topos-mcp

**For Claude Desktop**, add this to your config:

.. code-block:: json

   {
     "mcpServers": {
       "topos": {
         "command": "topos-mcp"
       }
     }
   }

Available MCP tools
~~~~~~~~~~~~~~~~~~~

All evaluation tools accept an optional ``priority`` parameter
(``"balanced"``, ``"composable"``, or ``"self_contained"``).

``evaluate_code(code, priority)``
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

``evaluate_file(filepath, priority)``
   Same as ``evaluate_code`` but reads from a file path.

``assess_improvement(current_code, proposed_code, priority)``
   Compares two versions. Returns ``IMPROVEMENT``, ``REGRESSION``, ``LATERAL_MOVE``,
   or ``IMPROVEMENT (Score)`` / ``REGRESSION (Score)`` for changes that move the
   continuous score without crossing a lattice threshold. Also reports per-dimension
   score deltas so the agent can detect incremental progress.

   Example response:

   .. code-block:: json

      {
        "status": "IMPROVEMENT",
        "current":  { "lattice_element": "BROKEN",         "scores": { "structural": 41.0 } },
        "proposed": { "lattice_element": "SELF_CONTAINED", "scores": { "structural": 72.0 } },
        "analysis": { "score_deltas": { "structural": 31.0 }, "evaluation_improved": true }
      }

``inspect_code(code, priority)``
   Detailed metric breakdown including per-function complexities and entropy analysis.

``compare_code(source_code, target_code)``
   Computes AST edit distance (topological drift) between two code strings.

``compare_files(source, target)``
   Same as ``compare_code`` but reads from file paths.


Writing Agent Prompts
---------------------

When directing an agent through the MCP server or CLI, include the priority in
the system prompt so every evaluation call is consistent:

.. code-block:: text

   You are writing a data-processing module. Priority: self_contained.

   After each significant change, evaluate your code with:
     evaluate_code(code=<your code>, priority="self_contained")

   Target: lattice_element == "SELF_CONTAINED" or "SOUND".
   If the verdict is BROKEN, read the guidance field and fix the indicated issue.
   Before finalizing, call assess_improvement to confirm the score has improved.

For coupling-aware evaluation (requires a dependency graph from GitNexus):

.. code-block:: text

   Priority: composable.
   Evaluate with priority="composable" and --gitnexus-dir .gitnexus.
   Target: coupling dimension == "COMPOSABLE" or "SOUND".
