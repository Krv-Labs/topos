.. _getting_started:

===============
Getting Started
===============

Topos evaluates Python code and returns a verdict — one of six stages.
You can run it from the CLI, use the Python API, or connect it as an MCP server
so your AI tool evaluates its own output automatically.

For what each verdict means and how to act on it, see :doc:`architecture`.

.. tab-set::

   .. tab-item:: CLI

      **Evaluate a file**

      .. code-block:: bash

         topos evaluate src/topos/main.py

      **Inspect detailed metrics**

      .. code-block:: bash

         topos inspect src/topos/main.py

      Shows the verdict, complexity and entropy scores, function-level
      breakdown, and AST metrics.

      **Evaluate a directory**

      .. code-block:: bash

         topos evaluate src/ -r

      Add ``--json`` for machine-readable output:

      .. code-block:: bash

         topos evaluate src/ -r --json

      **Add coupling metrics**

      If you have `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_
      set up (see :doc:`installation`), pass ``--gitnexus-dir`` to include
      dependency-graph metrics alongside the structural ones:

      .. code-block:: bash

         topos evaluate src/ -r --gitnexus-dir .gitnexus
         topos inspect src/topos/main.py --gitnexus-dir .gitnexus

      If ``--gitnexus-dir`` is provided but the data can't be loaded,
      Topos exits with an error rather than silently skipping coupling.

      **Compare two files**

      .. code-block:: bash

         topos compare before.py after.py

      Reports AST edit distance — useful for measuring structural drift
      across a refactor or AI-generated rewrite.

   .. tab-item:: Python API

      .. code-block:: python

         from topos import ProgramMorphism, SubobjectClassifier

         morphism = ProgramMorphism.from_file("module.py")
         result = SubobjectClassifier().classify_detailed(morphism)

         print(result.dimensions)        # {"structural": <EvaluationValue.STABLE: 4>}
         print(result.summary())         # e.g., "◐ STABLE"
         print(result.raw_metrics)       # {"ast.complexity": 12.0, "ast.entropy": 0.44}

      **Metrics**

      .. code-block:: python

         from topos.metrics.ast.complexity import calculate_cyclomatic_complexity
         from topos.metrics.ast.entropy import calculate_kolmogorov_proxy

      Common AST metrics are also re-exported from ``topos.metrics`` for convenience.

      **Comparing two programs**

      .. code-block:: python

         from topos import ProgramMorphism
         from topos.metrics.distance import calculate_ast_distance

         a = ProgramMorphism.from_file("before.py")
         b = ProgramMorphism.from_file("after.py")

         result = calculate_ast_distance(a.ast, b.ast)
         print(f"Similarity: {1 - result.normalized_distance:.1%}")

   .. tab-item:: MCP Server

      Topos ships an MCP server so Claude, Cursor, or other MCP-capable tools
      can evaluate code without leaving the conversation.

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

      To restrict file access to a specific directory:

      .. code-block:: bash

         export TOPOS_MCP_FILE_ROOT=/path/to/workspace
         topos-mcp
