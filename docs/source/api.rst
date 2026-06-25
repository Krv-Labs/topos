API Reference
=============

Use this reference when integrating with Topos internals, extending evaluation
policies, or embedding graph and metric primitives. For normal command-line
usage, start with :doc:`cli`; for agent workflows, start with :doc:`agents`.

The stable user-facing surfaces are the CLI, MCP tools, and the documented
modules listed here. Modules outside this reference should be treated as
internal unless they are documented elsewhere.

Where to start
--------------

``topos.evaluation``
   Verdicts, policies, suggestions, suppression, and the logic that maps
   measurements onto Topos quality pillars.

``topos.graphs``
   AST, CFG, CPG, MDG, PDG, and UAST representations used as structural lenses
   over source code.

``topos.functors``
   Metric probes and structural comparisons, including complexity, entropy,
   coupling, edit distance, and structural coverage.

``topos.mcp``
   Agent-facing server, tool schemas, formatting, resources, and workflow
   helpers.

``topos.core``
   The mathematical primitives behind programs, morphisms, categories, and the
   evaluation lattice.

``topos.cli``
   Command implementation internals for evaluation, inspection, coverage, and
   diagnostics.

``topos.config``
   Configuration, repository discovery, and parser/runtime helpers.

.. toctree::
   :maxdepth: 2

   api/core
   api/evaluation
   api/graphs
   api/functors
   api/mcp
   api/cli
   api/config
