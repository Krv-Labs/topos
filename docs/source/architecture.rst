.. _architecture:

============
Architecture
============

Topos evaluates programs through representation-specific metrics that are
aggregated in a single lattice classifier.

Representation Model
--------------------

Representations implement a shared protocol and expose namespaced metrics:

- ``ast`` (via ``ASTRepresentation``)
- ``depgraph`` (via ``DependencyGraph``)

Examples of emitted metric keys:

- ``ast.complexity``
- ``ast.entropy``
- ``depgraph.coupling``
- ``depgraph.instability``
- ``depgraph.fan_in`` / ``depgraph.fan_out``
- ``depgraph.dep_depth``

Depgraph metrics are built from repository-level GitNexus output in
``.gitnexus/lbug``, then evaluated per target file.

Policy note: all depgraph metrics are computed and exposed in inspection
output, but only ``depgraph.coupling`` and ``depgraph.instability`` currently
contribute verdicts to lattice aggregation.

Classification Flow
-------------------

.. mermaid::

   flowchart LR
      programSource[ProgramSource] --> programMorphism[ProgramMorphism]
      programMorphism --> astRepresentation[ASTRepresentation]
      programMorphism --> depgraphRepresentation[DependencyGraph]
      astRepresentation --> astMetrics["ast.complexity, ast.entropy"]
      depgraphRepresentation --> depgraphMetrics["depgraph.coupling, depgraph.instability, depgraph.fan_in, depgraph.fan_out, depgraph.dep_depth"]
      astMetrics --> classifierOmega[SubobjectClassifier]
      depgraphMetrics --> classifierOmega
      classifierOmega --> latticeValue[EvaluationValue]

By default, Topos evaluates AST metrics. Dependency-graph metrics are included
when a dependency graph representation is provided (for example with
``topos evaluate --gitnexus-dir .gitnexus``).

If ``--gitnexus-dir`` is supplied and the depgraph representation cannot be
constructed, evaluation fails fast instead of silently falling back to AST-only
classification.
