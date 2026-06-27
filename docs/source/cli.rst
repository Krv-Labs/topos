.. _cli:

=============
CLI Reference
=============

.. meta::
   :description: Topos command-line reference — evaluate, inspect, compare, structural test coverage, MCP, and dependency graphs.
   :twitter:description: Topos command-line reference — evaluate, inspect, compare, structural test coverage, MCP, and dependency graphs.

The Topos CLI is for **manual inspections**, **batch rollups**, and **CI-style checks** when you want structural quality verdicts on the terminal. Most agent workflows use the :doc:`MCP server <agents>` instead; the CLI exposes the same core evaluations without an editor integration.

.. hint::
   **CLI vs MCP:** ``--preferences`` on ``evaluate`` and ``inspect`` sets display metadata from the first pillar in the list. For a full **relaxation walk** (ranked fallbacks when ``🥇 GOLD`` is out of reach), use MCP tools — see :doc:`agents`.

Quick reference
---------------

.. code-block:: bash

   topos depgraph generate
   topos evaluate src/ -r --preferences simple,composable,secure --gitnexus-dir .gitnexus/
   topos inspect module.py --preferences simple,composable,secure
   topos compare before.py after.py
   topos coverage src/logic.py --tests tests/test_logic.py
   topos mcp

Run ``topos depgraph generate`` only when you need COMPOSABLE metrics. Run
``topos mcp`` as a smoke check, then stop it with ``Ctrl-C``.

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card:: 🏅 Quality commands
      :shadow: md

      Classify files, drill into metrics, measure AST drift, and score structural test overlap.
      ^^^
      ``evaluate`` · ``inspect`` · ``compare`` · ``coverage``

   .. grid-item-card:: ⚙️ System commands
      :shadow: md

      Run the MCP server, generate dependency graphs, update, or uninstall cleanly.
      ^^^
      ``mcp`` · ``depgraph generate`` · ``update`` · ``uninstall``

Quality commands
================

evaluate
--------

Evaluate code quality for one or more files or directories. This is the primary command for **Code Quality Medals** across the three pillars (see :doc:`measures`).

