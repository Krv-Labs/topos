.. _quickstart:

===========
Quick Start
===========

.. meta::
   :description: Start using Topos where coding agents already work: Claude Code, VS Code, or Cursor.
   :twitter:description: Start using Topos where coding agents already work: Claude Code, VS Code, or Cursor.

Topos is most useful when it sits in the loop with the agent changing your
code. Install it where you review and iterate: Claude Code, VS Code, or Cursor.
The terminal is still useful, but the point is not to collect another report;
the point is to give your agent a quality target it can actually chase.

Agents can write the mess, then burn your tokens reading it back. Topos helps
stop that loop. Cleaner code going in means fewer future tokens spent on
archaeology, faster context loading, and less refactoring drag on every feature
that follows.

.. admonition:: The short version
   :class: philosophy-box

   **Recommended:** run Topos through Claude Code, VS Code ``@mcp topos``, or
   the optional full VS Code extension. If you want package installs, source
   builds, or other MCP hosts, use :doc:`installation` and :doc:`agents`.

Install
-------

Choose the environment where your agent already works. Both options run Topos
locally and expose the same structural quality tools.

.. dropdown:: Claude Code

   Best when you already use Claude Code for repo work. Topos becomes the
   structural verifier in the same agent loop: measure, edit, verify, repeat.

   You do not need a persistent install. Let ``uvx`` resolve and run the
   ``topos-mcp`` server binary when Claude starts the MCP server — the
   PyPI ``topos-mcp`` package is a thin wheel bundling that compiled Rust
   binary, so there's no Python runtime involved.

   .. code-block:: bash

      claude mcp add --transport stdio topos -- uvx topos-mcp

   Prefer the full ``topos`` binary (CLI + MCP server) instead? Install it
   with the script in :doc:`installation`, then run
   ``claude mcp add --transport stdio topos -- topos mcp``.

   To confirm the install + MCP server status, you can run:

   .. code-block:: bash

      claude mcp list

   Then ask Claude Code:

   .. code-block:: text

      Use Topos to evaluate the worst files in <whatever directory/project you are working on>.
      Propose a refactor, make the change, and verify it with Topos.

   This is the happy path: the agent gets a concrete structural signal instead
   of a vague "clean it up" prompt. Tests say whether it ran; Topos tells the
   agent whether the shape improved.

.. dropdown:: VS Code / Cursor

   Best when your review happens in the editor. Pick **one** install path — do
   not install both, or agent mode can register two Topos MCP servers.

   **Recommended — MCP server (``@mcp`` gallery)**

   1. Extensions view → search ``@mcp topos`` → install **Topos**.
   2. Or use **Install MCP server** on the `GitHub MCP Registry page
      <https://github.com/mcp/Krv-Labs/topos>`_.

   VS Code pulls the registry PyPI package (``topos-mcp``) and owns
   registration, trust, and lifecycle — no separate Topos install required.

   **Optional — full Marketplace extension** (Command Palette + bundled runtime)

   .. button-link:: https://marketplace.visualstudio.com/items?itemName=KrvLabs.topos-vscode
      :color: primary
      :shadow:

      Install ``KrvLabs.topos-vscode``

   In agent mode, ask:

   .. code-block:: text

      Use Topos to evaluate this project and identify the lowest-hanging
      structural improvement.

   Full setup, and why not to install both paths, is in :doc:`agents`.

When to use Topos
-----------------

Use Topos when the code works, but you are not sure it can keep taking more
changes.

What Topos is
~~~~~~~~~~~~~

Topos is a structural code-quality evaluator for agent-written and
human-written code. It exists for the judgment gap created when agents generate
code faster than humans can review it. Binary pass/fail tests prove the code
ran; they do not prove the implementation belongs in the codebase.

Topos gives that review a measurable object: program structure. It asks whether
the implementation is simple enough to reason about, composable enough to fit
the repository, and free of known dangerous data-flow patterns.

Topos pre-computes the structural signals your agent would otherwise have to
rediscover by scanning the codebase: cyclomatic complexity, module coupling,
dangerous API reachability, and test-structure coverage. It gives the agent a
precise gradient to optimize against before the mess compounds, not after.

The useful mental model is simple: **tests catch broken behavior; Topos catches
structural drift.** Tests tell you whether the code produced the expected
answer. Topos asks whether the code is built in a way your team can understand,
change, and trust later.

What Topos is not
~~~~~~~~~~~~~~~~~

Topos is not a replacement for unit tests, integration tests, type checks, or
linters. Keep those gates.

Tests prove behavior against examples. Type checks prove values fit declared
contracts. Linters catch style and known syntax-level mistakes. Topos adds a
different layer: **is the structure healthy enough to keep changing?**

Why that matters
~~~~~~~~~~~~~~~~

That distinction matters. A feature can pass every test today and still be the
wrong shape for tomorrow.

.. admonition:: The Jenga Analogy
   :class: philosophy-box

   Think of your codebase as a Jenga tower. An agent implements a feature, the
   tests pass, and the tower remains standing—functionally, the move "worked."
   But if they succeeded only by sliding a load-bearing block out of the
   foundation to balance three quick patches on top, the next change becomes
   twice as dangerous. Topos catches that structural compromise before the
   system becomes too brittle to touch.

