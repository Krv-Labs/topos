.. _preferences:

===========
Preferences
===========

Preferences tell Topos how an agent should trade off quality goals when a file
cannot reach ``IDEAL`` within the available iteration budget.

Topos measures four independent quality generators:

* ``SIMPLE`` — low internal complexity.
* ``COMPOSABLE`` — healthy module coupling.
* ``SECURE`` — no known dangerous calls or taint paths.
* ``NAVIGABLE`` — shallow nesting, cheap for an agent to read.

These generators form a sixteen-element lattice. ``IDEAL`` means all four are
satisfied, but the single-generator states are intentionally incomparable:
``SIMPLE`` is not inherently better than ``SECURE``, ``COMPOSABLE``, or
``NAVIGABLE``. A preference ranking makes that tradeoff explicit.

What Preferences Do
-------------------

``preferences.ranking`` is a strict ordering of **all four** generators — a
three-element ranking written before v0.5.0 is no longer a valid permutation
and is rejected in favour of the default:

.. code-block:: text

   composable > secure > simple > navigable

This means: first try to satisfy all four generators. If that stalls, prefer
the best result that preserves ``COMPOSABLE`` and ``SECURE`` before spending
more effort on ``SIMPLE``.

Topos turns the ranking into a total order over lattice verdicts by weighting
the ranked generators ``8 / 4 / 2 / 1`` — each weight exceeds all the lower
ones combined, making the order strictly lexicographic. With:

.. code-block:: text

   simple > navigable > secure > composable

which is the **default** ranking, the induced order is:

.. list-table::
   :header-rows: 1
   :widths: 40 12 48

   * - Verdict
     - Score
     - Meaning
   * - ``IDEAL``
     - ``15``
     - all four generators satisfied
   * - ``SIMPLE_SECURE_NAVIGABLE``
     - ``14``
     - concedes only the last-ranked generator
   * - ``SIMPLE_COMPOSABLE_NAVIGABLE``
     - ``13``
     -
   * - ``SIMPLE_NAVIGABLE``
     - ``12``
     - fallback target if ``IDEAL`` stalls
   * - ``SIMPLE_COMPOSABLE_SECURE``
     - ``11``
     -
   * - ``SIMPLE_SECURE``
     - ``10``
     -
   * - ``SIMPLE_COMPOSABLE``
     - ``9``
     -
   * - ``SIMPLE``
     - ``8``
     - keeps the top preference only
   * - ``COMPOSABLE_SECURE_NAVIGABLE``
     - ``7``
     - satisfies the lower three preferences
   * - ``SECURE_NAVIGABLE``
     - ``6``
     -
   * - ``COMPOSABLE_NAVIGABLE``
     - ``5``
     -
   * - ``NAVIGABLE``
     - ``4``
     - keeps the second preference only
   * - ``COMPOSABLE_SECURE``
     - ``3``
     -
   * - ``SECURE``
     - ``2``
     - keeps the third preference only
   * - ``COMPOSABLE``
     - ``1``
     - keeps the last preference only
   * - ``SLOP``
     - ``0``
     - no generator satisfied

.. versionchanged:: 0.5.0
   The fallback target is no longer the element directly below ``IDEAL``.
   With three generators, "meet of the top two" and "one step below
   ``IDEAL``" were the same verdict; with four they differ. One step below
   ``IDEAL`` concedes only the lowest-ranked generator
   (``SIMPLE_SECURE_NAVIGABLE`` above); the fallback concedes the bottom
   two.

.. versionchanged:: 0.5.0
   The default ranking is ``simple > navigable > secure > composable``. The
   two pillars an agent can always compute and always fix inside one file
   rank highest; ``COMPOSABLE`` ranks last because it needs an external
   dependency graph and describes a module's place in the whole project,
   so it is the right thing to concede first when coupling data is absent.

The important behavior is the **fallback target**: when ``IDEAL`` plateaus, the
agent should aim for the meet of the top two ranked generators.

.. list-table::
   :header-rows: 1
   :widths: 45 25 30

   * - Ranking
     - First target
     - Fallback target
   * - ``simple > navigable > secure > composable`` (default)
     - ``IDEAL``
     - ``SIMPLE_NAVIGABLE``
   * - ``secure > simple > composable > navigable``
     - ``IDEAL``
     - ``SIMPLE_SECURE``
   * - ``navigable > composable > secure > simple``
     - ``IDEAL``
     - ``COMPOSABLE_NAVIGABLE``

How Agents Use Preferences
--------------------------

When an agent evaluates a file with preferences, Topos returns a
``preference_walk``. The walk gives the agent a concrete sequence of targets:

1. Try ``IDEAL`` first.
2. If ``IDEAL`` stops improving, divert to ``fallback_target``.
3. If that still stalls, follow ``next_step`` down the preference order.

For example, with:

.. code-block:: text

   ranking = simple > navigable > secure > composable
   current = SECURE

Topos can return:

.. code-block:: text

   target          = IDEAL
   fallback_target = SIMPLE_NAVIGABLE
   next_step       = COMPOSABLE_SECURE

``next_step`` is the smallest improvement above the current verdict that still
respects the user's ranking.

How to Set Preferences
----------------------

For a one-off CLI evaluation, pass the complete ranking as a comma-separated
value. Persist project defaults with ``topos config``::

   topos evaluate src/ -r --priority composable,secure,simple
   topos config set --priority composable,secure,simple

In MCP tools, pass ``preferences.ranking``:

.. code-block:: json

   {
     "filepath": "src/server.rs",
     "preferences": {
       "ranking": ["composable", "secure", "simple"]
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
   A single emphasis label used by result metadata and guidance. Current
   pass/fail policies use fixed raw gates and do not change achievement based
   on priority.

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
