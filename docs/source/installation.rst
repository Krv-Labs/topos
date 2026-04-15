.. _installation:

============
Installation
============

.. meta::
   :description: Get started with Topos. Install the CLI, MCP server, or build from source.
   :twitter:description: Get started with Topos. Install the CLI, MCP server, or build from source.

Topos is designed to be accessible as either a standalone binary or by building from source. 

.. hint::
   **Prerequisites:** Topos requires **Python 3.11+** for core evaluation. If you use the Binary installation, the embedded Python environment is managed for you.

.. tab-set::

   .. tab-item:: 🚀 Binary CLI (Recommended)

      The quickest way to get up and running with the Topos CLI and MCP server.

      .. code-block:: bash

         curl -sSL https://raw.githubusercontent.com/Krv-Labs/topos/main/install.sh | sh

      .. card:: 
         :header: **What happens during installation?**

         1. The latest release binary is downloaded to ``~/.local/bin``.
         2. An embedded Python environment is configured.
         3. You will be prompted to optionally install `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_ (requires Node.js 18+).

      .. note::
         **Why GitNexus?** GitNexus enables **Coupling Analysis** (the ``COMPOSABLE`` lattice target). Without it, Topos performs structural analysis (AST-based) but cannot evaluate inter-module dependencies.

      **Verify the Installation**

      .. code-block:: bash

         topos --version
         topos-mcp --help

   .. tab-item:: 🐍 Building from Source

      If you want to contribute, use the latest development version, or integrate Topos into your Python pipelines:

      .. code-block:: bash

         git clone https://github.com/Krv-Labs/topos.git
         cd topos
         pip install -e .

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card:: 🛠️ GitNexus Integration
      :link: https://github.com/abhigyanpatwari/GitNexus
      :link-type: url

      For coupling analysis of dependency graphs, use the built-in generator:
      ^^^
      .. code-block:: bash

         topos depgraph generate

   .. grid-item-card:: 🗑️ Uninstallation
      :shadow: md

      Removing the binary installation is simple and clean:
      ^^^
      .. code-block:: bash

         topos uninstall

Next Steps
----------

Once installed, proceed to the :ref:`Quickstart <index>` or learn about :ref:`Concepts <concepts>` to understand the Diamond Lattice evaluation model.
