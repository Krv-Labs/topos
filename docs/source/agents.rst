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
UI, or follow the setup tabs below to register it directly.

- `Official MCP Registry <https://registry.modelcontextprotocol.io/?q=topos>`_
- `Glama MCP server page <https://glama.ai/mcp/servers/Krv-Labs/topos>`_
- `ClawHub skill <https://clawhub.ai/krv-labs/skills/topos>`_ (OpenClaw):
  ``openclaw skills install @Krv-Labs/topos``

MCP Setup
---------

Start with the path your agent will actually use. The install path should be
short; troubleshooting belongs after the client is registered.

Choose an agent path
~~~~~~~~~~~~~~~~~~~~

.. tab-set::

   .. tab-item:: VS Code / Cursor
      :sync: vscode-extension

      VS Code exposes **two different install surfaces**. Pick **one** — do not
      install both, or agent mode can register two Topos MCP servers and double
      tool calls / trust prompts.

      **Recommended — MCP server (``@mcp`` gallery)**

      This is the official MCP Registry entry (``io.github.Krv-Labs/topos``),
      not the Marketplace extension. The registry points at the PyPI package
      ``topos-mcp``; VS Code installs/runs that package and owns registration,
      trust, and lifecycle. You do **not** need a separate local Topos install
      or the Marketplace extension.

      1. Open the Extensions view (``Ctrl+Shift+X`` / ``Cmd+Shift+X``).
      2. Search ``@mcp topos`` and install **Topos**.
      3. Or open the `GitHub MCP Registry page
         <https://github.com/mcp/Krv-Labs/topos>`_ and use **Install MCP
         server**.

      See `Add MCP servers in VS Code
      <https://code.visualstudio.com/docs/copilot/customization/mcp-servers>`_
      for gallery vs ``mcp.json`` details.

      **Optional — full Marketplace extension**

      Use this when you want Command Palette workflows (**Topos: Evaluate
      Project**, **Topos: Generate Dependency Graph**), bundled runtime
      resolution, and an editor-owned MCP provider — not only agent tools.

      .. button-link:: https://marketplace.visualstudio.com/items?itemName=KrvLabs.topos-vscode
         :color: primary
         :shadow:

         Topos: Code Quality Targets for Agents (``KrvLabs.topos-vscode``)

      The extension registers a ``topos-mcp`` server provider, resolves a
      bundled, cached, local, or downloaded Topos runtime, and starts
      ``topos mcp`` for agent mode. Topos still runs locally; the editor owns
      server registration and trust prompts.

      .. important::
         Install **either** ``@mcp topos`` **or** ``KrvLabs.topos-vscode``, not
         both. If you already have the full extension, skip the ``@mcp``
         install (and vice versa). Cursor builds that lack the MCP gallery
         should use the Marketplace extension or the Manual JSON tab.

   .. tab-item:: Agent CLIs
      :sync: agent-cli

      Run setup from the repository root you want Topos to evaluate. Prefer the
      CLI-native registration path for your agent so the host owns trust,
      lifecycle, and status checks.

      .. dropdown:: Claude Code

         Claude Code uses ``claude mcp add``. The double dash is required:
         everything after it is passed to ``topos`` unchanged.

         .. code-block:: bash

            claude mcp add --transport stdio topos -- topos mcp
            claude mcp list

         For a team-shared project config, add ``--scope project``; Claude will
         write ``.mcp.json`` and ask each user to approve it.

      .. dropdown:: Codex CLI

         Codex stores MCP servers in ``config.toml`` and shares that setup
         between the CLI and IDE extension.

         .. code-block:: bash

            codex mcp add topos -- topos mcp

         In the Codex TUI, run ``/mcp`` to confirm the server is active. For a
         project-scoped setup, put the equivalent table in ``.codex/config.toml``
         after trusting the project:

         .. code-block:: toml

            [mcp_servers.topos]
            command = "topos"
            args = ["mcp"]

      .. dropdown:: Gemini CLI

         Gemini CLI can add the stdio server directly and defaults to project
         scope.

         .. code-block:: bash

            gemini mcp add topos topos mcp
            gemini mcp list

         If Gemini reports the server as disconnected in an untrusted folder,
         trust the repository first:

         .. code-block:: bash

            gemini trust

      .. dropdown:: Antigravity CLI / ``agy``

         The current ``agy`` CLI does not expose a documented ``mcp`` setup
         command. Use one of these verified paths instead:

         - In VS Code or Cursor, use the VS Code tab above (``@mcp topos`` or
           the full Marketplace extension — not both).
         - If your Antigravity build exposes manual MCP JSON, use the
           ``Manual JSON`` tab below.
         - If you already manage MCP through Claude or Gemini plugins, import
           that host's plugin configuration with Antigravity's plugin commands,
           then verify in Antigravity before relying on Topos tools.

   .. tab-item:: Manual JSON
      :sync: manual-json

      Use this for Cursor JSON, Windsurf, or another MCP client that accepts
      a stdio server configuration.

      Add this server configuration in your client's MCP settings:

      .. code-block:: json

         { "mcpServers": { "topos": { "command": "topos", "args": ["mcp"] } } }

