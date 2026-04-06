.. _getting_started:

===============
Getting Started
===============

Topos can be used from the command line, as a Python library, or as an MCP server
for AI-assisted workflows. Choose your path below.

.. tab-set::

   .. tab-item:: CLI

      **Evaluate a file**

      .. code-block:: bash

         topos evaluate src/topos/main.py

      **Inspect detailed metrics**

      .. code-block:: bash

         topos inspect src/topos/main.py

      Shows the lattice evaluation, complexity and entropy scores,
      function-level complexity breakdown, and AST metrics.

      **Evaluate a directory**

      .. code-block:: bash

         topos evaluate src/ -r

      Add ``--json`` for machine-readable output:

      .. code-block:: bash

         topos evaluate src/ -r --json

      **Compare two files**

      .. code-block:: bash

         topos compare before.py after.py

      Reports normalized AST edit distance — useful for measuring structural
      drift across a refactor or an AI-generated rewrite.

   .. tab-item:: Python API

      .. code-block:: python

         from topos import ProgramMorphism, SubobjectClassifier

         morphism = ProgramMorphism.from_file("module.py")
         result = SubobjectClassifier().classify_detailed(morphism)

         print(result.evaluation)        # e.g., "◐ COMMODITY"
         print(result.complexity_score)
         print(result.entropy_score)

      To compare two programs:

      .. code-block:: python

         from topos import ProgramMorphism
         from topos.metrics.distance import calculate_ast_distance

         a = ProgramMorphism.from_file("before.py")
         b = ProgramMorphism.from_file("after.py")

         result = calculate_ast_distance(a.ast, b.ast)
         print(f"Similarity: {1 - result.normalized_distance:.1%}")

   .. tab-item:: MCP Server

      Topos ships an MCP server for use with Claude Desktop and other
      MCP-capable clients.

      **Start the server**

      .. code-block:: bash

         topos-mcp

      **Available tools**

      .. list-table::
         :widths: 35 65
         :header-rows: 1

         * - Tool
           - Description
         * - ``evaluate_code``
           - Classify code from a string
         * - ``evaluate_file``
           - Classify code from a file path
         * - ``compare_code``
           - Compare AST distance between two code strings
         * - ``compare_files``
           - Compare AST distance between two files
         * - ``assess_improvement``
           - Check if proposed code improves current
         * - ``inspect_code``
           - Detailed metrics breakdown

      To restrict file tool access to a safe directory:

      .. code-block:: bash

         export TOPOS_MCP_FILE_ROOT=/path/to/workspace
         topos-mcp
