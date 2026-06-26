.. _agent-cost-savings-case-study:

=====================
Agentic Cost Savings
=====================

This case study tests a narrow claim: when an agent starts from structurally
cleaner, Topos-approved code, does the next feature work take less time, fewer tokens, and less
money?

In one controlled experiment, the answer was yes. After a Topos-guided refactor,
four follow-on Gemini feature sessions used **32.6% fewer tokens**, ran **22.9%
faster**, and had **24.6% lower estimated feature-session cost** than the same
features on the unrefactored baseline. Including the upfront refactor, this
single run cost more overall.

That is not a universal benchmark. It is one synthetic business-software
fixture, and more work is needed before making broad claims.

Why this experiment matters
---------------------------

Passing tests are not the end of the story for agent-written code. A feature can
work and still make the next feature harder to add.

This case study asks whether that extra cost shows up in the agent loop itself:
more reading, more tool calls, more tokens, and more time. Topos is already
described elsewhere in these docs; here, the question is narrower. Does a
Topos-guided structural cleanup make the next agent sessions cheaper?

The setup
---------

The fixture was a small healthcare claims engine. It was synthetic, but it had
the texture of business software: payment rules, member eligibility, provider
networks, deductibles, coinsurance, audit messages, imports, exports, and tests.

The baseline had:

* 728 total lines.
* 10 production and test files.
* 10 passing tests.
* 83% line coverage.
* One overloaded ``adjudicator.py`` file at 264 lines.

Topos identified the intended structural problem. The central adjudicator had a
SIMPLE score of 0.0, cyclomatic complexity of 34, and a maximum function
complexity of 48. In other words, the code worked, but the main decision point
had become the place where future changes would get expensive.

The experiment compared two paths from the same baseline:

.. list-table::
   :header-rows: 1
   :widths: 24 56

   * - Condition
     - What happened
   * - **A: no Topos**
     - Gemini added four features directly to the original code.
   * - **B: Topos-guided refactor**
     - Gemini first used Topos to refactor the structure, then added the same
       four features in fresh sessions.

The four features were:

1. Coordination of benefits.
2. Provider network tier pricing.
3. Prior authorization denial and override workflow.
4. Audit export with redaction and tamper-evident hashes.

Both paths used ``gemini-3.1-pro-preview``. Cost was estimated from Gemini CLI
token statistics using the pricing assumptions in the experiment log. Exact
billing can vary by account, request shape, discounts, and cached-token policy.

The result
----------

The honest comparison has two parts.

First, the Topos path paid an upfront refactor cost. Across this single
four-feature run, that made it more expensive overall.

Second, after that cleanup, the follow-on feature work got cheaper. That is the
effect this case study is measuring.

.. list-table::
   :header-rows: 1
   :widths: 24 20 20 20

   * - Metric
     - A: unrefactored features
     - B: features after Topos refactor
     - Change
   * - Wall time
     - 550.5s
     - 424.6s
     - 22.9% faster
   * - Total tokens
     - 3,334,082
     - 2,246,033
     - 32.6% fewer
   * - Tool calls
     - 113
     - 86
     - 23.9% fewer
   * - Estimated cost
     - $3.15
     - $2.37
     - 24.6% lower
   * - Final tests
     - 27 passed
     - 30 passed
     - Both passed

The final test counts are not a quality score; the two implementations added
different tests. The important point is that both paths finished with passing
test suites.

Refactor cost and payback
~~~~~~~~~~~~~~~~~~~~~~~~~

The refactor was not free. The Topos-guided cleanup took 368.8 seconds,
used 2.67 million tokens, and cost an estimated $2.23.

Including that upfront work, the Topos path cost more in this single run:
$4.61, compared with $3.15 for the unrefactored path. The measured saving came
later. The four follow-on feature sessions cost $2.37 after the refactor,
versus $3.15 on the unrefactored baseline, a feature-only saving of $0.77.

At that observed rate, the refactor would need about three similar four-feature
batches, or roughly 12 comparable follow-on features, to pay back its initial
cost.

.. list-table:: Simple payback view
   :header-rows: 1
   :widths: 28 32 40

   * - Scenario
     - Cost counted
     - Result
   * - One four-feature run
     - Refactor plus features
     - Topos path cost $4.61 vs $3.15 baseline
   * - Feature work only
     - Four follow-on features after refactor
     - $0.77 cheaper than baseline
   * - Break-even
     - Same observed saving rate
     - About three four-feature batches, or about 12 follow-on features

This should be read as payback arithmetic, not as a general guarantee. A
different codebase, model, feature mix, or refactor target could produce a
different result.

There is also an accounting distinction. In this experiment the code was first
allowed to become structurally expensive, then cleaned up in a separate pass.
That creates a visible refactor cost. If similar structure is maintained from
the start, the same work is not a separate cleanup bill; it is routine
maintenance, like sharpening a tool before it becomes too dull to use
efficiently.

Future work is to measure Topos inside the agent loop, where structural guidance
could be applied during feature development rather than as a retrospective
cleanup step. That optimization is an intent, not a result measured by this run.

After the Topos-guided refactor, the overloaded adjudicator was split into
smaller rule modules. Its maximum function complexity fell from 48 to 9. Its
Topos SIMPLE score, a 0-100 measure of local structural complexity, improved
from 0.0 to 55.0 while the test suite stayed green.

The agent had less reconstruction to do. It could add features against clearer
boundaries instead of repeatedly re-reading one giant function to work out where
each new rule belonged.

What this does and does not prove
---------------------------------

The useful lesson is modest but important:

.. pull-quote::

   A Topos-guided refactor turned vague cleanup into a measurable target, and
   the next four feature sessions became cheaper.

Topos is not just scoring code after the fact. In this run, it pre-computed
structural information the agent would otherwise have had to infer from scratch.
Cleaner code going in meant fewer tokens spent reconstructing the code later.

This experiment does not prove that Topos will save 32.6% of tokens on every
project. It does not prove the same result for every model, language, codebase,
or team. It also does not compare Topos against an expert human refactor or a
separate unguided cleanup condition.

The result should be read as one case study:

* The fixture was realistic but synthetic.
* The run count was small.
* The model was fixed to one Gemini preview model.
* The cost estimate was approximate.
* The benefit appeared after paying an upfront refactor cost.

Those caveats matter. A serious benchmark should repeat the experiment across
larger repositories, different code shapes, multiple agent models, and both
Topos-guided and unguided refactor baselines.

The practical claim is narrower than a benchmark but still useful: in this run,
structural feedback made subsequent agent work cheaper to complete.
