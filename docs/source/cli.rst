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
covers more ground than the CLI (preference-ranked relaxation walks and
structured agent guidance). The CLI is a fresh,
from-scratch Rust implementation built directly on ``topos-engine``, not a
line-for-line port of the pre-v0.4.0 Python CLI — some Python-CLI features
haven't been ported yet; each command below says explicitly what's missing.

.. hint::
   ``evaluate`` automatically resolves GitNexus for COMPOSABLE scoring and
   supports JSON, priority, and preference inputs. MCP remains the richer
   agent surface: it returns the full preference walk, refactor targets,
   findings, and structured contracts.

Quick reference
---------------

.. code-block:: bash

   topos install
   topos status
   topos evaluate . -r
   topos config
   topos inspect module.py
   topos compare before.py after.py
   topos coverage src/logic.py --tests tests/test_logic.py
   topos depgraph generate
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

      Agent registration, project settings, advisory refactor hotspots, and the MCP server.
      ^^^
      ``install`` · ``status`` · ``uninstall`` · ``config`` · ``depgraph`` · ``graphify`` · ``mcp``

Quality commands
================

evaluate
--------

Evaluate code quality for one or more files or directories. This is the
primary command for **Code Quality Medals** across the four pillars (see
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
     - Optional discovery **filter**. Omit it and every supported language is
       discovered, each file parsed with its inferred language — the same
       multi-language default as MCP project evaluate. A named path that misses
       the filter, or does not exist, errors with the real cause.
   * - ``-v``, ``--verbose``
     - Print every file's full classification and raw metrics.
   * - ``--json``
     - Emit a machine-readable document without terminal progress. Each result
       carries its own ``language``, and COMPOSABLE problems surface in a
       top-level ``warnings`` array.
   * - ``--info``
     - Select one of the five weakest files in a TTY and show its top three
       line-level refactor targets. When piped, inspect the weakest file
       without prompting. Combine with ``--failures`` to inspect only files
       failing that pillar. Cannot be combined with ``--json``.
   * - ``--failures [simple|composable|secure]``
     - List every file whose policy gates fail the selected pillar, ordered by
       that pillar's diagnostic score. Cannot be combined with ``--json``.
   * - ``--priority PILLAR|RANKING``
     - Either a single pillar (``simple``, ``composable``, ``secure``,
       ``navigable``) as the run's primary guidance pillar, or a full
       comma-separated ranking of all four, most important first. This does
       not change fixed pass/fail gates.
   * - ``--no-composable``
     - Skip GitNexus and score SIMPLE/SECURE only.
   * - ``--gitnexus-dir PATH``
     - Use a non-default ``.gitnexus`` directory.

**Example**

.. code-block:: bash

   topos evaluate . -r                          # every supported language
   topos evaluate . -r --failures simple
   topos evaluate . -r --info
   topos evaluate . -r --failures simple --info
   topos evaluate . -r --language rust          # narrow to one language

For a directory, terminal output is a cumulative pillar table with status,
average and minimum diagnostic scores, failure counts, quality rails, and the
directory lattice floor. When a pillar fails, a short hint points to
``--failures PILLAR`` for the exact files; ``--info`` adds a bounded
``Weak spots`` list ranked by each file's average diagnostic score.
Opening a row reveals the
weakest pillar, ranked metrics, exact source spans, and recommended operations.
Combining ``--failures PILLAR --info`` applies the same browser to the five
lowest-scoring files that actually fail that pillar. A low score alone does
not put a file in the list: failure status always comes from policy gates.
Progress is drawn on stderr only while work is active.
Press ``Enter`` to open a file, ``Escape`` to return to the selector, and
``Escape`` again (or ``q``) to close it.
Single-file runs use the same compact summary without redundant aggregate
columns and point to ``topos inspect`` for the full file-level analysis.
Use ``--verbose`` only when a script or debugging session needs the legacy
inline raw-metric stream.

Representative directory output. The second line names the language when every
discovered file agrees and ``N languages`` when they do not:

.. code-block:: text

   ◇  Evaluated 20 files
   │  3 languages · priority simple · COMPOSABLE enabled
   │
   │  PILLAR        STATUS    AVG    MIN   FAILURES   SCORE
   │  SIMPLE        X FAIL    51%     0%     3 / 20    ━━━━━━━◆───────
   │  COMPOSABLE    X FAIL    60%     0%     8 / 20    ━━━━━━━━◆──────
   │  SECURE        ✓ PASS   100%   100%     0 / 20    ━━━━━━━━━━━━━━◆
   │
   │  Status reflects policy gates; scores are diagnostic — use them to guide refactoring.
   └  ✓ 🥈 SILVER · SIMPLE_SECURE · 70% average.

   Tip: add --failures simple to list its 3 failing files; --info shows overall weak spots.

When COMPOSABLE cannot be scored, the reason appears on the finished card rather
than as mid-run noise, and recoverable cases point at the fix:

.. code-block:: text

   ◇  Evaluated 20 files
   │  3 languages · priority simple · COMPOSABLE not measured
   │  ↻ GitNexus generation failed (Not inside a git repository.) — COMPOSABLE not scored

.. note::
   Pillar status comes from the raw policy gates. Normalized quality scores
   are diagnostic and therefore can be below the visual midpoint even when a
   pillar passes. ``--failures`` filters on those gates rather than scores.
   ``--info`` exposes the same ranked refactor-target evidence used by MCP
   without expanding every project row or rerunning the project.

inspect
-------

Inspect one file without losing the project context. Human output starts with
the same pillar summary as ``evaluate``, then shows ranked recommendations,
function complexity with line spans, and every raw metric. Policy metrics keep
their interpretations; supporting diagnostics remain available in a quieter
section.

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
   * - ``--no-composable``
     - Skip GitNexus and inspect SIMPLE/SECURE only.
   * - ``--gitnexus-dir PATH``
     - Use a non-default ``.gitnexus`` directory.

**Example**

.. code-block:: bash

   topos inspect src/main.py
   topos inspect src/main.py --json

The nearest ``.topos.toml`` supplies the inspection priority and preferences,
so file-level guidance stays aligned with the project. JSON field names and
values are unchanged by the human-output redesign.

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

   topos coverage SOURCE_PATHS... --tests TEST_PATH [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``--tests PATH`` *(required, repeatable)*
     - Test file or directory; repeat for multiple test paths.
   * - ``-r, --recursive``
     - Recursively discover files when source or test paths are directories.
   * - ``--language [python|rust|javascript|typescript|cpp|go]``
     - Language for parsing. Inferred when all discovered files use one language;
       required for mixed-language inputs.
   * - ``--k INTEGER``
     - DFS kind n-gram length for path recall (default: ``3``).
   * - ``--coverage-threshold FLOAT``
     - Minimum best-match recall to count a PUT declaration as covered (default: ``0.5``).
   * - ``--include-unknown``
     - Include ``Unknown`` UAST kinds in histograms and k-grams.

**Example**

.. code-block:: bash

   topos coverage src/logic.py --tests tests/test_logic.py --k 3

Directories use the same ignored-path discovery rules as ``evaluate``:

.. code-block:: bash

   topos coverage src/ --tests tests/ -r --language python

The headline reports mean declaration coverage. The following line reports
the percentage of individual source declarations meeting the configured
threshold. Topos rejects inputs with no measurable source or test declarations
instead of treating an empty corpus as covered.

.. note::
   ``--json`` is not yet ported to this CLI — plain-text output only. The
   same computation is exposed with structured JSON via the
   ``topos_calculate_coverage`` MCP tool.

Other commands
===============

install / uninstall / status
----------------------------

Register the Topos MCP server in your agent harnesses, and take it back out.
One entry per harness, with an absolute ``command`` path; no skill files, no
instruction blocks. See :doc:`agents` for the harness table and state model.

.. code-block:: bash

   topos install [HARNESSES]... [OPTIONS]
   topos uninstall [HARNESSES]... [OPTIONS]
   topos status [--json]

Harness ids: ``claude``, ``claude-desktop``, ``codex``, ``gemini``,
``copilot``, ``cursor``, ``vscode``, ``antigravity``.

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Flag
     - Behavior
   * - ``--all``
     - Target every supported harness. Required in a non-interactive shell
       when no ids are given (``install`` errors without it).
   * - ``--dry-run``
     - Print the plan and write nothing.
   * - ``-y``, ``--yes``
     - ``uninstall`` only — skip the confirmation prompt.
   * - ``--purge-backups``
     - ``uninstall`` only — also delete the ``.topos.backup`` files earlier
       installs left behind.
   * - ``--json``
     - ``status`` only — machine-readable output for agents.

With no ids in a terminal, both commands open a multi-select checklist.
``topos uninstall`` always previews what it will remove and asks first;
``topos install status`` is an alias for ``topos status``.

**Example**

.. code-block:: bash

   topos install --all --dry-run   # see what would change
   topos install claude codex      # just those two
   topos status --json             # for scripts and agents

config
------

View or update project evaluation settings in the nearest ``.topos.toml``.
Running bare ``topos config`` opens a small priority selector on a TTY and
falls back to ``show`` when input is non-interactive.

.. code-block:: bash

   topos config
   topos config show
   topos config set --priority secure
   topos config set --priority composable,secure,simple

``--priority`` accepts either form: a single pillar sets the emphasis and
reorders the existing ranking around it; a full comma-separated ranking
replaces it outright. Explicit ``evaluate`` flags override project settings.
A full ranking is the stronger statement of intent, so its first pillar
becomes the effective priority.

On disk, ``[evaluation].priority`` is a single key: a pillar string
(``priority = "secure"``) or a full ranking array
(``priority = ["composable", "secure", "simple"]``). ``config set`` always
writes the array form. A legacy ``preferences`` array is still read when
present, then dropped on the next write.

depgraph
--------

Build or refresh the GitNexus store used by COMPOSABLE scoring. Generation
no-ops when the existing graph is current unless ``--force`` is supplied.

.. code-block:: bash

   topos depgraph generate [PATH] [OPTIONS]

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``PATH``
     - Project directory to analyze (default: current directory).
   * - ``--force``
     - Regenerate even when the graph is current.
   * - ``--json``
     - Output the generation result as a single JSON object.

Requires GitNexus on ``PATH``. ``evaluate`` and ``inspect`` normally manage
the same store automatically; this command is useful for explicit refreshes
after dependency changes. If a graph reported as current appears stale, rerun
with ``--force``. When ``evaluate`` cannot measure COMPOSABLE, its terminal
summary points back to this command.

graphify
--------

Generate and inspect a `Graphify <https://github.com/Graphify-Labs/graphify>`_
knowledge graph — the ``graphify`` target of Topos's advisory refactor suite.
**Purely advisory**: orphan/dead-code and fragile-edge hotspots here never
affect the SIMPLE/COMPOSABLE/SECURE/NAVIGABLE medal. See ``docs/decisions/refactor-suite.md`` in
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
