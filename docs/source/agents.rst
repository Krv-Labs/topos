.. _agents:

==========
For Agents
==========

.. admonition:: The Agent Loop
   :class: philosophy-box

   Give any MCP-compatible coding agent a live feed of Topos verdicts so it can
   evaluate and iterate on its own output.
   
   Topos lets you set the quality target while the agent handles the loop:
   measure, change, verify, stop when the target or budget is reached.

Find the MCP server
-------------------

Topos is published as ``io.github.Krv-Labs/topos`` on the official MCP Registry
and mirrored on Glama. Use either listing when discovering servers from a host
UI, or run ``topos install`` (below) to register it directly.

- `Official MCP Registry <https://registry.modelcontextprotocol.io/?q=topos>`_
- `Glama MCP server page <https://glama.ai/mcp/servers/Krv-Labs/topos>`_
- `ClawHub skill <https://clawhub.ai/krv-labs/skills/topos>`_ (OpenClaw):
  ``openclaw skills install @Krv-Labs/topos``

MCP Setup
---------

You no longer register Topos agent by agent. ``topos install`` detects the
harnesses on your machine and shows them as a checklist — the ones already
configured or found on disk come pre-checked, and you pick where Topos goes.

.. code-block:: bash

   topos install

.. code-block:: text

   ┌  Which agent integrations do you want to configure?
   │
   │  ↑↓ move · space toggle · a all · enter confirm · esc cancel
   │
   │ ❯ ● Claude Code         (✓ active)
   │   ○ Claude Desktop      (not configured)
   │   ● Codex CLI           (detected)
   │   ○ Gemini CLI          (not configured)
   │   ○ GitHub Copilot CLI  (not configured)
   │   ● Cursor              (detected)
   │   ● VS Code             (▲ needs repair)
   │   ○ Google Antigravity  (not configured)
   └

Enter writes one thing per harness: the Topos MCP server entry, with an
**absolute** ``command`` path so GUI-launched apps (which inherit a minimal
``PATH``) can still spawn it. It never writes skill files, instruction blocks,
or ``@import`` lines.

.. code-block:: text

   ┌  Topos Harness Install
   │
   │  Using /opt/homebrew/bin/topos
   │
   │  Claude Code
   │    ✓ MCP server registered in ~/.claude.json (unchanged)
   │
   │  Codex CLI
   │    ✓ [mcp_servers.topos] present in ~/.codex/config.toml
   │
   │  Cursor
   │    ✓ MCP server registered in ~/.cursor/mcp.json
   │
   │  VS Code
   │    ✓ repaired — servers.topos present in the VS Code user mcp.json
   │
   └  Done. Restart any running agent for it to pick up the new server.

Skip the menu with harness ids or ``--all``, and preview with ``--dry-run``:

.. code-block:: bash

   topos install claude codex   # ids: claude, claude-desktop, codex, gemini,
                                #      copilot, cursor, vscode, antigravity
   topos install --all
   topos install --all --dry-run

Non-interactive shells (CI, scripts) must pass ids or ``--all`` — there is no
menu to fall back on, and ``topos install`` says so rather than guessing.

Check and repair
~~~~~~~~~~~~~~~~

``topos status`` — also ``topos install status``, with ``--json`` for agents —
shows every harness, worst-first, plus anything Topos found but will not touch:

.. code-block:: text

   ┌  Topos Harness Status
   │
   │  Binary: /opt/homebrew/bin/topos
   │
   │  Claude Code
   │    ✓ MCP server registered in ~/.claude.json
   │
   │  Codex CLI
   │    ✓ [mcp_servers.topos] present in ~/.codex/config.toml
   │
   │  VS Code
   │    ↻ `/usr/local/bin/topos` no longer exists — run `topos install vscode`
   │
   │  Cursor
   │    ▲ `topos` in ~/.cursor/mcp.json is an MCP entry topos did not write —
   │      inspect it by hand
   │
   │  Claude Desktop
   │    ○ no MCP server entry in the Claude Desktop config
   │
   │  Found but not managed by topos
   │    ▲ ~/.claude/skills/topos/SKILL.md — topos agent skill, installed by
   │      openclaw rather than by `topos install`
   │      remove it with `openclaw skills uninstall @Krv-Labs/topos`, or delete
   │      the `topos` skill directory by hand
   │
   └  3/8 harness integrations active.

Four states, one glyph each:

