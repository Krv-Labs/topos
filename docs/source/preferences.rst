.. _preferences:

===========
Preferences
===========

Preferences tell Topos how an agent should trade off quality goals when a file
cannot reach ``IDEAL`` within the available iteration budget.

Topos measures three independent quality generators:

* ``SIMPLE`` — low internal complexity.
* ``COMPOSABLE`` — healthy module coupling.
* ``SECURE`` — no known dangerous calls or taint paths.

These generators form an eight-element lattice. ``IDEAL`` means all three are
satisfied, but the single-generator states are intentionally incomparable:
``SIMPLE`` is not inherently better than ``SECURE`` or ``COMPOSABLE``. A
preference ranking makes that tradeoff explicit.

What Preferences Do
-------------------

``preferences.ranking`` is a strict ordering of the three generators:

.. code-block:: text

   composable > secure > simple

This means: first try to satisfy all three generators. If that stalls, prefer
the best result that preserves ``COMPOSABLE`` and ``SECURE`` before spending
more effort on ``SIMPLE``.

Topos turns the ranking into a total order over lattice verdicts by weighting
the ranked generators ``4 / 2 / 1``. With:

.. code-block:: text

   simple > composable > secure

the induced order is:

.. list-table::
   :header-rows: 1
   :widths: 28 16 56

   * - Verdict
     - Score
     - Meaning
   * - ``IDEAL``
     - ``7``
     - all three generators satisfied
   * - ``SIMPLE_COMPOSABLE``
     - ``6``
     - fallback target if ``IDEAL`` stalls
   * - ``SIMPLE_SECURE``
     - ``5``
     - keeps the first and third preferences
   * - ``SIMPLE``
     - ``4``
     - keeps the top preference only
   * - ``COMPOSABLE_SECURE``
     - ``3``
     - satisfies the lower two preferences
   * - ``COMPOSABLE``
     - ``2``
     - keeps the second preference only
   * - ``SECURE``
     - ``1``
     - keeps the third preference only
   * - ``SLOP``
     - ``0``
     - no generator satisfied

The important behavior is the **fallback target**: when ``IDEAL`` plateaus, the
agent should aim for the meet of the top two ranked generators.

.. list-table::
   :header-rows: 1
   :widths: 45 25 30

   * - Ranking
     - First target
     - Fallback target
   * - ``simple > composable > secure``
     - ``IDEAL``
     - ``SIMPLE_COMPOSABLE``
   * - ``secure > simple > composable``
     - ``IDEAL``
     - ``SIMPLE_SECURE``
   * - ``composable > secure > simple``
     - ``IDEAL``
     - ``COMPOSABLE_SECURE``

How Agents Use Preferences
--------------------------

When an agent evaluates a file with preferences, Topos returns a
``preference_walk``. The walk gives the agent a concrete sequence of targets:

1. Try ``IDEAL`` first.
2. If ``IDEAL`` stops improving, divert to ``fallback_target``.
3. If that still stalls, follow ``next_step`` down the preference order.

For example, with:

.. code-block:: text

   ranking = simple > composable > secure
   current = SECURE

Topos can return:

.. code-block:: text

   target          = IDEAL
   fallback_target = SIMPLE_COMPOSABLE
   next_step       = COMPOSABLE

``next_step`` is the smallest improvement above the current verdict that still
respects the user's ranking.

How to Set Preferences
----------------------

As of v0.4.0, preferences are an **MCP-only** feature — the ``topos`` CLI's
``evaluate``/``inspect`` commands don't have a ``--preferences`` flag yet
(see :doc:`cli`). In MCP tools, pass ``preferences.ranking``:

.. code-block:: json

   {
     "params": {
       "filepath": "src/server.py",
       "preferences": {
         "ranking": ["composable", "secure", "simple"]
       }
     }
   }

Use ``composable,secure,simple`` for library surfaces where coupling matters
most. Use ``secure,simple,composable`` for files handling untrusted input. Use
``simple,composable,secure`` for leaf implementation files where local
complexity is the main source of drag.

Preferences vs. Priority
------------------------

Preferences and priority are related, but they are not the same thing.

``priority``
   A single scorer knob. It changes how metrics are weighted inside the scoring
   policy for a run.

``preferences.ranking``
   A full target-ordering contract for agents. It decides how to rank lattice
   verdicts, where to divert when ``IDEAL`` stalls, and what ``next_step`` means.

Use preferences when you want the agent to know what kind of silver or bronze
outcome is acceptable if gold is not reachable. Use priority when you only want
to bias the metric scorer for a single evaluation.

Related Tools
-------------

``topos_preference_walk``
   Returns the induced target order without evaluating source code. This is
   useful when an agent needs to refresh the next lattice target between
   refactor iterations.

``topos_evaluate_file`` and ``topos_evaluate_project``
   Include ``preference_walk`` in their structured output when preferences are
   supplied.

``topos_assess_worktree_change``, ``topos_assess_snapshot``, and ``topos_assess_changeset``
   Preserve the same preferences when verifying in-place edits, snapshot baselines,
   or multi-file module splits.

``topos_depgraph_status`` and ``topos_generate_depgraph``
   Surface graph availability and refresh ``.gitnexus/`` when COMPOSABLE is blocked
   by ``missing_gitnexus_dir`` or ``stale_gitnexus_dir`` in ``agent_contract``.
