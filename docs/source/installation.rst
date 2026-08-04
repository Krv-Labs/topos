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
Python runtime anywhere in the stack. Install the CLI, register it with your
agents, and you have all three pillars.

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
     - PyPI package *(secondary)*
     - ``pip install topos-mcp`` installs *only* the ``topos-mcp`` server binary (a thin wheel, zero Python runtime dependency) — not the full ``topos`` CLI, so no ``topos install``.
   * - Development
     - Source checkout *(secondary)*
     - Requires the Rust toolchain either way. Build with ``cargo`` for both binaries, or ``uv`` for a locally-built ``topos-mcp`` wheel.

Install the CLI
---------------

Two supported channels, both giving you the full ``topos`` binary — CLI and MCP
server in one. Pick whichever matches how you manage tooling; PATH order decides
which wins if you install both.

.. tab-set::

   .. tab-item:: Binary CLI
      :sync: binary

      Recommended for most users. Installs the ``topos`` executable — which
      is both the CLI and, via ``topos mcp``, the MCP server — then offers to
      install GitNexus if npm/pnpm is available.

      .. code-block:: bash

         curl -fsSL https://docs.krv.ai/topos/install.sh | bash

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
      separately so COMPOSABLE can score:

      .. code-block:: bash

         pnpm add -g gitnexus  # or: npm install -g gitnexus

      If a non-Homebrew ``topos`` is already on the machine (for example
      ``~/.local/bin/topos`` from the curl installer), ``brew install`` /
      ``brew upgrade`` prints a warning and caveats. Homebrew cannot prompt
      interactively; remove the foreign binary or fix PATH if you intend to
      use the Homebrew install.

Other install paths
-------------------

Neither of these is the recommended route. Use them when you want the MCP server
without the CLI, or you are working on Topos itself.

.. dropdown:: MCP server only — PyPI package

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
   so COMPOSABLE can score:

   .. code-block:: bash

      pnpm add -g gitnexus  # or: npm install -g gitnexus

.. dropdown:: Development — source checkout

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
   so COMPOSABLE can score:

   .. code-block:: bash

      pnpm add -g gitnexus  # or: npm install -g gitnexus

   Run the local test suite:

   .. code-block:: bash

      cargo test --workspace

Register with your agents
-------------------------

``topos install`` writes the Topos MCP server entry into every agent harness you
select — Claude Code, Claude Desktop, Codex CLI, Gemini CLI, GitHub Copilot CLI,
Cursor, VS Code, and Google Antigravity. There is no per-client ``mcp add`` step.

.. code-block:: bash

   topos install          # interactive checklist in a terminal
   topos install --all    # every supported harness, no prompts
   topos status           # what is registered, and what needs repair
   topos uninstall        # take the registrations back out

The recorded ``command`` is the absolute path of the ``topos`` you ran it with,
so GUI-launched apps can spawn it. Re-run ``topos install`` after switching
install channels or upgrading in a way that moves the binary —
``topos status`` reports that drift as ``↻ Incomplete``. Full harness table,
state model, and flags are in :doc:`agents`.

GitNexus and COMPOSABLE
-----------------------

COMPOSABLE is scored by default. ``topos evaluate`` and ``topos inspect``
resolve or refresh the repository's ``.gitnexus`` dependency graph before
scoring, so the only setup is having GitNexus on ``PATH`` — the binary installer
offers to do it for you:

.. code-block:: bash

   pnpm add -g gitnexus  # or: npm install -g gitnexus

Without it, SIMPLE and SECURE still score and COMPOSABLE reports as unavailable
rather than failing. ``topos depgraph generate`` forces a rebuild;
``--no-composable`` skips the pillar entirely.

Graphify is different: it powers the **advisory** refactor suite
(``topos graphify``, ``topos_refactor(target="graphify")``) and never affects
the medal. Install it only if you want orphan and fragile-edge detection:

.. code-block:: bash

   pip install graphifyy   # or: uvx --from graphifyy graphify --version

See :doc:`cli` and the repository's ``docs/decisions/refactor-suite.md``.

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
   * - Inspect the five weakest files
     - ``topos evaluate . -r --info``
   * - Configure project priorities
     - ``topos config``
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

      curl -fsSL https://docs.krv.ai/topos/install.sh | bash

   Homebrew installs should upgrade through Homebrew:

   .. code-block:: bash

      brew upgrade topos

   Source checkouts should use ``git pull && cargo build --release -p
   topos`` (Cargo path) or ``git pull && uv sync`` (uv path). There is no
   built-in ``topos update`` — that was a pip-specific self-update in the
   pre-migration Python CLI and doesn't carry over to a cargo/homebrew-distributed
   binary. ``topos uninstall`` exists, but it removes *agent MCP registrations*,
   not the binary.

   After an upgrade that moves the binary, re-run ``topos install`` so the
   harness entries point at the new path.

.. dropdown:: Clean uninstall

   First remove the agent registrations, which also prunes files and directories
   Topos created:

   .. code-block:: bash

      topos uninstall --all --purge-backups

   Then remove the binary itself.

   Binary installs: delete the downloaded binary (default
   ``~/.local/bin/topos``) and remove any PATH block the installer added to
   your shell profile.

   Package installs should be removed with the package manager that installed
   them, such as ``uv pip uninstall topos-mcp``.

Next steps
----------

Wire Topos into an agent with :doc:`agents`, use terminal workflows from
:doc:`cli`, or review the metric definitions in :doc:`measures`.