Another way to think about it: a unit test checks that the door opens. Topos
checks whether the hinges are now screwed into drywall.

This is also a cost problem. If code is tangled, every future agent session has
to spend more context figuring out what changed, why it matters, and where the
real boundaries are. Cleaner structure makes sessions last longer because the
agent spends less time re-reading its own homework.

Concrete use cases
~~~~~~~~~~~~~~~~~~

Concrete times to use Topos:

* **After an agent writes code:** verify that the diff is not just correct, but
  still simple, composable, and safe to build on.
* **Before merging a large generated diff:** use Topos when the review is too
  big to trust by vibe.
* **Before asking for the next feature:** catch structural debt before the next
  session spends paid context rediscovering it.
* **When a passing implementation feels too clever:** check whether complexity
  is concentrated in one tangled function.
* **When a file imports too much:** catch modules that quietly become
  load-bearing because they know about half the codebase.
* **During refactors:** verify that the structure actually improved, instead of
  just moving lines around.
* **When agent sessions keep getting expensive:** use Topos to reduce the deep
  codebase scans and repeated explanations caused by tangled code.
* **When reviewing tests:** check whether the tests structurally cover the code
  paths and declarations they claim to protect.
* **Inside an agent loop:** let the agent measure, refactor, and verify its own
  work against a concrete quality target.

How it works
------------

Do not let the academic jargon fool you. Topos uses graph terminology because
that is the right machinery under the hood, but the product question is plain:
how messy, tangled, or risky is this code?

Think of it like a vehicle inspection report for software. The engine may
start, but you still want to know whether the brakes are worn, the frame is
cracked, or the steering is loose. Topos checks three independent things.

See :doc:`measures` and :doc:`concepts` for more information.

SIMPLE: local complexity
~~~~~~~~~~~~~~~~~~~~~~~~

**Question:** how hard is this function or file to read, test, and understand?

SIMPLE looks inside the code. It uses an AST, which is a map of the code's
grammar, and a CFG, which is a map of the roads execution can take.

Cyclomatic complexity counts decision points such as ``if``, ``while``, and
``for``. A function that prints ``"hello"`` has one path. Add an ``if`` and
there are two paths. Keep nesting branches and the number of paths climbs until
the function becomes hard to reason about and hard to test.

Token entropy looks at how predictable or chaotic the code's vocabulary is. A
small, regular function tends to be easier to understand. A function packed
with unrelated names, one-off variables, and surprising operations usually
deserves suspicion.

The practical fix is usually familiar: split the function, reduce branching,
name the pieces better, and make the control flow boring.

COMPOSABLE: module coupling
~~~~~~~~~~~~~~~~~~~~~~~~~~~

**Question:** if this file changes, how much of the repo has to care?

COMPOSABLE zooms out from one function to the module dependency graph, built by
`GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_. It looks at how
files and modules depend on each other.

Fan-out measures how many other modules a file relies on. If
``UserPayment.js`` needs twenty local modules just to run, it is carrying a lot
of coupling. Martin instability describes the shape of those relationships. A
module that depends on everyone else but is used by nobody is easy to edit, but
fragile because upstream changes keep hitting it. A module that everyone uses
is stable, but dangerous to change because one bad move can shake the whole
system.

This is where the Jenga metaphor matters. Blocks near the top are easy to move.
Blocks near the bottom are load-bearing. Topos helps an agent notice when it is
turning ordinary feature code into a load-bearing block, or balancing too much
logic on a wobbly one.

The practical fix is to reduce fan-out, split responsibilities, move shared
interfaces into cleaner boundaries, and stop making one file know everything.

SECURE: data-flow safety
~~~~~~~~~~~~~~~~~~~~~~~~

**Question:** can untrusted input reach something dangerous?

SECURE uses a code property graph to follow data through the program, with
supplementary findings from the embedded `Sighthound <https://github.com/Corgea/Sighthound>`_
engine. It looks for dangerous API reachability and taint paths.

"Taint" just means untrusted input: a URL parameter, a form field, a search box,
or anything else a user can control. A taint path is the trail that value takes
through your code. If a user-provided string moves through a few variables and
lands in ``eval()``, a shell command, or a raw database call without validation,
that is the kind of path SECURE is meant to flag.

The practical fix is to break the path: validate input, sanitize it, remove the
dangerous call, or move the operation behind a safer API.

Medals and agent iteration
~~~~~~~~~~~~~~~~~~~~~~~~~~

Each file gets a result across the three pillars. ``GOLD`` means SIMPLE,
COMPOSABLE, and SECURE all pass. ``SILVER`` means two pass. ``BRONZE`` means
one passes. ``SLOP`` means none pass or the file cannot be parsed.

The point is not the medal itself. The point is that each failure tells an
agent what kind of improvement to make next. "Make this better" is a vibes
prompt. "Reduce branching in this function" or "lower fan-out in this module"
is an engineering target.

Preferences tell the agent which tradeoff matters most under time and token
budgets. If ``GOLD`` is not reachable, the lattice defines the next-best target
instead of leaving the agent stuck.

Because Topos measures graph structure, cosmetic changes such as comment
padding or line shuffling should not move the score. It rewards the change you
actually wanted, not the performance of looking busy.
