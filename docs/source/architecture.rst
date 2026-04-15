.. _architecture:

=======================
For AI-assisted coding
=======================

When an AI tool writes Python for you, the output can look fine but be
structurally messy — hard to change, prone to breaking, or too tightly wired
to everything else. Topos gives every piece of code a verdict so you (or the
agent itself) know exactly what to fix.

The key idea: **the lattice is your value system.** You decide what "good
code" means by where the thresholds sit. The agent gets quantitative feedback
and iterates until it meets your standard — no guessing, no vague prompts.

Setting up MCP
--------------

The fastest way to connect Topos to an AI tool is through the MCP server.
Once connected, the agent can evaluate and improve its own output in a loop
without you copying verdicts back and forth.

**1. Start the server**

.. code-block:: bash

   topos-mcp

**2. Connect it to your AI tool**

Add the server to your tool's MCP configuration. For Claude Desktop, add this
to your config file (``~/Library/Application Support/Claude/claude_desktop_config.json``
on macOS):

.. code-block:: json

   {
     "mcpServers": {
       "topos": {
         "command": "topos-mcp"
       }
     }
   }

For Claude Code, add it to ``.mcp.json`` in your project root:

.. code-block:: json

   {
     "mcpServers": {
       "topos": {
         "type": "stdio",
         "command": "topos-mcp"
       }
     }
   }

Other MCP-capable tools (Cursor, Windsurf, etc.) have similar configuration — point
them at the ``topos-mcp`` command.

**3. Restrict file access (optional)**

By default the server can read files under the working directory. To limit
access to a specific path:

.. code-block:: bash

   export TOPOS_MCP_FILE_ROOT=/path/to/workspace
   topos-mcp

How agents use Topos
--------------------

Once connected, the agent has six tools it can call:

.. list-table::
   :widths: 30 70
   :header-rows: 1

   * - Tool
     - What it does
   * - ``evaluate_code``
     - Classify a code string. Returns a verdict per dimension plus raw metrics.
   * - ``evaluate_file``
     - Same, but reads from a file path.
   * - ``assess_improvement``
     - Compare current vs. proposed code. Returns ``IMPROVEMENT``,
       ``REGRESSION``, or ``LATERAL_MOVE`` with complexity deltas.
   * - ``inspect_code``
     - Detailed breakdown: per-function complexity, entropy analysis,
       metric interpretations.
   * - ``compare_code``
     - AST edit distance between two code strings.
   * - ``compare_files``
     - AST edit distance between two files.

**The self-improvement loop**

The most useful pattern is ``evaluate`` then ``assess_improvement``:

1. The agent writes code.
2. It calls ``evaluate_code`` and sees ``COMPLEX`` with ``ast.complexity: 18.0``.
3. It rewrites, then calls ``assess_improvement`` with both versions.
4. The response says ``IMPROVEMENT`` — complexity dropped, verdict moved to ``STABLE``.
5. It can iterate again or stop.

The agent gets real numbers (complexity, entropy, similarity) alongside the
verdict, so it can target specific metrics rather than guessing what "simpler"
means.

The lattice as your value system
--------------------------------

Topos doesn't define "good code" in the abstract. The six-stage lattice is a
value system — **your** value system — that the agent follows. Each verdict
describes what the metrics observe about the code's structure:

.. list-table::
   :widths: 8 16 36 40
   :header-rows: 1

   * - Symbol
     - Verdict
     - What it observes
     - What to do
   * - ⊤
     - ``SOUND``
     - Clean, maintainable, appropriately scoped
     - Use it. This is good output.
   * - ◐
     - ``STABLE``
     - Working code; structurally sound with minor concerns
     - Fine for most uses. Consider simplifying later.
   * - ◒
     - ``COMPLEX``
     - More complex than the task warrants
     - Tell the agent: *"Simplify — too many moving parts."*
   * - ◑
     - ``COUPLED``
     - Significant coupling or structural anomaly
     - Tell the agent: *"Reduce dependencies and simplify."*
   * - ○
     - ``ENTANGLED``
     - Extreme structural or coupling pathology
     - Regenerate entirely. Give the agent the verdict and start over.
   * - ⊥
     - ``BROKEN``
     - Syntax error — cannot be evaluated
     - Paste the error back to fix.

What Topos measures
-------------------

Every evaluation checks two independent dimensions. You always see which axis
is the problem — structure, coupling, or both.

**Structure** (always on)

- *Complexity* — how many decision paths run through the code (branches, loops).
- *Entropy* — how compressible the code is. Consistent code compresses well;
  chaotic code doesn't.

**Coupling** (optional, requires GitNexus)

- *Coupling* — how many other modules depend on this one, and vice versa.
- *Instability* — whether this module absorbs a lot of change from its dependencies.

To add coupling metrics, set up `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_
(see :doc:`installation`) and pass ``--gitnexus-dir`` on the CLI.

Without MCP
-----------

If your AI tool doesn't support MCP, you can still close the loop manually.
After receiving code, save it and run:

.. code-block:: bash

   topos evaluate output.py

If the verdict is ``COMPLEX``, ``COUPLED``, or ``ENTANGLED``, paste it back:

   *"Topos rated this code as* ``[VERDICT]``\ *. Please rewrite it to be simpler and cleaner."*

For the theory behind the classification pipeline, see :doc:`concepts`.