.. code-block:: bash

   topos evaluate [PATHS]... [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``-r``, ``--recursive``
     - Recursively evaluate directories.
   * - ``-v``, ``--verbose``
     - Show detailed metrics for each file.
   * - ``--json``
     - Output results as a single JSON array.
   * - ``--priority [simple|composable|secure]``
     - Which pillar to emphasize in result metadata (default: ``secure``). Does not change pass/fail thresholds.
   * - ``--preferences TEXT``
     - Comma-separated pillar ranking (e.g. ``simple,composable,secure``).
   * - ``--gitnexus-dir PATH``
     - Path to a ``.gitnexus/`` directory for **COMPOSABLE** metrics (requires `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_).
   * - ``--allow TEXT``
     - Acknowledge a dangerous-call pattern for this run. Repeat or comma-separate values.
   * - ``--language [python|rust|javascript|typescript|cpp]``
     - Source language for parsing and directory discovery (default: ``python``).

**Example**

.. code-block:: bash

   topos evaluate src/ -r --preferences simple,composable,secure --gitnexus-dir .gitnexus/

Text output prints each file with its resolved medal, for example
``src/audit/_ingest.py [🥈 COMPOSABLE_SECURE]``.  Directory summaries separate
two different concepts:

- ``Directory Average Score`` is the mean file score across the evaluated files.
- ``Directory Floor Verdict`` is the aggregate floor: a pillar passes only if it
  passes across the evaluated set.

.. important::
   Without ``--gitnexus-dir``, Topos still scores **SIMPLE** and **SECURE**, but **COMPOSABLE** (and any medal requiring it) stays unreachable. Generate the graph once per repo with ``topos depgraph generate``.

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
   * - ``--priority [simple|composable|secure]``
     - Which pillar to emphasize in output (default: ``secure``).
   * - ``--preferences TEXT``
     - Comma-separated pillar ranking.
   * - ``--gitnexus-dir PATH``
     - Path to a ``.gitnexus/`` directory for coupling metrics.
   * - ``--allow TEXT``
     - Acknowledge a dangerous-call pattern for this run.
   * - ``--json``
     - Output the inspection as a single JSON object.

**Example**

.. code-block:: bash

   topos inspect src/main.py --preferences simple,composable,secure

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

**UAST (default):** declaration-level bipartite matching and k-gram path recall. No test execution required.

**Topological (ECT):** when the ``ect-coverage`` optional extra is installed,
also scores CPG semantic overlap via fastembed embeddings and Euler
characteristic transform (TRAILED). First topological run downloads the
embedding model to ``~/.cache/fastembed``.

Install the optional extra in Python package/source environments with:

.. code-block:: bash

   uv pip install "topos-mcp[ect-coverage]"

.. code-block:: bash

   topos coverage [PUT_PATHS]... --tests TEST_PATH [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``--tests PATH`` *(required, repeatable)*
     - Test file path; pass multiple times for several modules.
   * - ``--language [python|rust|javascript|typescript|cpp]``
     - Language for parsing (default: ``python``).
   * - ``--k INTEGER``
     - DFS kind n-gram length for path recall (default: ``3``).
   * - ``--coverage-threshold FLOAT``
     - Minimum best-match recall to count a PUT declaration as covered (default: ``0.5``).
   * - ``--include-unknown``
     - Include ``Unknown`` UAST kinds in histograms and k-grams.
   * - ``--json``
     - Emit a single JSON object with scores and diagnostics.

**Example**

.. code-block:: bash

   topos coverage src/logic.py --tests tests/test_logic.py --json

System commands
===============

mcp
---

Start the Topos **Model Context Protocol** server on stdio. AI coding agents connect to this instead of shelling out to ``evaluate``.

.. code-block:: bash

   topos mcp

.. tip::
   Verify the binary before wiring it into an editor (see :doc:`agents`):

   .. code-block:: bash

      topos mcp

   The command waits on standard input. Press ``Ctrl-C`` after the FastMCP
   banner appears.

depgraph generate
-----------------

Generate a module dependency graph with `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_. Required for the **COMPOSABLE** pillar and medals that depend on it.

.. code-block:: bash

   topos depgraph generate [--dir REPO_ROOT]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``--dir REPO_ROOT``
     - Repository root to analyze (default: current working directory).

Writes ``.gitnexus/`` under the repo root. Re-run when imports change.

If ``gitnexus`` is missing, install it first:

.. code-block:: bash

   npm install -g gitnexus

**GitNexus / LadybugDB compatibility**

.. list-table::
   :header-rows: 1
   :widths: 20 30 50

   * - Topos
     - Python binding
     - GitNexus CLI
   * - ≤0.3.3
     - ``real-ladybug`` 0.15 (storage v40)
     - pin ``gitnexus@<1.6`` or omit ``--gitnexus-dir``
   * - ≥0.3.4
     - ``ladybug`` 0.17+ (storage v41)
     - ``gitnexus@latest`` (tested with 1.6.7)

**Example**

.. code-block:: bash

   cd /path/to/your/repo
   topos depgraph generate
   topos evaluate src/ -r --gitnexus-dir .gitnexus/

update
------

Upgrade Topos using the detected install channel (binary installer, PyPI package manager, or source checkout).

.. code-block:: bash

   topos update [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``--check``
     - Exit 0 if up to date, 1 if outdated (for scripts). No upgrade is performed.
   * - ``--version VERSION``
     - Pin a release for binary installs (maps to ``TOPOS_VERSION`` in ``install.sh``).

Channel behavior:

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Install channel
     - ``topos update`` behavior
   * - Binary (``install.sh``)
     - Re-runs the installer with checksum verification; preserves PATH hints.
   * - PyPI (``uv`` / ``pip``)
     - Runs ``uv pip install -U topos-mcp`` or ``pip install -U topos-mcp``.
   * - Source (editable checkout)
     - Prints instructions: ``git pull && uv pip install -e .``
   * - Unknown
     - Prints all supported upgrade paths.

On interactive CLI use (not ``topos mcp``), Topos may print a one-line update notice at most once per 24 hours when a newer release is available. Set ``TOPOS_NO_UPDATE_NOTICES=1`` to disable.

uninstall
---------

Safely remove Topos based on how it was installed (binary installer vs package manager).

.. code-block:: bash

   topos uninstall [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``--dry-run``
     - Show what would be removed without changing anything.
   * - ``--yes``
     - Skip confirmation prompts.
   * - ``--prune-path-hints``
     - Remove PATH hint blocks previously added by the installer.

Next steps
----------

- :doc:`installation` — install the binary or build from source
- :doc:`agents` — wire Topos into Claude Code, Cursor, Gemini CLI, and other MCP clients
- :doc:`measures` — what each pillar measures and how thresholds map to medals
- :doc:`concepts` — lattice and characteristic-morphism background
