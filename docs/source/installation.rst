.. _installation:

============
Installation
============

.. meta::
   :description: Get started with Topos. Install the CLI, MCP server, and GitNexus composability metrics.
   :twitter:description: Get started with Topos. Install the CLI, MCP server, and GitNexus composability metrics.

As of v0.4.0 (`PR #159 <https://github.com/Krv-Labs/topos/pull/159>`_) Topos
is an all-Rust `Cargo workspace <https://github.com/Krv-Labs/topos/tree/main/topos>`_
of three crates — ``topos-engine`` (the compute engine), ``topos`` (the
CLI binary), and ``topos-mcp`` (the MCP server binary). There is no
Python runtime anywhere in the stack. Install the CLI first, then add
GitNexus only when you need COMPOSABLE.

.. list-table::
   :header-rows: 1
   :widths: 22 28 50
   :class: topos-install-table

   * - Use case
     - Install path
     - What to know
   * - Most users
     - Binary CLI
     - One command installs ``topos`` and prompts to install GitNexus for COMPOSABLE metrics.
   * - Homebrew users
     - Homebrew formula
     - Installs ``topos`` from the ``krv-labs/tap`` tap. macOS arm64 and Linux amd64/arm64 only.
   * - MCP server only
     - PyPI package
     - ``pip install topos-mcp`` installs *only* the ``topos-mcp`` server binary (a thin wheel, zero Python runtime dependency) — not the full ``topos`` CLI.
   * - Development
     - Source checkout
     - Requires the Rust toolchain either way. Build with ``cargo`` for both binaries, or ``uv`` for a locally-built ``topos-mcp`` wheel.

Choose an install path
----------------------

.. tab-set::

   .. tab-item:: Binary CLI
      :sync: binary

      Recommended for most users. Installs the ``topos`` executable — which
      is both the CLI and, via ``topos mcp``, the MCP server — then offers to
      install GitNexus if npm/pnpm is available.

      .. code-block:: bash

         curl -fsSL https://docs.krv.ai/topos/install.sh | sh

      The installer:

      * downloads the latest release binary to ``~/.local/bin``;
      * verifies the release checksum;
      * warns when another ``topos`` (for example Homebrew) is already on the
        machine and suggests upgrading that channel instead;
      * adds ``~/.local/bin`` to your shell profile when needed;
      * prompts to install GitNexus through pnpm/npm for COMPOSABLE metrics.

      If GitNexus is already installed, the installer detects it and skips the
      prompt. If npm/pnpm is missing or you decline the prompt, Topos still
      works for SIMPLE, SECURE, AST comparison, structural coverage, Graphify
      refactor hotspots, and MCP tools.

      Verify the binary:

      .. code-block:: bash

         topos --version
         topos --help

      From your repo root (or ``cd /path/to/your/repo`` first):

      .. code-block:: bash

         topos evaluate . -r

      Smoke-test the MCP server:

      .. code-block:: bash

         topos mcp

      ``topos mcp`` runs the in-process Rust MCP server over stdio and waits
      on standard input. Press ``Ctrl-C`` to exit.

   .. tab-item:: Homebrew
      :sync: homebrew

      Use this when you manage tooling with Homebrew. Prefer the fully
      qualified install (Homebrew 6+: auto-taps and trusts only this formula):

      .. code-block:: bash

         brew install krv-labs/tap/topos

      Or tap first, then install. On Homebrew 6+, short-name install needs an
      explicit trust step:

      .. code-block:: bash

         brew tap krv-labs/tap
         brew trust --formula krv-labs/tap/topos
         brew install topos

      Do not set ``HOMEBREW_NO_REQUIRE_TAP_TRUST`` — that escape hatch is
      discouraged and slated for removal. See the Homebrew
      `Tap Trust <https://docs.brew.sh/Tap-Trust>`_ docs.

      Supported platforms are macOS arm64 and Linux amd64/arm64. Intel macOS
      is not supported. Upgrade through Homebrew:

      .. code-block:: bash

         brew upgrade topos

      Homebrew installs do not install GitNexus automatically. Add it
      separately when you need COMPOSABLE metrics:

      .. code-block:: bash

         pnpm add -g gitnexus  # or: npm install -g gitnexus

      If a non-Homebrew ``topos`` is already on the machine (for example
      ``~/.local/bin/topos`` from the curl installer), ``brew install`` /
      ``brew upgrade`` prints a warning and caveats. Homebrew cannot prompt
      interactively; remove the foreign binary or fix PATH if you intend to
      use the Homebrew install.

   .. tab-item:: PyPI package
      :sync: pypi

      Installs *only* the ``topos-mcp`` server binary (as the ``topos-mcp``
      command) — a thin `maturin <https://www.maturin.rs/>`_ ``bin`` wheel
      that bundles the compiled Rust binary with zero Python runtime or
      import surface. This does **not** give you the ``topos`` CLI
      (``evaluate``/``inspect``/``compare``/``coverage``/``graphify``) — use
      the binary installer or a source build for that.

      .. code-block:: bash

         uv pip install topos-mcp
         # or run without a persistent install:
         uvx topos-mcp

      PyPI installs do not install GitNexus automatically. Add it separately
      when you need COMPOSABLE metrics:

      .. code-block:: bash

         pnpm add -g gitnexus  # or: npm install -g gitnexus

   .. tab-item:: Source checkout
      :sync: source

      Use this for development, local patches, or repository integration.
      Two build paths, depending on what you need — both require the Rust
      toolchain (``cargo``); neither needs a Python runtime at *run* time.

      **Cargo — full Rust build.** Gives you both the ``topos`` CLI and the
      ``topos-mcp`` server as native binaries, straight from the workspace.

      .. code-block:: bash

         git clone https://github.com/Krv-Labs/topos.git
         cd topos
         cargo build --release -p topos        # -> target/release/topos
         cargo build --release -p topos-mcp   # -> target/release/topos-mcp

      **uv — the** ``topos-mcp`` **PyPI wheel, built locally.** Builds the
      same thin ``bin`` wheel published to PyPI — `maturin
      <https://www.maturin.rs/>`_ compiles ``topos/mcp`` under the
      hood, per ``pyproject.toml``'s ``[build-system]``. Useful for testing
      local ``topos-mcp`` changes through the exact install path end users
      get, or for producing a wheel without a full workspace build. Cargo
      still does the compiling; uv only drives the Python-side packaging.

      .. code-block:: bash

         git clone https://github.com/Krv-Labs/topos.git
         cd topos
         uv sync              # builds + installs topos-mcp into .venv
         uv run topos-mcp     # -> the compiled MCP server binary

      Or produce a distributable wheel directly:

      .. code-block:: bash

         uv build                              # -> dist/topos_mcp-*.whl
         uv pip install dist/topos_mcp-*.whl

      This path does not build the ``topos`` CLI (``evaluate``/``inspect``/
      ``compare``/``coverage``/``graphify``) — use the Cargo build above, or
      the binary installer, for that.

      Source installs do not install GitNexus automatically. Add it separately
      when you need COMPOSABLE metrics:

      .. code-block:: bash

         pnpm add -g gitnexus  # or: npm install -g gitnexus

      Run the local test suite:

      .. code-block:: bash

         cargo test --workspace

