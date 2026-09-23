.. _cli:

=============
CLI Reference
=============

.. meta::
   :description: Topos command-line reference — evaluate, inspect, compare, structural test coverage, and MCP.
   :twitter:description: Topos command-line reference — evaluate, inspect, compare, structural test coverage, and MCP.

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
   topos mcp

Run ``topos mcp`` as a smoke check, then stop it with ``Ctrl-C``.

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card:: 🏅 Quality commands
      :shadow: md

      Classify files, drill into metrics, measure AST drift, score structural test overlap, and recap a pull request.
      ^^^
      ``evaluate`` · ``inspect`` · ``compare`` · ``coverage`` · ``pr-recap``

   .. grid-item-card:: ⚙️ Other commands
      :shadow: md

      Agent registration, project settings, and the MCP server.
      ^^^
      ``install`` · ``status`` · ``uninstall`` · ``config`` · ``depgraph`` · ``mcp``

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

pr-recap
--------

Structural before/after for a git range or a pull request, judged by the
project's PR gates: ``READY``, ``NEEDS ATTENTION``, or ``BLOCKED``, and the
one finding that decided it. Deterministic and reproducible from ``--json``;
there is no LLM in the loop.

.. code-block:: bash

   topos pr-recap
   topos pr-recap 5
   topos pr-recap --base main --head HEAD
   topos pr-recap 5 --head :worktree
   topos pr-recap 5 --verbose
   topos pr-recap 5 --info
   topos pr-recap 5 --strict
   topos pr-recap 5 --preset relaxed
   topos pr-recap 5 --json
   topos pr-recap 5 --format github
   topos pr-recap 5 --no-coupling
   topos pr-recap 5 --priority composable

.. list-table::
   :header-rows: 1
   :widths: 28 72

   * - Option
     - Description
   * - ``PR``
     - Review this pull request against the branch it merges into. Mutually
       exclusive with ``--base``/``--head``.
   * - ``--base COMMIT``
     - Git commit the change starts from.
   * - ``--head COMMIT``
     - Git commit the change ends at. Defaults to ``HEAD``. Use
       ``--head :worktree`` to include uncommitted edits and untracked files.
   * - ``--repo PATH``
     - Repository to read. Defaults to the current directory.
   * - ``--json``
     - Emit ``topos.pr_recap.v3`` instead of the review card: the readiness,
       the exit code, every finding with its gate and severity, and the
       ``gate`` settings that produced them.
   * - ``--max-files N``
     - Do not score more than this many added or modified files (default 40).
       The most-changed files (lines added plus removed) are kept; the recap
       is then marked incomplete and the rest are listed as skipped. What
       that costs the verdict is the ``incomplete`` gate's call (info by
       default).
   * - ``--verbose``
     - Add the **Changed files** table, every split child, and the
       per-function move ledger inside the card.
   * - ``--info``
     - Append the recommended change for each blocking or warning finding
       after the card, in ``inspect``'s grammar.
   * - ``--strict``
     - Fail the check on ``NEEDS ATTENTION`` too (``fail_on = "warn"``).
   * - ``--preset PRESET``
     - Gate with a built-in preset (``relaxed``, ``recommended``, ``strict``)
       for this run, ignoring the project's ``[pr_recap]``, its waivers
       included.
   * - ``--format [card|github]``
     - Which card to print. ``card`` is the default, on a terminal and in a
       pipe alike; piped output drops the color. ``github`` renders the
       Markdown sticky-comment format.
   * - ``--no-coupling``
     - Skip coupling-store preparation; COMPOSABLE is reported as not measured.
   * - ``--priority VALUE``
     - Pillar to prioritize when classifying, as for ``evaluate``. Defaults to
       the priority in ``.topos.toml``, or ``secure`` when none is configured.
       The card's meta line names the priority used.

The separate compact layout is retired: the default card already fits a CI
log. ``--compact`` and ``--format compact`` remain as hidden aliases for the
default card so existing scripts keep working.

**Range**

- The before side is the merge-base of the base and the head, like
  ``git diff base...head``: commits that landed on the base branch after the
  fork are not charged to the change.
