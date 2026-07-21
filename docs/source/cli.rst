.. _cli:

=============
CLI Reference
=============

.. meta::
   :description: Topos command-line reference — evaluate, inspect, compare, structural test coverage, Graphify refactor hotspots, and MCP.
   :twitter:description: Topos command-line reference — evaluate, inspect, compare, structural test coverage, Graphify refactor hotspots, and MCP.

The Topos CLI is for **manual inspections** and **terminal workflows** when
you want structural quality verdicts without an editor integration. Most
agent workflows use the :doc:`MCP server <agents>` instead — it currently
covers more ground than the CLI (COMPOSABLE scoring, preference-ranked
relaxation walks, JSON output for most tools). The CLI is a fresh,
from-scratch Rust implementation built directly on ``topos-core``, not a
line-for-line port of the pre-v0.4.0 Python CLI — some Python-CLI features
haven't been ported yet; each command below says explicitly what's missing.

.. hint::
   **CLI vs MCP, as of v0.4.0:** the CLI's ``evaluate``/``inspect`` commands
   score only SIMPLE and SECURE — COMPOSABLE (which needs a GitNexus
   dependency graph) is wired up on the **MCP server only** so far. There is
   no ``--preferences``/``--priority`` relaxation walk, no ``--json``, and no
   ``--allow`` acknowledgement flag on the CLI yet. Use MCP tools
   (:doc:`agents`) for all of these.

Quick reference
---------------

.. code-block:: bash

   topos evaluate src/ -r
   topos inspect module.py
   topos compare before.py after.py
   topos coverage src/logic.py --tests tests/test_logic.py
   topos graphify generate && topos graphify orphans src/logic.py
   topos mcp

Run ``topos mcp`` as a smoke check, then stop it with ``Ctrl-C``.

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card:: 🏅 Quality commands
      :shadow: md

      Classify files, drill into metrics, measure AST drift, and score structural test overlap.
      ^^^
      ``evaluate`` · ``inspect`` · ``compare`` · ``coverage``

   .. grid-item-card:: ⚙️ Other commands
      :shadow: md

      Advisory refactor hotspots and the MCP server.
      ^^^
      ``graphify`` · ``mcp``

Quality commands
================

evaluate
--------

Evaluate code quality for one or more files or directories. This is the
primary command for **Code Quality Medals** across the three pillars (see
:doc:`measures`).

