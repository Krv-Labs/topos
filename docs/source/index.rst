.. _index:

=====
Topos
=====

**Topos** gives every piece of Python code a plain-language verdict — one of six stages — so
you always know whether code is clean, tangled, or broken, and exactly what to fix.

.. admonition:: Where to start

   | **Using AI tools to write code?**
   | → :doc:`architecture` — what each verdict means and how to act on it.
   |
   | **Building with or extending Topos?**
   | → :doc:`getting_started` for commands, then :doc:`concepts` for the theory.

Quick look
----------

.. code-block:: bash

   topos evaluate src/ -r              # classify a directory
   topos inspect module.py             # detailed metrics
   topos compare before.py after.py    # AST edit distance

Each file gets a verdict per quality dimension — **structural** (complexity, entropy)
and optionally **coupling** (dependency graph). You always see which axis is the
problem, not a single blended number.

For coupling metrics, point Topos at a
`GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_ directory:

.. code-block:: bash

   topos evaluate src/ -r --gitnexus-dir .gitnexus

See :doc:`getting_started` for the full CLI, Python API, and MCP server usage.

How it works
------------

Topos measures two things about every file and maps them to one of six verdicts:

- **Structure** — how many decision paths run through the code (complexity) and how
  predictable its pattern is (entropy).
- **Coupling** — how tightly the file is wired to the rest of the codebase (optional).

The six verdicts range from ``SOUND`` (clean, well-scoped) down to ``BROKEN``
(syntax error). See :doc:`architecture` for the full table and what to do with each.

.. note::

   Under the hood, Topos models code as a **morphism** and evaluates it via a
   six-valued `Heyting algebra <https://en.wikipedia.org/wiki/Heyting_algebra>`_.
   See :doc:`concepts` for the formal details.

.. toctree::
   :maxdepth: 1
   :caption: Documentation
   :hidden:

   installation
   getting_started
   concepts
   For AI-assisted coding <architecture>