.. dropdown:: Troubleshooting and optional checks

   Use these only when the server does not connect, Topos cannot see your files,
   or COMPOSABLE / ``IDEAL`` is unavailable.

   Dependency graph
      COMPOSABLE and ``IDEAL`` require a ``.gitnexus/`` store. SIMPLE, SECURE,
      AST comparison, MCP docs, and UAST coverage work without it.

      Prefer the MCP tools (no shell required):

      .. code-block:: text

         topos_depgraph_status({"params": {}})
         topos_generate_depgraph({"params": {}})

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

      As of v0.4.0 there is no CLI equivalent — the ``topos`` CLI's
      ``evaluate``/``inspect`` commands don't build or read ``.gitnexus``
      yet, so COMPOSABLE is reachable through the MCP server only.

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

      ``topos mcp`` prints the FastMCP banner and waits on standard input.
      Press ``Ctrl-C`` after the smoke check.

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
(a compiled Rust binary — see :doc:`installation`). Evaluation, inspection,
assessment, coverage, depgraph, refactor, and graphify tools take a single
``params`` object. ``topos_get_doc`` takes a direct ``topic`` argument.

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

``topos_evaluate_file({"params": {"filepath": ..., "preferences": ..., "gitnexus_dir": ..., "include_security_findings": ..., "allow": ..., "verbose": ...}})``
   Classifies a file on disk. Pass ``gitnexus_dir`` to enable the COMPOSABLE pillar and
   reach higher badges like ``IDEAL``. Missing or rejected GitNexus configuration is
   reported in ``warnings``, ``agent_contract.blocked_by``, and the COMPOSABLE pillar
   interpretation. Returns ``metric_locations`` for failing complexity gates.

``topos_evaluate_code({"params": {"code": ..., "language": ..., "preferences": ..., "allow": ..., "verbose": ...}})``
   Classifies a raw code string (SIMPLE and SECURE only).

``topos_evaluate_project({"params": {"path": ..., "preferences": ..., "gitnexus_dir": ..., "limit": ..., "offset": ..., "include_security_findings": ..., "allow": ..., "verbose": ...}})``
   Project-wide rollup with progress reporting and pagination. Autodetects
   all six supported languages (Python, Rust, JavaScript, TypeScript, C++,
   Go) in one walk — no language argument needed. Returns worst-scoring
   files first. Use ``aggregate_floor_verdict`` for the codebase floor and
   ``worst_files`` / ``guidance`` for the next action.