.. code-block:: bash

   topos evaluate [PATHS]... [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``-r``, ``--recursive``
     - Recursively evaluate directories.
   * - ``--language [python|rust|javascript|typescript|cpp|go]``
     - Source language for parsing and file discovery when paths are directories (default: ``python``).

**Example**

.. code-block:: bash

   topos evaluate src/ -r --language rust

Prints each file's path, resolved medal (e.g. ``Verdict: SIMPLE_SECURE``),
per-generator scores, and raw metrics. When more than one file is evaluated,
a **Directory rollup** line follows, combining each generator's verdict
across every file (the pointwise lattice meet — a pillar passes the rollup
only if every file passes it).

.. important::
   COMPOSABLE never appears in CLI output as of v0.4.0: the CLI never builds
   or reads a GitNexus dependency graph. Only SIMPLE and SECURE (plus a
   diagnostic-only PDG contribution to raw metrics) are attached. Use the
   MCP server's ``topos_evaluate_file(gitnexus_dir=...)`` for COMPOSABLE —
   see :doc:`agents`.

.. note::
   Not yet ported to this CLI (available via MCP tools instead): ``--json``,
   ``--preferences``/``--priority``, ``--gitnexus-dir``, ``--allow``, and the
   ranked "lowest-hanging fruit" digest the pre-v0.4.0 Python CLI printed for
   directory rollups.

inspect
-------

Inspect detailed metrics and entropy analysis for a **single** file.

.. code-block:: bash

   topos inspect PATH [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``--json``
     - Output the inspection as a single JSON object (a subset of the
       pre-v0.4.0 Python CLI's ``--json`` fields — no ``suggestions``/
       ``security_findings``/suppression rendering yet). Mainly intended for
       machine comparison, not primary human reading.

**Example**

.. code-block:: bash

   topos inspect src/main.py
   topos inspect src/main.py --json

.. note::
   Not yet ported: ``--priority``/``--preferences``, ``--gitnexus-dir``,
   ``--allow``.

compare
-------

Compare **structural (AST) distance** between two programs — topological drift via UAST edit distance, not line-level diff.

.. code-block:: bash

   topos compare SOURCE TARGET [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``-v``, ``--verbose``
     - Show insertions, deletions, and substitutions.

**Example**

.. code-block:: bash

   topos compare old_version.py new_version.py -v

coverage
--------

Measure how much of the **program-under-test (PUT)** structure is represented in test code.

Declaration-level bipartite matching and k-gram path recall. No test execution required. See :doc:`measures` for the underlying algorithm.

.. code-block:: bash

   topos coverage [PUT_PATHS]... --tests TEST_PATH [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``--tests PATH`` *(required, repeatable)*
     - Test file path; pass multiple times for several modules.
   * - ``--language [python|rust|javascript|typescript|cpp|go]``
     - Language for parsing (default: ``python``).
   * - ``--k INTEGER``
     - DFS kind n-gram length for path recall (default: ``3``).
   * - ``--coverage-threshold FLOAT``
     - Minimum best-match recall to count a PUT declaration as covered (default: ``0.5``).
   * - ``--include-unknown``
     - Include ``Unknown`` UAST kinds in histograms and k-grams.

**Example**

.. code-block:: bash

   topos coverage src/logic.py --tests tests/test_logic.py --k 3

.. note::
   ``--json`` is not yet ported to this CLI — plain-text output only. The
   same computation is exposed with structured JSON via the
   ``topos_calculate_coverage`` MCP tool.

Other commands
===============

graphify
--------

Generate and inspect a `Graphify <https://github.com/Graphify-Labs/graphify>`_
knowledge graph — the ``graphify`` target of Topos's advisory refactor suite.
**Purely advisory**: orphan/dead-code and fragile-edge hotspots here never
affect the SIMPLE/COMPOSABLE/SECURE medal. See ``docs/decisions/refactor-suite.md`` in
the repository for the full design, and :doc:`agents` for the equivalent MCP
tools (``topos_generate_graphify_graph``, ``topos_refactor(target="graphify")``).

.. code-block:: bash

   topos graphify generate [PATH] [OPTIONS]
   topos graphify orphans FILEPATH [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 30 20 50
   :class: topos-command-table

   * - Subcommand
     - Option
     - Description
   * - ``generate``
     - ``PATH``
     - Directory to analyze (default: current directory). Invokes the external ``graphify`` CLI as a subprocess.
   * - ``generate``
     - ``--force``
     - Regenerate even when a graph is already present.
   * - ``generate``
     - ``--json``
     - Output the result as a single JSON object.
   * - ``orphans``
     - ``FILEPATH``
     - The file to scope orphan nodes / fragile edges to (matched against each node/edge's ``source_file``).
   * - ``orphans``
     - ``--graphify-dir PATH``
     - Directory containing ``graph.json`` (default: ``./graphify-out``).
   * - ``orphans``
     - ``--limit N``
     - Maximum rows to print (default: ``5``).
   * - ``orphans``
     - ``--json``
     - Output the result as a single JSON object.

Requires `Graphify <https://github.com/Graphify-Labs/graphify>`_ on ``PATH``
(``pip install graphifyy``) for ``generate``; ``orphans`` only reads an
already-generated ``graphify-out/graph.json``.

**Example**

.. code-block:: bash

   cd /path/to/your/repo
   topos graphify generate
   topos graphify orphans src/module.py --limit 10

mcp
---

Start the Topos **Model Context Protocol** server on stdio. AI coding agents connect to this instead of shelling out to ``evaluate``.

.. code-block:: bash

   topos mcp

.. tip::
   Verify the binary before wiring it into an editor (see :doc:`agents`):

   .. code-block:: bash

      topos mcp

   The command waits on standard input. Press ``Ctrl-C`` to exit.

Next steps
----------

- :doc:`installation` — install the binary or build from source
- :doc:`agents` — wire Topos into Claude Code, Cursor, Gemini CLI, and other MCP clients
- :doc:`measures` — what each pillar measures and how thresholds map to medals
- :doc:`concepts` — lattice and characteristic-morphism background
