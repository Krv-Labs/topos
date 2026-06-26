Graphs
======

Graph modules build and expose the structural representations Topos measures:
AST, CFG, CPG, MDG, PDG, and UAST. Use this section when adding language
support, debugging parser output, or tracing how source code becomes measurable
program structure.

.. autosummary::
   :toctree: generated/graphs

   topos.graphs.base
   topos.graphs.ast.dispatch
   topos.graphs.ast.object
   topos.graphs.ast.types
   topos.graphs.ast.providers.base
   topos.graphs.ast.providers.native_provider
   topos.graphs.ast.providers.tree_sitter_provider
   topos.graphs.cfg.builder
   topos.graphs.cfg.models
   topos.graphs.cfg.object
   topos.graphs.cpg.builder
   topos.graphs.cpg.models
   topos.graphs.cpg.object
   topos.graphs.mdg.object
   topos.graphs.pdg.object
   topos.graphs.uast.mapper_common
   topos.graphs.uast.mapper_cpp
   topos.graphs.uast.mapper_javascript
   topos.graphs.uast.mapper_python
   topos.graphs.uast.mapper_rust
   topos.graphs.uast.mapper_typescript
   topos.graphs.uast.models