.. list-table::
   :header-rows: 1
   :widths: 20 44 36

   * - State
     - Meaning
     - What to do
   * - ``✓`` Active
     - Registered, pointing at this binary.
     - Nothing.
   * - ``↻`` Incomplete
     - Ours, but needs repair — usually the recorded path drifted after an
       upgrade or reinstall.
     - Re-run ``topos install``. Never ``topos uninstall``.
   * - ``▲`` Conflict
     - The file will not parse, or the ``topos`` key holds something Topos did
       not write.
     - Topos reports the path and writes nothing. Resolve it by hand.
   * - ``○`` Absent
     - No entry.
     - ``topos install``.

The residue block is read-only: skill files, instruction blocks, and
``@import`` lines written by other tools or by pre-0.4.4 Topos. Topos names them
and their owner, then leaves them alone.

Uninstall
~~~~~~~~~

Same checklist, then a preview and a confirm that defaults to **No**:

.. code-block:: text

   ┌  Uninstall Topos from these agents?
   │
   │  · Claude Code — remove the MCP server entry from ~/.claude.json
   │  · Codex CLI — remove [mcp_servers.topos] from ~/.codex/config.toml
   │
   │ ❯ ● No
   │   ○ Yes
   │
   │  ↑↓ · enter · esc
   └

.. code-block:: bash

   topos uninstall                        # select, preview, confirm
   topos uninstall --all --yes            # no prompts
   topos uninstall --all --dry-run        # preview only
   topos uninstall --all --purge-backups  # also delete .topos.backup files

Uninstall removes only the entries Topos wrote plus the files and directories it
created, leaves hand-made entries and other servers alone, and never replaces a
symlinked config with a regular file. It does not remove the ``topos`` binary —
see :doc:`installation` for that.

.. dropdown:: Other clients, and editor-owned alternatives

   **Any other MCP client.** Add this stdio server to its MCP settings — use an
   absolute path to ``topos`` if the client is a GUI app:

   .. code-block:: json

      { "mcpServers": { "topos": { "command": "topos", "args": ["mcp"] } } }

   **VS Code / Cursor.** ``topos install vscode`` / ``topos install cursor``
   registers your local binary and is the recommended path. Two editor-owned
   alternatives exist; pick **one** of the three, or agent mode can register two
   Topos servers and double tool calls and trust prompts.

   *MCP gallery* — Extensions view (``Ctrl+Shift+X`` / ``Cmd+Shift+X``), search
   ``@mcp topos``, install **Topos**; or use **Install MCP server** on the
   `GitHub MCP Registry page <https://github.com/mcp/Krv-Labs/topos>`_. This
   pulls the registry's PyPI package (``topos-mcp``) and VS Code owns
   registration, trust, and lifecycle — no local Topos install needed. See
   `Add MCP servers in VS Code
   <https://code.visualstudio.com/docs/copilot/customization/mcp-servers>`_.

   *Marketplace extension* — for Command Palette workflows (**Topos: Evaluate
   Project**, **Topos: Generate Dependency Graph**) and bundled runtime
   resolution, not only agent tools.

   .. button-link:: https://marketplace.visualstudio.com/items?itemName=KrvLabs.topos-vscode
      :color: primary
      :shadow:

      Topos: Code Quality Targets for Agents (``KrvLabs.topos-vscode``)

   Cursor builds without the MCP gallery should use ``topos install cursor`` or
   the Marketplace extension.

   **Antigravity.** The ``agy`` CLI exposes no documented MCP setup command;
   ``topos install antigravity`` writes ``~/.gemini/config/mcp_config.json``
   directly. If Antigravity has not migrated to that location yet,
   ``topos status`` says so.

