.. _concepts:

========
Concepts
========

This page provides the mathematical foundations of Topos. It explains how we use category theory—specifically topos theory—to model code quality as a structural property of programs. For a practical breakdown of the metrics we use, see :doc:`measures`.

Topos Evaluations: The Quality Pillars
--------------------------------------

Topos classifies code against an eight-valued **Quality Badge** lattice (technically a free Heyting algebra on three generators). This captures *degrees of independent quality* rather than a single pass/fail score.

The three generators—**SIMPLE**, **COMPOSABLE**, and **SECURE**—represent the three **Quality Pillars** of software quality in Topos. They are *pairwise incomparable*, meaning a program can excel in one while failing in another:

*   **SIMPLE** (Code Complexity): Evaluates internal readability and logic flow.
*   **COMPOSABLE** (Module Coupling): Evaluates how a module relates to the rest of the system.
*   **SECURE** (Data Flow Safety): Evaluates the absence of dangerous operations and taint flows.

A program can earn any combination of these pillars, unlocking different **Quality Badges** (e.g., the ``SIMPLE_COMPOSABLE`` badge, or the ultimate ``IDEAL`` badge). This preserves nuances: Topos never collapses a security failure into a complexity score.

Code Quality as a Characteristic Morphism
-----------------------------------------

In Topos, we treat programs as mathematical objects that can be classified based on their structure.

**Programs as Graphs**
   We model programs as graphs (or systems of graphs) within the **Category of Graphs**. A program isn't just a text file; it is a Control Flow Graph (CFG), a Code Property Graph (CPG), and a node in a Module Dependency Graph (MDG).

**The Subobject Classifier ( :math:`\Omega` )**
   In topos theory, a **topos** is a category that behaves like the category of sets, specifically one that possesses a **subobject classifier**. The subobject classifier, denoted :math:`\Omega`, is an object that represents "truth values." While a standard topos (like Set) has :math:`\Omega = \{True, False\}`, Topos introduces a custom :math:`\Omega`—the eight-valued Heyting algebra of our quality pillars.

**The Characteristic Morphism ( :math:`\chi` )**
   The "quality" of a program is defined by its **characteristic morphism**. This is a mapping:

   .. math::

      \chi: \text{Program} \to \Omega

   This map classifies a program by sending it to a specific element in the lattice :math:`\Omega`. This classification is decided based on structural descriptions of the program's graph representations. For the specific metrics that determine this map, see :doc:`measures`.

Representations, Probes, and Profunctors
------------------------------------------

The characteristic morphism is computed through three levels of abstraction:

**Representations**
   Internal graph structures built from the source code (CFG, CPG, MDG). These capture the raw structural data of the program.

**Probes**
   Functions that map a representation to a real-valued metric :math:`f: \text{Rep} \to \mathbb{R}`. For example, a probe on the CFG calculates cyclomatic complexity. Probes are the "sensors" that feed the characteristic morphism.

**Profunctors**
   Relational tools that operate *between* two programs (or a program and its tests). They compute structural distances or correspondences:
   
   - **AST Distance**: Measures the topological drift between two programs using UAST edit distance.
   - **Structural Coverage**: Estimates how much of a program's structure is "covered" by a test suite.

Profunctors operate outside the main three-generator lattice but provide essential signals for agent workflows, such as detecting if a refactor was purely cosmetic or structurally significant.

User Preferences and the Relaxation Walk
-----------------------------------------

While the lattice :math:`\Omega` is only partially ordered, users often have specific quality goals. Topos uses **User Preferences** — a strict total order (permutation) of the three pillars — to linearize the lattice and guide agent iteration.

**The Induced Total Order**
   A preference ranking like ``(SIMPLE, COMPOSABLE, SECURE)`` induces a total order on the 8 Quality Badges. This allows the system to score every verdict :math:`v \in \Omega` lexicographically based on which pillars are satisfied.

**Aspirational vs. Pragmatic Targets**
   - **Aspirational Target**: Usually ``IDEAL`` (⊤). The agent first attempts to satisfy all three quality pillars.
   - **Pragmatic Target (The "Ideal Intersection")**: The meet (∧) of the top-two ranked pillars. If the agent plateaus while aiming for ``IDEAL``, it naturally diverts to this fallback.

**The Relaxation Walk**
   Given a preference ranking and a current verdict, Topos calculates a **relaxation walk** — the descending sequence of Quality Badges from the target down to the current state. Agents use this walk to identify the "next step" improvement, ensuring that every refactor iteration moves the codebase closer to the user's intent.
