# Topos agent cost-savings experiment

## Goal

Measure whether using Topos to clean agent-written code before the next feature
reduces future agent cost:

- fewer input/output tokens,
- less wall time,
- fewer file reads and broad codebase scans,
- fewer files and lines changed,
- lower refactoring drag across follow-up features,
- equal or better test pass rate and structural quality.

The hypothesis is not just "Topos improves code quality." The hypothesis is:

> Cleaner code going in reduces paid agent archaeology later.

## Core claim to test

Agents can write code that passes tests but is expensive to extend. The next
agent session then has to rediscover boundaries, untangle functions, inspect
more files, and spend more context before making a safe change.

Topos should reduce that cost by pre-computing the structural signals the agent
would otherwise infer manually:

- cyclomatic complexity,
- module coupling,
- dangerous API reachability,
- structural / semantic test coverage.

## Experiment shape

Use paired A/B runs from the same generated starting codebase.

### Condition A: no Topos cleanup

1. Gemini writes an initial small codebase.
2. Tests pass.
3. Fresh Gemini session adds Feature 1.
4. Fresh Gemini session adds Feature 2.
5. Fresh Gemini session adds Feature 3.
6. Measure all costs and final quality.

### Condition B: Topos cleanup before follow-up work

1. Start from the exact same initial codebase as Condition A.
2. Gemini uses Topos to evaluate structure and clean the code.
3. Tests pass.
4. Fresh Gemini session adds Feature 1.
5. Fresh Gemini session adds Feature 2.
6. Fresh Gemini session adds Feature 3.
7. Measure all costs and final quality.

Measure both:

- **feature-only savings**: Feature 1-3 cost in A vs B.
- **amortized savings**: cleanup cost + Feature 1-3 cost in B vs Feature 1-3
  cost in A.

This distinction matters. Topos cleanup has an upfront cost. The business
question is how quickly that cost pays back across future agent sessions.

## Why use a synthetic repo first

Do not start on Topos itself. A synthetic fixture gives tighter controls:

- same starting code every run,
- same feature requests,
- easy pass/fail tests,
- intentionally extensible domain,
- clear structural failure modes.

After the protocol works, repeat on a real Topos module as external validation.

## Fixture: `order_rules`

Use a small Python package for order pricing. It is simple enough to inspect,
but feature additions punish bad structure quickly.

Target layout:

```text
order_rules/
  pyproject.toml
  order_rules/
    __init__.py
    cart.py
    discounts.py
    shipping.py
    taxes.py
    audit.py
  tests/
    test_pricing.py
    test_discounts.py
    test_shipping.py
```

Initial domain:

- carts with line items,
- item categories,
- coupon codes,
- percentage and fixed discounts,
- regional tax rates,
- shipping rules,
- audit messages explaining price adjustments.

Expected structural risks:

- one giant `price_order()` function,
- duplicated discount logic,
- shipping/tax rules mixed into cart state,
- category conditionals spread across files,
- high fan-out from a central module,
- tests that assert totals but do not cover rule structure.

## Agent prompts

Keep prompts stable across runs. Store them in files and pipe them to Gemini
rather than hand-typing them.

### Prompt 0: generate the starting codebase

```text
Create a small Python package named order_rules.

It should implement:
- carts with line items,
- item categories,
- coupon codes,
- percentage and fixed discounts,
- region-specific tax rates,
- shipping rules,
- an audit trail explaining every price adjustment.

Include pytest tests. Keep the implementation realistic, but do not overbuild.
When done, run the tests and fix failures.
```

Run this once per replicate. Commit the generated result as the common baseline
for both A and B.

### Prompt B0: Topos cleanup

Only used in Condition B.

```text
Use Topos to evaluate this package for structural quality.

Your goal is to make the code easier for a future agent to extend.
Prioritize:
1. SIMPLE
2. COMPOSABLE
3. SECURE

Refactor the worst structural issues Topos identifies. Preserve behavior.
Run tests after the refactor. Then run Topos again and report:
- before/after verdicts,
- the main files improved,
- what changed structurally.
```

### Prompt 1: feature 1

```text
Add support for subscription items.

Requirements:
- subscription items may be billed monthly or annually,
- annual subscriptions get a prorated first-month discount,
- normal item discounts must not apply to subscription setup fees,
- audit output must explain every subscription adjustment.

Run tests and add coverage for the new behavior.
```

### Prompt 2: feature 2

```text
Add bundle pricing.

Requirements:
- a bundle is a named group of SKUs,
- bundles can apply a fixed discount or percentage discount,
- bundle discounts stack after item-level discounts but before tax,
- audit output must explain which bundle matched and why.

Run tests and add coverage for the new behavior.
```

### Prompt 3: feature 3

```text
Add customer-tier rules.

Requirements:
- customers may be standard, silver, gold, or enterprise,
- each tier can affect discounts, shipping, and audit text,
- enterprise customers may override regional tax handling with an exemption flag,
- invalid tier configurations should raise a clear error.

Run tests and add coverage for the new behavior.
```

## Running Gemini

Use headless Gemini CLI so the experiment is scriptable. Gemini CLI supports
non-interactive prompts with `-p/--prompt` and structured output with
`--output-format json` or `--output-format stream-json`.