.. dropdown:: Troubleshooting

   Use these when the server does not connect, Topos cannot see your files, or
   COMPOSABLE / ``IDEAL`` is unavailable.

   Dependency graph
      COMPOSABLE is scored by default and needs a ``.gitnexus/`` store, which
      Topos generates and refreshes on its own. SIMPLE, SECURE, AST comparison,
      MCP docs, and UAST coverage work without it.

      Prefer the MCP tools (no shell required):

      .. code-block:: text

         topos_depgraph_status({})
         topos_generate_depgraph({})

      ``topos_depgraph_status`` is read-only and reports ``missing``,
      ``present``, ``stale``, ``load_error``, ``schema_mismatch``, or
      ``invalid_dir``.
      ``topos_generate_depgraph`` shells out to GitNexus and rewrites
      ``.gitnexus/`` — approval-gated in most clients. Re-run when imports
      change, modules are renamed, or directories are restructured (it also
      no-ops safely when the graph is already current — pass ``force=true``
      to always regenerate).

      Requires GitNexus on ``PATH``:

      .. code-block:: bash

         pnpm add -g gitnexus  # or: npm install -g gitnexus

      The CLI has the same behavior: ``topos evaluate`` and ``topos inspect``
      detect and generate/refresh ``.gitnexus`` before
      scoring, accept ``--gitnexus-dir`` / ``--no-composable``, and
      ``topos depgraph generate`` forces a rebuild.

   Root override
      If the MCP host starts Topos outside the repository, set the trusted root
      explicitly:

      .. code-block:: json

         {
           "command": "topos",
           "args": ["mcp"],
           "env": { "TOPOS_MCP_FILE_ROOT": "/absolute/path/to/repo" }
         }

   Server smoke check
      Verify the binary before wiring it into editors:

      .. code-block:: bash

         topos mcp

      ``topos mcp`` waits silently on standard input. Press ``Ctrl-C`` after
      the smoke check.

   Workflow docs
      Topos exposes the workflow docs through MCP resources:

      .. code-block:: text

         topos://docs/agent-contract
         topos://docs/workflows

      Some hosts surface MCP resources directly as attachable context. Others do
      not expose resource fetching to the model, so use the equivalent tool call:

      .. code-block:: text

         topos_get_doc(topic="agent-contract")
         topos_get_doc(topic="workflows")

      Clients that expose MCP prompts can also invoke the refactor-loop prompt:

      .. code-block:: text

         topos_refactor_until_ideal(filepath="path/to/file.py")

      For a full smoke test, ask:

      .. code-block:: text

         Use topos_evaluate_project to find the worst file in src/.
         Edit it in place, then verify with topos_assess_worktree_change.
         If COMPOSABLE is blocked, call topos_depgraph_status first.

      If COMPOSABLE stays unavailable, call ``topos_depgraph_status`` or pass
      ``gitnexus_dir`` explicitly. Evaluation results include ``agent_contract``
      with ``blocked_by`` codes such as ``missing_gitnexus_dir`` or
      ``stale_gitnexus_dir`` and ``next_tool`` pointing at
      ``topos_generate_depgraph``. ``topos_evaluate_code`` can only score SIMPLE
      and SECURE because raw strings do not carry dependency-graph context.

Setting Preferences
-------------------

A **preference ranking** is a strict total order over the three quality pillars:
``simple``, ``composable``, and ``secure``. Topos uses the ranking to compute a
**relaxation walk**: the sequence of lattice targets an agent should try when
``IDEAL`` is not reachable within the available time or token budget.

Use it when you care about the order of tradeoffs. For example,
``["simple", "composable", "secure"]`` tells the agent to preserve simplicity
first, then composability, then security if all three cannot be improved at once.

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

1. **Aspirational target**: The agent first tries to reach ``IDEAL`` (all three pillars pass).
2. **Pragmatic fallback**: If progress stalls, the agent diverts to ``SIMPLE_COMPOSABLE``
   (the intersection of the top two).

MCP Tools
---------

Topos registers eighteen MCP tools, all implemented directly in ``topos-mcp``
(a compiled Rust binary — see :doc:`installation`). Every tool takes a **flat
arguments object** — the named inputs sit at the top level, with no ``params``
wrapper. Sending ``{"params": {...}}`` is rejected as an unknown field.

Most evaluation and assessment tools accept optional ``preferences`` with a
strict ``ranking`` (for example
``{"ranking": ["simple", "composable", "secure"]}``).

Structured responses may include:

``agent_contract``
   Outcome-first guidance: ``next_tool``, ``next_actions``, ``blocked_by``,
   ``verification_gates``, and ``risk_flags``. Prefer these fields over parsing
   markdown prose. Common ``blocked_by`` values include ``missing_gitnexus_dir``,
   ``stale_gitnexus_dir``, ``invalid_gitnexus_dir``, and ``parse_failures``.

``metric_locations``
   On ``topos_evaluate_file`` and ``topos_inspect_code``, maps failing
   complexity gates (``cfg.cyclomatic``, ``ast.max_function_complexity``) to
   concrete source spans with ``qualified_name``, ``kind``, line range, and
   nesting info.

``suggestions``
   Actionable fix hints for failing pillars; markdown includes a checklist when
   present.

Core Evaluation
~~~~~~~~~~~~~~~

``topos_evaluate_file({"filepath": ..., "preferences": ..., "gitnexus_dir": ..., "no_composable": ..., "refactor_targets": ..., "include_security_findings": ..., "allow": ..., "verbose": ...})``
   Classifies a file on disk. COMPOSABLE is scored by default — a missing or stale
   ``.gitnexus`` is generated/refreshed before scoring, so badges like ``IDEAL`` are
   reachable with no extra call. Pass ``gitnexus_dir`` only to point at a non-default
   graph, or ``no_composable`` to skip detection entirely. If GitNexus is not installed
   or generation fails, that is reported in ``warnings``,
   ``agent_contract.blocked_by``, and the COMPOSABLE pillar interpretation. Returns
   ``metric_locations`` for failing complexity gates.

