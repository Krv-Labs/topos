.. _installation:

============
Installation
============

.. meta::
   :description: Get started with Topos. Install the CLI, MCP server, and GitNexus composability metrics.
   :twitter:description: Get started with Topos. Install the CLI, MCP server, and GitNexus composability metrics.

Install Topos first, then add optional metrics only when you need them. The
core install includes the CLI, MCP server, SIMPLE and SECURE evaluation, AST
comparison, and UAST structural coverage.

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
   * - Managed Python environment
     - PyPI package
     - Requires Python 3.11+ and ``uv``. Install GitNexus separately when needed.
   * - Development
     - Source checkout
     - Requires Python 3.11+ and Cargo. Install optional metrics separately.

Choose an install path
----------------------

.. tab-set::

   .. tab-item:: Binary CLI
      :sync: binary

      Recommended for most users. Installs the ``topos`` executable and MCP
      server, then offers to install GitNexus if npm is available.

      .. code-block:: bash

         curl -fsSL https://docs.krv.ai/topos/install.sh | sh

      The installer:

      * downloads the latest release binary to ``~/.local/bin``;
      * verifies the release checksum;
      * warns when another ``topos`` (for example Homebrew) is already on the
        machine and suggests upgrading that channel instead;
      * records install provenance for ``topos uninstall``;
      * adds ``~/.local/bin`` to your shell profile when needed;
      * prompts to install GitNexus through npm for COMPOSABLE metrics.

      If GitNexus is already installed, the installer detects it and skips the
      prompt. If npm is missing or you decline the prompt, Topos still works for
      SIMPLE, SECURE, AST comparison, MCP tools, and UAST coverage.

      Verify the binary:

      .. code-block:: bash

         topos --version
         topos --help

      Smoke-test the MCP server:

      .. code-block:: bash

         topos mcp

      ``topos mcp`` waits on standard input. Press ``Ctrl-C`` after the FastMCP
      banner appears.

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

      Use this when you want Topos in a managed Python environment.

      .. code-block:: bash

         uv pip install topos-mcp

      PyPI installs do not install GitNexus automatically. Add it separately
      when you need COMPOSABLE metrics:

      .. code-block:: bash

         pnpm add -g gitnexus  # or: npm install -g gitnexus

   .. tab-item:: Source checkout
      :sync: source

      Use this for development, local patches, or repository integration.

      .. code-block:: bash

         git clone https://github.com/Krv-Labs/topos.git
         cd topos
         uv pip install -e .

      Source installs do not install GitNexus automatically. Add it separately
      when you need COMPOSABLE metrics:

      .. code-block:: bash

         pnpm add -g gitnexus  # or: npm install -g gitnexus

      Run the local test suites:

      .. code-block:: bash

         pytest
         cargo test

Enable optional metrics
-----------------------

.. tab-set::

   .. tab-item:: COMPOSABLE
      :sync: composable

      GitNexus builds the repository dependency graph used by the COMPOSABLE
      pillar. Install it once, generate ``.gitnexus/`` per repository, then pass
      that directory to CLI evaluations.

      .. code-block:: bash

         pnpm add -g gitnexus  # or: npm install -g gitnexus
         cd /path/to/your/repo
         topos depgraph generate
         topos evaluate src/ -r --gitnexus-dir .gitnexus/

      Re-run ``topos depgraph generate`` after imports, module names, or
      directory structure change. MCP tools auto-detect ``./.gitnexus`` from
      the project root; CLI commands expect ``--gitnexus-dir``.

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
   * - Evaluate a directory
     - ``topos evaluate src/ -r``
   * - Include COMPOSABLE
     - ``topos evaluate src/ -r --gitnexus-dir .gitnexus/``
   * - Measure test structure
     - ``topos coverage src/logic.py --tests tests/test_logic.py``
   * - Start MCP
     - ``topos mcp``

Details and troubleshooting
---------------------------

.. dropdown:: What the binary installer does

   Set ``TOPOS_INSTALL`` to choose a different install directory,
   ``TOPOS_VERSION`` to install a specific release, or
   ``TOPOS_NO_MODIFY_PATH=1`` to prevent shell-profile edits. If the installer
   adds a PATH block, it marks it with ``BEGIN TOPOS INSTALLER PATH`` /
   ``END TOPOS INSTALLER PATH`` so ``topos uninstall --prune-path-hints`` can
   remove it later.

   When another ``topos`` binary is already present (Homebrew, a second path,
   and so on), the installer prints channel-correct upgrade hints. If you run
   the script with an interactive stdin (for example ``sh install.sh`` in a
   terminal), it asks before continuing (default: no). Piped installs such as
   ``curl | sh`` warn and continue without blocking. Set ``TOPOS_FORCE=1`` or
   ``TOPOS_YES=1`` to skip the confirm. Prefer one install channel; PATH order
   decides which binary runs.

.. dropdown:: macOS Team ID error on old releases

   If ``topos --version`` fails with a PyInstaller ``different Team IDs`` error
   involving ``libpython3.12.dylib``, upgrade to v0.3.2 or later with the
   installer. Source installs are not affected.

.. dropdown:: Upgrade

   Binary and PyPI installs:

   .. code-block:: bash

      topos update

   Check for updates without upgrading (exit 0 if current, 1 if outdated):

   .. code-block:: bash

      topos update --check

   Pin a binary release:

   .. code-block:: bash

      topos update --version v0.3.6

   Homebrew installs should upgrade through Homebrew:

   .. code-block:: bash

      brew upgrade topos

   Source checkouts should use:

   .. code-block:: bash

      git pull && uv pip install -e .

   Set ``TOPOS_NO_UPDATE_NOTICES=1`` to disable passive update notices on interactive CLI use.

.. dropdown:: Clean uninstall

   Binary installs can be removed by Topos itself:

   .. code-block:: bash

      topos uninstall

   Package installs should be removed with the package manager that installed
   them, such as ``uv pip uninstall topos-mcp``.

Next steps
----------

Wire Topos into an agent with :doc:`agents`, use terminal workflows from
:doc:`cli`, or review the metric definitions in :doc:`measures`.
