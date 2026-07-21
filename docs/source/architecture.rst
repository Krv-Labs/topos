.. _architecture:

============
Architecture
============

.. meta::
   :description: How Topos is built — the Rust crate workspace, external tool adapters, and the advisory refactor suite.
   :twitter:description: How Topos is built — the Rust crate workspace, external tool adapters, and the advisory refactor suite.

As of v0.4.0 (`PR #159 <https://github.com/Krv-Labs/topos/pull/159>`_) Topos
is an all-Rust `Cargo workspace <https://github.com/Krv-Labs/topos/tree/main/topos>`_.
There is no Python implementation anywhere in the stack — the ``topos-mcp``
PyPI package is a thin wheel bundling a compiled binary, not a Python
package. For the underlying math, see :doc:`concepts`; for what each metric
means, see :doc:`measures`.

The three crates
-----------------

.. list-table::
   :header-rows: 1
   :widths: 20 80

   * - Crate
     - Role
   * - ``topos-engine``
     - The pure compute engine: tree-sitter AST parsing, the CFG / MDG / CPG
       / PDG / UAST graph representations, the characteristic morphism
       :math:`\chi_S : \text{Program} \to \Omega`, the SIMPLE/COMPOSABLE/
       SECURE scoring policies, and all refactor-suite probes (cycle basis,
       Forman/Forman-Ricci curvature, Graphify orphan detection). External
       tools (GitNexus, Graphify) are reached only through ``adapters::``
       subprocess wrappers — never library imports — so ``topos-engine`` never
       depends on anything installed outside the Rust toolchain.
   * - ``topos``
     - The CLI binary: ``evaluate`` / ``inspect`` / ``compare`` /
       ``coverage`` / ``graphify`` / ``mcp``. Calls straight into
       ``topos-engine``; no logic is duplicated with ``topos-mcp``.
   * - ``topos-mcp``
     - The MCP server (the ``topos-mcp`` binary, and also what ``topos mcp``
       launches in-process). One ``#[tool_router]`` module per tool family
       (evaluate, assess, compare, coverage, depgraph, docs, graphify,
       inspect, preferences, refactor — eighteen tools total, see
       :doc:`agents`). Embeds the `Sighthound <https://github.com/Corgea/Sighthound>`_
       SAST engine as a compiled-in library dependency (not a subprocess) for
       supplementary security findings.

.. raw:: html

   <figure class="topos-figure">
     <img class="only-light" src="_static/figures/topos-methods.svg" alt="AST, CFG, PDG, and MDG graph lenses glued over a shared source-coordinate base, amalgamated into a single code property graph." />
     <img class="only-dark" src="_static/figures/topos-methods-dark.svg" alt="" aria-hidden="true" />
     <figcaption>Each lens reads the same source coordinates; topos-engine amalgamates them into one code property graph, then measures structure over the unified space.</figcaption>
   </figure>

External tools stay at the edges
---------------------------------

Two external tools feed Topos, and both are reached the same way — a
subprocess adapter in ``topos-engine::adapters``, never a library dependency
pulled into the scoring path:

`GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_
   Builds the cross-file module dependency graph that the COMPOSABLE pillar
   scores. This is the one external tool whose output actually feeds the
   evaluation lattice.

`Graphify <https://github.com/Graphify-Labs/graphify>`_
   Builds a tree-sitter-based knowledge graph consumed only by the advisory
   refactor suite's ``graphify`` target (orphan/dead-code detection, fragile
   low-confidence edges). **Never** feeds SIMPLE/COMPOSABLE/SECURE.

`Sighthound <https://github.com/Corgea/Sighthound>`_ is different from
these two: it's compiled directly into ``topos-mcp`` as a Rust library
dependency (no subprocess, no ``$PATH`` discovery), and it only supplies
supplementary ``security_findings`` detail — the SECURE score itself always
comes from ``topos-engine``'s native CPG probes, never from Sighthound.

The advisory refactor suite
-----------------------------

Beyond the scored medal, ``topos_refactor`` (MCP) and, for Graphify, the
``topos graphify`` CLI subcommand, surface ranked structural hotspots from
four independent engines. **None of these feed SIMPLE/COMPOSABLE/SECURE** —
see :doc:`agents` for the tool contract and ``docs/decisions/refactor-suite.md`` in the
repository for the full design rationale:

- ``cycles`` — CFG cycle-basis extraction (persistent-homology-flavored),
  pointing at the actual loop/branch bodies behind cyclomatic complexity.
- ``dependencies`` — balanced Forman curvature over the MDG, naming
  load-bearing import edges.
- ``process`` — directed Forman-Ricci curvature over GitNexus process
  graphs, flagging execution-path choke points.
- ``graphify`` — degree and confidence over a Graphify knowledge graph,
  flagging likely dead code and fragile (``INFERRED``/``AMBIGUOUS``) edges.

Rust API docs
--------------

This Sphinx site doesn't autodoc the Rust crates (there's no Python module
to introspect anymore). For the compiled API surface, generate rustdoc
locally:

.. code-block:: bash

   git clone https://github.com/Krv-Labs/topos.git
   cd topos
   cargo doc --workspace --no-deps --open

``topos-engine`` and ``topos`` are not yet published to crates.io (tracked
in `issue #149 <https://github.com/Krv-Labs/topos/issues/149>`_); until then,
rustdoc generated locally or the source on GitHub are the primary references.