``topos_evaluate_code({"code": ..., "language": ..., "preferences": ..., "allow": ..., "verbose": ...})``
   Classifies a raw code string (SIMPLE and SECURE only).

``topos_evaluate_project({"path": ..., "preferences": ..., "gitnexus_dir": ..., "no_composable": ..., "limit": ..., "offset": ..., "include_security_findings": ..., "allow": ..., "verbose": ...})``
   Project-wide rollup with progress reporting and pagination. Autodetects
   all six supported languages (Python, Rust, JavaScript, TypeScript, C++,
   Go) in one walk — no language argument needed. Returns worst-scoring
   files first. Use ``aggregate_floor_verdict`` for the codebase floor and
   ``worst_files`` / ``guidance`` for the next action.

``topos_inspect_code({"code": ..., "filepath": ..., "language": ..., "preferences": ..., "top_n_functions": ..., "allow": ..., "verbose": ...})``
   Detailed metric breakdown: top-N functions by complexity (with line numbers and
   ``qualified_name``), entropy details, and full metric table. Provide exactly
   one of ``code`` or ``filepath``.

Refactor & Iterate
~~~~~~~~~~~~~~~~~~

``topos_assess_worktree_change({"filepath": ..., "baseline_ref": "HEAD", "preferences": ..., "gitnexus_dir": ..., "include_security_findings": ..., "allow": ...})``
   **Default edit-in-place loop.** Compares the working-tree file to a git baseline
   (``git show <baseline_ref>:<path>``). Edit the file, then call this — no snapshot
   or pasted source required.

``topos_begin_refactor({"filepath": ..., "preferences": ..., "gitnexus_dir": ...})``
   Captures the current file as a baseline snapshot before editing. Returns a
   ``snapshot_id``. Use for untracked files or uncommitted baselines that git cannot
   serve.

``topos_assess_snapshot({"snapshot_id": ..., "filepath": ..., "include_security_findings": ..., "allow": ...})``
   Compares the current on-disk file to a snapshot from ``topos_begin_refactor``.

``topos_assess_improvement({"filepath": ..., "current_code": ..., "proposed_code": ..., "proposed_filepath": ..., "language": ..., "preferences": ..., "gitnexus_dir": ..., "include_security_findings": ..., "allow": ...})``
   Side-by-side variant assessment. Provide exactly one of ``filepath`` or
   ``current_code`` and exactly one of ``proposed_code`` or ``proposed_filepath``.

   Anti-gaming check: if scores improved but AST edit distance is near zero, it returns
   ``SUSPICIOUS_NO_STRUCTURAL_CHANGE``.

   When SECURE fails, file-level assessment includes ``security_findings`` with the
   dangerous callee, line, and source snippet — sourced from the embedded
   `Sighthound <https://github.com/Corgea/Sighthound>`_ SAST engine for
   Python/JavaScript/TypeScript/Go, falling back to local CPG probes for
   Rust/C++ or when Sighthound is disabled (``TOPOS_DISABLE_SIGHTHOUND=1``).
   These findings are supplementary detail only — the SECURE verdict itself
   always comes from the native CPG probes, never from Sighthound.

``topos_assess_changeset({"files": [...], "baseline_ref": "HEAD", "preferences": ..., "gitnexus_dir": ..., "include_security_findings": ..., "allow": ...})``
   Multi-file / module-split assessment (read-only). Each file is compared to the git
   baseline; new files have no baseline. Returns per-file verdicts, a project rollup
   (``aggregate_before`` / ``aggregate_after``), and flags
   ``complexity_relocated_within_file`` and ``project_regression``. When COMPOSABLE is
   blocked, call ``topos_generate_depgraph`` first, then re-assess.

``topos_preference_walk({"ranking": ..., "target": ..., "current": ...})``
   Returns the concrete relaxation walk (sequence of Quality Badges) the agent should
   follow to reach the target from its current state.

Dependency Graph
~~~~~~~~~~~~~~~~

``topos_depgraph_status({"gitnexus_dir": ...})``
   Read-only ``.gitnexus`` state: ``missing``, ``present``, ``stale``,
   ``load_error``, ``schema_mismatch``, or ``invalid_dir`` (a bad ``gitnexus_dir``
   override). Staleness is anchored to the commit the graph was built from
   (falling back to file mtimes for graphs built before that marker existed), so
   a regenerate reliably clears ``stale``. Never shells out.

