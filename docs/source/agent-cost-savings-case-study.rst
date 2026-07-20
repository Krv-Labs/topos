.. _agent-cost-savings-case-study:

=====================
Agentic Cost Savings
=====================

This case study tests a narrow claim: when an agent starts from structurally
cleaner code, does the next feature work take less time, fewer tokens, and less
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
different tests. They show only that both paths ended with passing suites.

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

This accounting is specific to a retrospective cleanup. In this run, the code
was first allowed to become structurally expensive and then refactored in a
separate pass, so the cleanup appears as an upfront cost. If structural feedback
is used during ordinary feature work, the economics may differ; this experiment
did not measure that workflow.

After the Topos-guided refactor, the overloaded adjudicator was split into
smaller rule modules. Its maximum function complexity fell from 48 to 9. Its
Topos SIMPLE score, a 0-100 measure of local structural complexity, improved
from 0.0 to 55.0 while the test suite stayed green.

The likely mechanism is simpler boundaries: the agent had less code to
reconstruct before deciding where each new rule belonged.

Coverage guardrails
~~~~~~~~~~~~~~~~~~~

Coverage was collected as a guardrail, not as the headline result. Ordinary
pytest coverage asks which lines ran during the tests. Topos structural coverage
asks whether tests cover the declarations and code shapes in the
program-under-test. In this experiment, both signals were useful, but neither
was the source of the cost-savings claim.

.. list-table:: Coverage snapshots
   :header-rows: 1
   :widths: 28 20 24 28

   * - Snapshot
     - Tests
     - Line coverage
     - Topos structural
   * - Baseline
     - 10
     - 83%
     - 0.867
   * - B cleanup
     - 10
     - 87%
     - 0.817
   * - A final
     - 27
     - 88%
     - 0.843
   * - B final
     - 30
     - 91%
     - 0.853

The Topos cleanup changed structure without expanding the test suite: the same
10 tests still passed, and line coverage rose from 83% to 87%.

Structural coverage dipped during cleanup, from 0.867 to 0.817, because the
refactor increased production declarations from 19 to 32 while the tests stayed
fixed. That is a useful check on the interpretation. Topos did not make the
numbers look better by adding tests during cleanup; it changed the code shape
and preserved behavior.

The later feature sessions added tests. By final B, the test suite had 30 tests
and 91% line coverage, versus 27 tests and 88% line coverage in A final. Topos
structural coverage stayed above threshold in both final snapshots: 0.853 for B
and 0.843 for A.

This fixture was small enough that an agent could still inspect much of the
production and test code directly. That limits what structural coverage can
prove here. A larger-system hypothesis is that precomputed structural coverage
may reduce paid reading and review uncertainty, but this run only used coverage
as a guardrail.

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