Enable optional metrics
-----------------------

.. tab-set::

   .. tab-item:: COMPOSABLE
      :sync: composable

      GitNexus builds the repository dependency graph used by the COMPOSABLE
      pillar. As of v0.4.0, wiring it up is **MCP-only** — the ``topos`` CLI's
      ``evaluate``/``inspect`` commands don't build or read ``.gitnexus`` yet
      (that CLI wiring is tracked, not yet ported; see :doc:`cli`).

      .. code-block:: bash

         pnpm add -g gitnexus  # or: npm install -g gitnexus
         claude mcp add --transport stdio topos -- topos mcp
         # then, from an agent:
         #   topos_generate_depgraph()
         #   topos_evaluate_file(filepath=..., gitnexus_dir=".gitnexus")

      Re-run ``topos_generate_depgraph`` (or use its ``force=true``) after
      imports, module names, or directory structure change; it also
      no-ops safely when the graph is already current. See :doc:`agents`.

   .. tab-item:: Graphify (advisory)
      :sync: graphify

      `Graphify <https://github.com/Graphify-Labs/graphify>`_ builds a
      tree-sitter-based knowledge graph used by the advisory refactor suite's
      ``graphify`` target (orphan/dead-code detection, fragile-edge
      flagging) — it never affects the SIMPLE/COMPOSABLE/SECURE medal.

      .. code-block:: bash

         pip install graphifyy   # or: uvx --from graphifyy graphify --version
         cd /path/to/your/repo
         topos graphify generate
         topos graphify orphans src/module.py

      See :doc:`cli` and the repository's ``docs/decisions/refactor-suite.md`` for the
      full design.

First useful commands
---------------------

.. list-table::
   :header-rows: 1
   :widths: 36 64
   :class: topos-command-table

   * - Goal
     - Command
   * - Inspect one file
     - ``topos inspect path/to/file.py``
   * - Evaluate your repo
     - ``topos evaluate . -r`` (from the repo root)
   * - Measure test structure
     - ``topos coverage src/logic.py --tests tests/test_logic.py``
   * - Advisory refactor hotspots
     - ``topos graphify orphans src/module.py``
   * - Start MCP
     - ``topos mcp``

Details and troubleshooting
---------------------------

.. dropdown:: What the binary installer does

   Set ``TOPOS_INSTALL`` to choose a different install directory or
   ``TOPOS_VERSION`` to install a specific release. Set
   ``TOPOS_NO_MODIFY_PATH=1`` to skip shell-profile edits.

   When another ``topos`` binary is already present (Homebrew, a second path,
   and so on), the installer prints channel-correct upgrade hints. If you run
   the script with an interactive stdin (for example ``sh install.sh`` in a
   terminal), it asks before continuing (default: no). Piped installs such as
   ``curl | sh`` warn and continue without blocking. Set ``TOPOS_FORCE=1`` or
   ``TOPOS_YES=1`` to skip the confirm. Prefer one install channel; PATH order
   decides which binary runs.

.. dropdown:: Upgrading

   Re-run the installer to fetch the latest release:

   .. code-block:: bash

      curl -fsSL https://docs.krv.ai/topos/install.sh | sh

   Homebrew installs should upgrade through Homebrew:

   .. code-block:: bash

      brew upgrade topos

   Source checkouts should use ``git pull && cargo build --release -p
   topos`` (Cargo path) or ``git pull && uv sync`` (uv path). There is
   no built-in ``topos update``/``topos uninstall``
   subcommand as of v0.4.0 — those were pip-specific self-update/uninstall
   commands in the pre-migration Python CLI and don't carry over to a
   cargo/homebrew-distributed binary.

.. dropdown:: Clean uninstall

   Binary installs: delete the downloaded binary (default
   ``~/.local/bin/topos``) and remove any PATH block the installer added to
   your shell profile.

   Package installs should be removed with the package manager that installed
   them, such as ``uv pip uninstall topos-mcp``.

Next steps
----------

Wire Topos into an agent with :doc:`agents`, use terminal workflows from
:doc:`cli`, or review the metric definitions in :doc:`measures`.