``topos_generate_depgraph({"directory": ..., "gitnexus_dir": ..., "force": ...})``
   Runs ``gitnexus analyze`` and writes ``.gitnexus/``. When ``directory`` is
   omitted, the analyze root is derived from ``gitnexus_dir`` the same way
   ``topos_depgraph_status`` does. Side-effecting and approval-gated. Requires
   the ``gitnexus`` CLI (``pnpm add -g gitnexus  # or: npm install -g gitnexus``).

Structure & Coverage
~~~~~~~~~~~~~~~~~~~~

``topos_compare_files({"source": ..., "target": ...})``
   AST edit distance (topological drift) between two files on disk.

``topos_compare_code({"source_code": ..., "target_code": ..., "language": ...})``
   AST edit distance (topological drift) between two code strings.

``topos_calculate_coverage({"put_files": ..., "test_files": ..., "language": ..., "k": ..., "include_unknown": ..., "coverage_threshold": ...})``
   Calculates structural test coverage (UAST declaration matching and k-gram
   recall). Coverage is a separate signal; it does not change the
   SIMPLE / COMPOSABLE / SECURE lattice verdict.

Refactor Suite (advisory, not scored)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Ranked, actionable structural hotspots from four independent engines.
**None of these feed SIMPLE/COMPOSABLE/SECURE** — this is refactoring
guidance layered on top of the scored medal, distinct from the
gate-failure ``refactor_targets`` surfaced *inside* ``topos_evaluate_file``.
See the repository's ``docs/decisions/refactor-suite.md`` for the full design.

``topos_refactor({"target": "cycles"|"dependencies"|"process"|"graphify", "filepath": ..., "gitnexus_dir": ..., "graphify_dir": ..., "limit": ...})``
   One tool, four targets, each surfacing a different structural-analysis
   engine and returning a ranked list of hotspots:

   .. list-table::
      :header-rows: 1
      :widths: 18 30 12 40

      * - ``target``
        - Engine
        - Needs
        - What you get
      * - ``cycles``
        - CFG cycle basis (homology)
        - —
        - Source ranges for real loops/branches behind cyclomatic complexity
      * - ``dependencies``
        - Balanced Forman curvature on the MDG
        - GitNexus
        - Dependency edges worth strengthening (bottlenecks)
      * - ``process``
        - Directed Forman-Ricci curvature on process graphs
        - GitNexus
        - Execution choke-point transitions
      * - ``graphify``
        - Degree + confidence over a `Graphify <https://github.com/Graphify-Labs/graphify>`_ knowledge graph
        - Graphify
        - Orphan/dead-code nodes and low-confidence (``INFERRED``/``AMBIGUOUS``) edges

   ``gitnexus_dir``/``graphify_dir`` are ignored for targets that don't need
   them. ``gitnexus_available``/``tool_available`` report ``false`` (no
   error) when the backing tool/graph isn't present — the same graceful
   degradation ``topos_evaluate_file`` uses for COMPOSABLE.

``topos_generate_graphify_graph({"directory": ..., "force": ...})``
   Generates the Graphify knowledge graph (``graphify-out/graph.json``) via
   the external ``graphify`` CLI (``pip install graphifyy``). Side-effecting;
   skips running ``graphify`` when a graph is already current unless
   ``force=true``. Feeds only ``topos_refactor(target="graphify")`` — never
   SIMPLE/COMPOSABLE/SECURE.

Agent Knowledge
~~~~~~~~~~~~~~~

``topos_get_doc(topic)``
   Retrieves Topos documentation (``agent-contract``, ``workflows``, ``lattice``,
   ``metrics``, ``preferences``, or ``priority``)
   as Markdown. Use it when the client does not expose MCP resource fetching to
   the model.


MCP Prompt
----------

``topos_refactor_until_ideal(filepath, priority, max_iterations, preferences)``
   Returns a compact refactor-loop prompt with the baseline measure call,
   inspection call, improvement-assessment call, and acceptance gates. Use it
   when a client exposes MCP prompts directly.


MCP Resources
-------------

Topos exposes these Markdown resources. Clients may surface them as browsable
resources, attachable context, or direct agent context depending on host
support:

- ``topos://docs/agent-contract`` — compact outcome-first loop contract and done gates
- ``topos://docs/workflows`` — expanded review → plan → refactor → re-measure guide
- ``topos://docs/lattice`` — the 8-element Quality Badge lattice
- ``topos://docs/metrics`` — every metric key, pillar, and threshold
- ``topos://docs/priority`` — priority profiles (simple / composable / secure)
- ``topos://docs/preferences`` — strict generator rankings and preference walks