``topos_inspect_code({"params": {"code": ..., "filepath": ..., "language": ..., "preferences": ..., "top_n_functions": ..., "allow": ..., "verbose": ...}})``
   Detailed metric breakdown: top-N functions by complexity (with line numbers and
   ``qualified_name``), entropy details, and full metric table. Provide exactly
   one of ``code`` or ``filepath``.

Refactor & Iterate
~~~~~~~~~~~~~~~~~~

``topos_assess_worktree_change({"params": {"filepath": ..., "baseline_ref": "HEAD", "preferences": ..., "gitnexus_dir": ..., "include_security_findings": ..., "allow": ...}})``
   **Default edit-in-place loop.** Compares the working-tree file to a git baseline
   (``git show <baseline_ref>:<path>``). Edit the file, then call this — no snapshot
   or pasted source required.

``topos_begin_refactor({"params": {"filepath": ..., "preferences": ..., "gitnexus_dir": ...}})``
   Captures the current file as a baseline snapshot before editing. Returns a
   ``snapshot_id``. Use for untracked files or uncommitted baselines that git cannot
   serve.

``topos_assess_snapshot({"params": {"snapshot_id": ..., "filepath": ..., "include_security_findings": ..., "allow": ...}})``
   Compares the current on-disk file to a snapshot from ``topos_begin_refactor``.

``topos_assess_improvement({"params": {"filepath": ..., "current_code": ..., "proposed_code": ..., "proposed_filepath": ..., "language": ..., "preferences": ..., "gitnexus_dir": ..., "include_security_findings": ..., "allow": ...}})``
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

``topos_assess_changeset({"params": {"files": [...], "baseline_ref": "HEAD", "preferences": ..., "gitnexus_dir": ..., "include_security_findings": ..., "allow": ...}})``
   Multi-file / module-split assessment (read-only). Each file is compared to the git
   baseline; new files have no baseline. Returns per-file verdicts, a project rollup
   (``aggregate_before`` / ``aggregate_after``), and flags
   ``complexity_relocated_within_file`` and ``project_regression``. When COMPOSABLE is
   blocked, call ``topos_generate_depgraph`` first, then re-assess.

``topos_preference_walk({"params": {"ranking": ..., "target": ..., "current": ...}})``
   Returns the concrete relaxation walk (sequence of Quality Badges) the agent should
   follow to reach the target from its current state.

Dependency Graph
~~~~~~~~~~~~~~~~

``topos_depgraph_status({"params": {"gitnexus_dir": ...}})``
   Read-only ``.gitnexus`` state: ``missing``, ``present``, ``stale``,
   ``load_error``, ``schema_mismatch``, or ``invalid_dir`` (a bad ``gitnexus_dir``
   override). Staleness is anchored to the commit the graph was built from
   (falling back to file mtimes for graphs built before that marker existed), so
   a regenerate reliably clears ``stale``. Never shells out.

``topos_generate_depgraph({"params": {"directory": ...}})``
   Runs ``gitnexus analyze`` and writes ``.gitnexus/``. Side-effecting and
   approval-gated. Requires the ``gitnexus`` CLI (``pnpm add -g gitnexus  # or: npm install -g gitnexus``).

Structure & Coverage
~~~~~~~~~~~~~~~~~~~~

``topos_compare_files({"params": {"source": ..., "target": ...}})``
   AST edit distance (topological drift) between two files on disk.

``topos_compare_code({"params": {"source_code": ..., "target_code": ..., "language": ...}})``
   AST edit distance (topological drift) between two code strings.

``topos_calculate_coverage({"params": {"put_files": ..., "test_files": ..., "language": ..., "k": ..., "include_unknown": ..., "coverage_threshold": ...}})``
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

``topos_refactor({"params": {"target": "cycles"|"dependencies"|"process"|"graphify", "filepath": ..., "gitnexus_dir": ..., "graphify_dir": ..., "limit": ...}})``
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

``topos_generate_graphify_graph({"params": {"directory": ..., "force": ...}})``
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
