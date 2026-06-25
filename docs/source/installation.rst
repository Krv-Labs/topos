.. _installation:

============
Installation
============

.. meta::
   :description: Get started with Topos. Install the CLI, MCP server, GitNexus composability metrics, and optional semantic coverage.
   :twitter:description: Get started with Topos. Install the CLI, MCP server, GitNexus composability metrics, and optional semantic coverage.

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
   * - Managed Python environment
     - PyPI package
     - Requires Python 3.11+ and ``uv``. Install GitNexus and ``ect-coverage`` separately when needed.
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

   .. tab-item:: PyPI package
      :sync: pypi

      Use this when you want Topos in a managed Python environment.

      .. code-block:: bash

         uv pip install topos-mcp

      Add semantic coverage when needed:

      .. code-block:: bash

         uv pip install "topos-mcp[ect-coverage]"

      PyPI installs do not install GitNexus automatically. Add it separately
      when you need COMPOSABLE metrics:

      .. code-block:: bash

         npm install -g gitnexus

   .. tab-item:: Source checkout
      :sync: source

      Use this for development, local patches, or repository integration.

      .. code-block:: bash

         git clone https://github.com/Krv-Labs/topos.git
         cd topos
         uv pip install -e .

      Add semantic coverage when needed:

      .. code-block:: bash

         uv pip install -e ".[ect-coverage]"

      Source installs do not install GitNexus automatically. Add it separately
      when you need COMPOSABLE metrics:

      .. code-block:: bash

         npm install -g gitnexus

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

         npm install -g gitnexus
         cd /path/to/your/repo
         topos depgraph generate
         topos evaluate src/ -r --gitnexus-dir .gitnexus/

      Re-run ``topos depgraph generate`` after imports, module names, or
      directory structure change. MCP tools auto-detect ``./.gitnexus`` from
      the project root; CLI commands expect ``--gitnexus-dir``.

   .. tab-item:: Semantic coverage
      :sync: coverage

      ``topos coverage`` always reports UAST structural coverage. With
      ``ect-coverage`` installed, it also reports CPG topological coverage.

      .. code-block:: bash

         uv pip install "topos-mcp[ect-coverage]"
         topos coverage src/logic.py --tests tests/test_logic.py

      The embedding model downloads on first use to ``~/.cache/fastembed``.
      Prefer module- or file-scoped PUT/test pairs over whole-repository runs.

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

.. dropdown:: macOS Team ID error on old releases

   If ``topos --version`` fails with a PyInstaller ``different Team IDs`` error
   involving ``libpython3.12.dylib``, upgrade to v0.3.2 or later with the
   installer. Source installs are not affected.

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