.. dropdown:: Appendix: recreate a similar experiment

   This is not a byte-for-byte reproduction script. It is the shape of the
   experiment, written so another agent can build a comparable fixture and run
   the same A/B comparison.

   .. note::
      This experiment predates the v0.4.0 Rust migration and its CLI
      commands use pre-migration flags (``topos evaluate --gitnexus-dir
      ... --json``, ``topos coverage --json``) that the current ``topos``
      CLI doesn't have yet — COMPOSABLE and JSON output are MCP-only as of
      v0.4.0 (see :doc:`cli`). Reproducing this today means substituting the
      equivalent MCP tool calls (``topos_generate_depgraph``,
      ``topos_evaluate_project(gitnexus_dir=...)``,
      ``topos_calculate_coverage``) for the ``--gitnexus-dir``/``--json``
      commands below.

   **Fixture target**

   Build a small Python package named ``claims_engine`` with tests. The package
   should model healthcare claim adjudication and include enough cross-cutting
   rules that new features have to touch payment, denial, patient
   responsibility, and audit behavior.

   Aim for this baseline shape:

   * about 700-800 total lines,
   * about 10 production and test files,
   * a passing pytest suite,
   * roughly 80% line coverage,
   * one intentionally overloaded ``claims_engine/adjudicator.py`` file of
     about 250 lines,
   * one main adjudication function with high branching complexity.

   The baseline should include:

   * member eligibility,
   * payer plan rules,
   * provider network status,
   * procedure allowed amounts,
   * modifiers,
   * specialty reductions,
   * deductibles,
   * coinsurance,
   * copays,
   * out-of-pocket maximums,
   * hardship credit,
   * audit messages,
   * CSV/JSON import,
   * JSON decision export.

   **Initial structural and coverage checks**

   After creating the baseline, run tests and collect Topos structural and
   coverage snapshots.

   .. code-block:: bash

      python -m pytest -q
      python -m pytest --cov=claims_engine --cov-report=term-missing

      gitnexus analyze --force --skip-git --index-only --skip-agents-md .

      topos evaluate claims_engine \
        -r \
        --gitnexus-dir .gitnexus \
        --json > baseline_topos.json

      topos coverage \
        --tests tests/test_claims_engine.py \
        --json \
        $(rg --files -g '*.py' claims_engine) \
        > baseline_coverage.json

   The fixture used in this case study had ``adjudicator.py`` as the intended
   structural problem: SIMPLE 0.0, cyclomatic complexity 34, and maximum
   function complexity 48. A close reproduction does not need the exact same
   scores, but it should have one obvious central file that Topos identifies as
   expensive to extend.

   **Feature prompts**

   Run the same four feature requests in the same order:

   1. Add coordination of benefits.
   2. Add provider network tier pricing.
   3. Add prior authorization denial and override workflow.
   4. Add audit export with redaction and tamper-evident hashes.

   Each feature prompt should require:

   * production code,
   * tests for the new behavior,
   * preservation of the existing tests,
   * no web search,
   * a short final summary of changed files and verification commands.

   **Condition A: no Topos**

   Copy the baseline into a clean directory. Ask the agent to implement the four
   features in sequence, starting each feature from the previous feature's
   output. Do not mention Topos in the feature prompts.

   Record for each feature:

   * wall time,
   * input tokens,
   * cached input tokens,
   * output tokens,
   * total tokens,
   * tool calls,
   * estimated cost,
   * final test result.
   * Topos coverage JSON.

   After each feature, rerun the same snapshot commands: pytest, line coverage,
   ``gitnexus analyze``, ``topos evaluate``, and ``topos coverage``.

   **Condition B: Topos-guided cleanup**

   Copy the same baseline into another clean directory. Before feature work,
   ask the agent to use Topos to identify the worst structural file, refactor it
   for simpler boundaries, and keep the tests passing.

   A useful cleanup prompt is:

   .. code-block:: text

      Use Topos to evaluate this package. Identify the worst structural
      bottleneck, refactor it into clearer modules, and verify that tests still
      pass. Preserve behavior. Prefer smaller rule modules over a larger
      orchestration function.

   After cleanup, run the same four feature prompts from Condition A. Record the
   same metrics and rerun the same Topos coverage snapshots.

   **Suggested result table**

   Compare feature work separately from cleanup work:

   .. list-table::
      :header-rows: 1
      :widths: 26 22 22 30

      * - Metric
        - A: unrefactored features
        - B: features after cleanup
        - Difference
      * - Wall time
        - ``<seconds>``
        - ``<seconds>``
        - ``<percent>``
      * - Total tokens
        - ``<tokens>``
        - ``<tokens>``
        - ``<percent>``
      * - Estimated cost
        - ``<$>``
        - ``<$>``
        - ``<$ and percent>``

   Then account for the cleanup separately:

   .. list-table::
      :header-rows: 1
      :widths: 30 30 40

      * - Scenario
        - Cost counted
        - Result
      * - One feature batch
        - Cleanup plus features
        - ``<B total>`` vs ``<A total>``
      * - Feature work only
        - Features after cleanup
        - ``<B feature cost>`` vs ``<A feature cost>``
      * - Break-even
        - Same observed saving rate
        - ``<cleanup cost / feature-batch savings>``

   **Cost estimate**

   Use the model provider's token report if available. Keep cached input,
   uncached input, and output tokens separate, because they may be priced
   differently. State the pricing assumptions next to the result; do not treat
   the estimate as an invoice.