- With no arguments, ``pr-recap`` reviews the uncommitted edits in the working
  tree (``--base HEAD --head :worktree``), including untracked files.

**Gates**

Each gate in ``[pr_recap]`` (set with ``topos config``, see `config`_) turns
what happened into a finding with a severity: ``off``, ``info``, ``warn`` or
``block``. Under the recommended preset, a lost pillar, a new file that fails
SECURE or is SLOP, and a split that gained SECURE findings block; a score drop
past ``[pr_recap.score_drop]`` (10 points, in a file with at least 20 changed
lines), a score that moved while the structure did not, and a function that
grew as a split moved it need attention; a pillar that already failed and got
worse, a new file failing another pillar, a bloated split, and skipped files
are info. ``topos config show`` lists every gate and its severity.

Moving code is not charged as a regression. When a file loses a pillar only
because code moved into it from another file in the same range (the function
arrived unchanged, the file received moved code and little new logic, or the
dependency graph traced the move), the finding is reported under the
``moved_pillar`` gate instead of ``pillar_lost``, naming the file the code came
from; ``moved_pillar`` is info under the relaxed and recommended presets and
warn under strict. A score drop the same move explains is info. A moved
function that grew on the way is still ``pillar_lost``, and a SECURE loss is
never excused when the range as a whole gained SECURE findings.

**Waivers**

A known finding can be set aside with a ``[[pr_recap.waive]]`` entry:

.. code-block:: toml

   [[pr_recap.waive]]
   gate = "pillar_lost"
   path = "src/legacy/**"
   reason = "vendored parser, replaced in #412"
   expires = "2026-12-31"

``gate`` is a gate key, ``path`` a glob over the finding's file, and
``reason`` is required; ``expires`` is optional and the waiver applies through
that date (UTC). A waived finding keeps its severity and stays in the
document, marked ``waived`` in ``--json``, but no longer counts toward the
readiness or the exit code. The card counts waived sites and unused or
expired waivers on a dim line (``1 waived · 1 unused waiver``);
``--verbose`` lists each waived site with its reason, and the GitHub
comment adds a **Waived** section. A site is one place, gate and waiver:
a function that lost two pillars under one waiver is one line naming both
(``pillar_lost SIMPLE, NAVIGABLE``), while ``--json`` keeps one finding per
pillar. An entry missing a field, naming an
unknown gate or carrying a malformed date is dropped with a warning.
Waivers are not gate settings: they do not make the preset ``custom``, and
``topos config set --pr-preset`` keeps them.

The policy comes from the flags, else the nearest ``.topos.toml``, else the
``recommended`` preset. Every card ends with a dim gate line that names it,
so a reader knows which rules produced the verdict: ``gate: recommended``,
``gate: strict``, or ``gate: custom · 2 changes · ./.topos.toml``. When the
change needs attention under a policy that fails only on blocks, the line
adds ``· warnings don't fail the check``.

**Sample card**

.. code-block:: text

   ◇  Reviewed #359  fix/ts-parser-cpg-precision → main
   │  3 files · +174/-1 · priority navigable · COMPOSABLE not measured (--no-coupling)
   │
   │  X BLOCKED   dispatch.rs lost SIMPLE and NAVIGABLE (GOLD → BRONZE)
   │
   │  PILLAR        STATUS   BEFORE   AFTER   FAILING   SCORE
   │  SIMPLE        X FAIL      62%     42%     1 / 3   ━━━━◆───── ↓
   │  SECURE        ✓ PASS     100%    100%     0 / 3   ━━━━━━━━━◆
   │  NAVIGABLE     X FAIL      98%     67%     1 / 3   ━━━━━━◆─── ↓
   │
   │  1. X topos/engine/src/graphs/ast/dispatch.rs · sanitize_typescript_type_imports:62
   │       SIMPLE 32 > 10 · NAVIGABLE 21.4 > 10 · lift the deepest nested block into a named function
   │       1 smaller dip
   │
   │  gate: recommended
   └  X 🥇 GOLD → 🥉 BRONZE · SECURE · 87% → 70% average.

   Tip: add --verbose for every file, or run topos inspect topos/engine/src/graphs/ast/dispatch.rs.

``--verbose`` adds the **Changed files** table between the findings and the
gate line:

