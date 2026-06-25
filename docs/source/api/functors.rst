Functors
========

Functor modules contain the metric probes and structural comparisons that feed
evaluation. Probes measure a single representation, such as CFG complexity or
MDG coupling; profunctors compare programs or program/test structure, such as
UAST edit distance and structural coverage.

.. autosummary::
   :toctree: generated/functors

   topos.functors.probes.ast.complexity
   topos.functors.probes.ast.entropy
   topos.functors.probes.cfg.complexity
   topos.functors.probes.cfg.paths
   topos.functors.probes.cpg.danger
   topos.functors.probes.cpg.taint
   topos.functors.probes.mdg.coupling
   topos.functors.probes.mdg.fan
   topos.functors.probes.uast.compare
   topos.functors.probes.uast.signature
   topos.functors.profunctors.ast.compare
   topos.functors.profunctors.cfg.compare
   topos.functors.profunctors.cpg.compare
   topos.functors.profunctors.cpg.topological_coverage
   topos.functors.profunctors.mdg.compare
   topos.functors.profunctors.pdg.compare
   topos.functors.profunctors.uast.compare
   topos.functors.profunctors.uast.structural_test_coverage