Recommended invocation shape:

```bash
gemini \
  --prompt "$(cat prompts/feature_1.txt)" \
  --output-format stream-json \
  --skip-trust \
  --approval-mode yolo \
  2>&1 | tee logs/A/run_01/feature_1.stream.jsonl
```

Use `--approval-mode yolo` only inside disposable experiment directories.

Use a fresh Gemini session for each feature step. Do not use `--resume`.

## Topos setup for Condition B

Configure Gemini MCP once in the experiment environment:

```bash
gemini mcp add topos uvx --from "topos-mcp[ect-coverage]" topos mcp
gemini mcp list
```

For CLI-side measurements after each step:

```bash
topos evaluate order_rules/ -r --json > metrics/topos-evaluate.json
topos coverage order_rules/ --tests tests/ --json > metrics/topos-coverage.json
```

If COMPOSABLE should be included, generate the dependency graph:

```bash
topos depgraph generate
topos evaluate order_rules/ -r --gitnexus-dir .gitnexus --json
```

## Replicates

Run at least five paired replicates:

```text
run_01/
  baseline/
  A_no_topos/
  B_with_topos/
run_02/
...
run_05/
```

Each replicate gets a newly generated baseline from Prompt 0. Within a
replicate, A and B must start from the same baseline commit.

Use the same model, same prompts, same approval mode, and same test commands
for A and B.

## Metrics

### Agent cost metrics

Collect per step:

- wall-clock seconds,
- input tokens,
- output tokens,
- total tokens,
- estimated USD cost,
- number of model turns,
- number of tool calls,
- number of file reads/searches,
- number of shell commands,
- number of edit operations.

If Gemini stream JSON exposes usage metadata, use it. If not, use fallback
estimates:

```text
estimated_input_tokens = input_chars / 4
estimated_output_tokens = output_chars / 4
```

Keep estimated and observed token fields separate.

### Engineering metrics

Collect per step:

- tests passed,
- test count,
- files changed,
- lines added,
- lines deleted,
- total churn,
- number of commits,
- final package import success,
- final feature acceptance checklist pass/fail.

### Topos metrics

Collect before and after every feature:

- aggregate verdict,
- worst-file verdict,
- medal distribution,
- average SIMPLE score,
- average COMPOSABLE score,
- average SECURE score,
- worst SIMPLE file,
- worst COMPOSABLE file,
- worst SECURE file,
- coverage score,
- topological coverage availability / score.

## Derived metrics

For each replicate:

```text
feature_token_savings =
  1 - (tokens_B_features_1_3 / tokens_A_features_1_3)

feature_time_savings =
  1 - (seconds_B_features_1_3 / seconds_A_features_1_3)

amortized_token_savings =
  1 - ((tokens_B_cleanup + tokens_B_features_1_3) / tokens_A_features_1_3)

amortized_time_savings =
  1 - ((seconds_B_cleanup + seconds_B_features_1_3) / seconds_A_features_1_3)

scan_reduction =
  1 - (file_reads_B_features_1_3 / file_reads_A_features_1_3)

churn_reduction =
  1 - (changed_lines_B_features_1_3 / changed_lines_A_features_1_3)
```

Report medians across replicates, not just averages. Also report min/max
because agent runs have high variance.

## Success criteria

The experiment supports the cost-savings claim if Condition B shows:

- lower median feature-only token use,
- lower median feature-only wall time,
- fewer broad file reads/searches,
- equal or better test pass rate,
- equal or better final Topos verdict,
- amortized savings by Feature 2 or Feature 3.

The experiment does not support the claim if Topos cleanup improves medals but
does not reduce future feature cost, or if the savings only appear because B
does less work or skips requirements.

## Guardrails against false wins

- Use identical feature prompts in A and B.
- Use fresh sessions for each feature.
- Compare against the same baseline inside each replicate.
- Require tests to pass in both conditions.
- Use an acceptance checklist for every feature.
- Count cleanup cost separately and in amortized totals.
- Keep all raw transcripts and stream logs.
- Do not manually fix one branch unless the same intervention is applied to the
  paired branch.

## Output report template

```text
# Topos agent cost-savings experiment

Runs: 5 paired replicates
Model: <model>
Date: <date>

## Result

Using Topos before follow-up features changed median feature work by:

- Tokens: <x% lower/higher>
- Wall time: <x% lower/higher>
- File reads/searches: <x% lower/higher>
- Churn: <x% lower/higher>
- Final Topos verdict: <A> -> <B>
- Tests: <A pass rate> vs <B pass rate>

Cleanup amortized by: Feature <n> / did not amortize within 3 features.

## Interpretation

<What this does and does not prove.>

## Raw data

<Link paths to logs and metrics JSON.>
```

## Optional second phase: real Topos module

After the synthetic fixture works, repeat the experiment on a contained Topos
module in a disposable worktree. Good candidates:

- `topos/cli/commands/coverage.py`
- `topos/mcp/tools/coverage.py`
- `topos/evaluation/policies/coverage.py`

Feature prompts should be small and realistic, such as adding one output field,
one validation rule, or one CLI flag. The real-code phase is less controlled
but more persuasive once the synthetic protocol is proven.