.. code-block:: text

   │  CHANGED FILES  topos/engine/src/graphs/
   │  FILE                        MEDAL            CHANGE
   │  X ast/dispatch.rs           GOLD → BRONZE    X SIMPLE lost · X NAVIGABLE lost
   │    uast/mapper_javascript.rs GOLD             ↓ SIMPLE 69 → 64
   │  1 file kept its medal

**How to read it**

- The verdict line states the single most important finding. ``X`` marks a
  block, ``!`` a warning; info findings carry no mark and are only counted.
- The project table is ``evaluate``'s, over the existing files the change
  touched: whether each pillar passes at head, the mean score before and
  after, and how many files fail it. Added files are rolled up on their own,
  so they cannot lift or sink the average.
- The numbered list shows the blocking findings, then the warnings, most
  important first, at most ``max_hotspots`` of them (3 by default). Findings
  at the same function merge into one item. A dim line counts the rest:
  ``N more``, smaller dips, and notes. Score dips under one point are hidden.
- In **Changed files**, the mark is the file's worst finding, MEDAL shows
  ``BEFORE → AFTER`` when the tier moved, and CHANGE says what happened pillar
  by pillar: ``X SIMPLE lost``, ``✓ NAVIGABLE gained``, ``↓ SIMPLE 69 → 64``
  (a score move of at least a point that crossed no gate), ``new``, and
  ``cosmetic`` (scores moved while the syntax tree barely changed). Losing a
  pillar is a loss even when another was gained:
  ``X SIMPLE lost · ✓ NAVIGABLE gained`` is a trade and still blocks.
- The splits table shows a parent file whose code moved into new children;
  ``--verbose`` lists every child and the per-function move ledger. It
  passes (``✓``) when the split stayed lean, warns (``!``) when decisions grew
  more than 10% or a child landed SLOP, and fails (``X``) when a moved
  function came out more complex or the split carries more SECURE findings
  than the parent had. What each costs the verdict is the ``split_*`` gates'
  call.
- The footer carries the readiness mark and the project medal, so the last
  line summarizes the run on its own.

**GitHub comment**

``--format github`` renders the same document as Markdown for a sticky pull
request comment, headed ``### X Blocked · Topos structural review of #359``.
It lists **Blocking** and **Needs attention** items, collapses info findings
in ``<details>``, shows the changed files with the same MEDAL and CHANGE
columns, and puts the gate line in the footer. A hidden marker on the first
line lets a later run edit the comment instead of adding another.

**Exit codes**

- ``0`` — ``READY``, or ``NEEDS ATTENTION`` under ``fail_on = "block"`` (the
  recommended and relaxed presets).
- ``1`` — ``BLOCKED``, or ``NEEDS ATTENTION`` under ``fail_on = "warn"``
  (``--strict``, or the strict preset).
- ``2`` — ``pr-recap`` could not produce a verdict (bad range, repository not
  found, ``gh`` failure, etc.).

The ``direction`` field in ``--json`` (``IMPROVEMENT``, ``SCORE DOWN``,
``LATERAL``, ...) says which way the structure moved and never changes the
exit code.

.. note::
   Structural direction is not proof that tests or behavior still pass.

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
``copilot``, ``cursor``, ``vscode``, ``antigravity``, ``pi``, ``opencode``.

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

View or update project settings in the nearest ``.topos.toml``: the
evaluation priority and the PR gate preset ``topos pr-recap`` checks against.
Running bare ``topos config`` opens a two-step wizard on a TTY (priority,
then PR gate) that writes once at the end, and falls back to ``show`` when
input is non-interactive.

.. code-block:: bash

   topos config
   topos config show
   topos config set --priority secure
   topos config set --priority composable,secure,simple
   topos config set --pr-preset strict

``--pr-preset`` takes ``relaxed``, ``recommended`` (the default), ``strict``,
or ``custom``. A named preset is stored alone under ``[pr_recap]``, so the
project picks up improved defaults; ``custom`` writes every gate setting,
with its default and meaning in a comment, for you to edit in the file.
``config show`` lists every PR gate setting and marks the ones that differ
from the preset, then any ``[[pr_recap.waive]]`` entries.

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
