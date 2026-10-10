# Gate-anchored pillar scores

Status: **ACCEPTED**. Supersedes the score-normalization parts of
[`file-level-composable.md`](file-level-composable.md) and
[`composable-instability-resolution.md`](composable-instability-resolution.md)
(their gate decisions stand). Verdicts and medals are unchanged.

Full derivation and proofs: [`docs/methods/gate-anchored-scoring.tex`](../methods/gate-anchored-scoring.tex).
Operator procedure for the one-time leaderboard rerun:
[`docs/calibration/leaderboard-rerun.md`](../calibration/leaderboard-rerun.md).

## Problem

Each pillar reports a binary verdict (the four verdicts form the 16-element
Heyting-algebra lattice) and a continuous 0–100 score. The score was the `min`
over linear "quality" curves whose caps were unrelated to the gates, and the
`min` included advisory metrics. Score and verdict therefore disagreed:

- **4,757 of 30,686** leaderboard files pass SIMPLE but score below 50%.
- **302 of 1,061** freshly scored files pass COMPOSABLE but score below 50%.

COMPOSABLE was worse than miscalibrated. Its fixed instability band
`[0.3, 0.7]` (issue #351) scored files at `I = 0` or `I = 1` as 0%. Main-sequence
distance only activated when abstractness `A > 0` (62 of 1,061 files). 41% of
files have `Ca + Ce < 2` and got a fabricated `I = 0.5`, and the in-band rate
swings with the denominator (31/44/16/46% for `n = 2..5`). The leaderboard
calibration report's "binary distribution for modularity", which justified the
0.80 COMPOSABLE floor, was this artifact.

## Decision

Four layers. `τ = 0.5` throughout.

### 1. Gate-anchored desirability

Every *gated* metric maps to `d ∈ [0, 1]` with `d = τ` exactly at its global
gate `g` (Derringer & Suich desirability):

| kind | metrics (gate) | `d(v)` |
| --- | --- | --- |
| lower is better | `ast.max_function_complexity` (10), `mdg.fan_out` (10), `nav.max_function_divergence` (10) | `1 − ½·v/g` on `[0, g]`; `½·(2g − v)/g` on `(g, 2g)`; `0` at `≥ 2g` |
| band | `ast.entropy` (`[0.2, 0.8]`, ideal 0.5) | piecewise linear: `1` at 0.5, `½` at 0.2 and 0.8, `0` at 0 and 1 |
| zero tolerance | `cpg.dangerous_calls`, `cpg.taint_flows` (0) | `1` at 0; `½·exp(−v/3)` otherwise |

Exemptions (for example the entropy entrypoint exemption) clamp `d ≥ τ`.
Invariant: **`d ≥ τ ⇔ the gate passes`**.

### 2. Gates combine with `min`

`G = min_i d_i` over a pillar's gates; the pillar passes iff `G ≥ τ`. `min`
(the Gödel t-norm) is the only t-norm whose α-cut equals the conjunction of
α-cuts for every α. Product or geometric mean break it: two gates at `0.6` each
pass, but `0.6 · 0.6 = 0.36` would read as a failure.

### 3. Codebase-relative advisory metrics

Advisory metrics no longer have fixed caps. Each is read as a percentile of the
codebase it lives in, shrunk toward a per-language prior:

```text
F̃(x) = w·F_local(x) + (1 − w)·F_lang(x),    w = n / (n + k)
```

- `F_local`, `F_lang` are mid-rank ECDFs, `(#{x_j < x} + ½·#{x_j = x}) / n`,
  which handle the heavy ties (46–88% zeros for divergence, 52–63% for the fan
  metrics).
- `F_lang` is a per-language prior table computed once from leaderboard runs.
  `k = σ²_within / τ²_between` of `log1p(x)` by package (method of moments): the
  normal–normal shrinkage ratio. `F̃` is also the Dirichlet-process posterior-
  mean CDF with base measure `F_lang` and concentration `k`.
- Estimated `k`: max function complexity 3.7, cyclomatic 3.8, nesting 3.8,
  essential 3.1, longest path 3.9, entropy 4.6, divergence 6.4. A 10-file repo
  gets about 70% local weight; a 50-file repo about 93%.

| pillar | advisory metrics | direction |
| --- | --- | --- |
| SIMPLE | `cfg.cyclomatic`, `cfg.nesting_depth`, `cfg.essential` | higher is worse |
| COMPOSABLE | `mdg.fan_in` | higher is worse |
| COMPOSABLE | `mdg.instability` (skipped when `Ca + Ce < 2`) | two-sided |

Quality `a = 1 − F̃_<` (higher-worse, using the strict lower CDF so the best observed value scores 1 even when most files tie there) or `1 − 2·|F̃ − ½|` (two-sided). The pillar
advisory score `A` is the geometric mean of its `a` values (`1` when none
apply). A reading is **flagged** as an advisory suggestion when `F̃ ≥ 0.9` and
the value exceeds the prior median (two-sided: `|F̃ − ½| ≥ 0.45`). The global
percentile `F_lang(x)` is always reported beside the relative one so a
uniformly weak codebase stays visible.

The population pass runs in `topos evaluate -r` and MCP
`topos_evaluate_project`. Single-file calls have no population (`n = 0`) and
use the language prior alone. PR recap keeps prior-only scores so before/after
deltas stay comparable.

`mdg.main_sequence_distance` and the instability band leave scoring entirely;
both remain visible in `inspect` as diagnostics.

### 4. Banded pillar score

```text
S = τ + (G − τ)·A^{w_A}   if G ≥ τ
S = G·A^{w_A}             otherwise,        w_A = 1/3
```

- `A = 1` gives `S = G` (identity).
- `S` never crosses `τ`: **pass ⇔ `S ≥ 50`**.
- `S` is monotone in both `G` and `A`.

This is the ELECTRE veto-plus-concordance pattern (Roy 1991): the gates veto,
the advisories rank within the half the gates chose.

### Continuous lattice

The score vector lives in `[0, 1]^4`, a product of Gödel chains, which is a
prelinear Heyting (Gödel) algebra. The existing 16-element lattice is its τ-cut
`p_k = [S_k ≥ τ]`. The cut preserves `∧` and `∨`, so a medal's continuous score
is the `min` of its member pillar scores. It does **not** preserve Gödel
implication (for `y < x < τ`, `x → y = y < τ` but `[x ≥ τ] → [y ≥ τ] = 1`), so
implication and negation stay on the crisp generators.

## Consequences

- No passing file can score below 50, and no failing file at or above 50. The
  "achieved but 0%" explanations in the metrics docs are retired.
- Scores move: most passing files move up (they were dragged down by advisory
  caps), some move down within their half where the codebase-relative advisories
  are atypical.
- New `advisories` field on each result: per metric, `value`,
  `relative_percentile`, `global_percentile`, `local_weight`, `quality`,
  `flagged`. `gate_scores` carries `G` per pillar. `language` keys the prior.
- Advisory wording is "atypical vs. top OSS", never "buggy": percentiles measure
  typicality, not defectiveness (Lavazza & Morasca 2016).
- Normalized score floors (`score_floor`) and advisory caps are retired.
- One medal change, from the SECURE reporting work rather than the scoring:
  the allowlist used to partition only the first 20 findings, so a file whose
  only acknowledged risk sat past position 20 escaped the grade cap and could
  keep IDEAL. The partition now covers every finding, so the cap always fires
  when an acknowledged risk is what makes a file IDEAL. It requires a raw
  SECURE failure fully covered by the allowlist, all other pillars passing,
  more than 20 scanner findings (mostly Sighthound rules the CPG gate does not
  count), and the allowlisted one ordered after the twentieth.
- `w_A = 1/3` is the one judgment-call constant. It bounds advisory influence:
  for a passing pillar, `S ∈ [τ + (G − τ)·ε^{1/3}, G]` with `ε = 10⁻³`, so a
  passing pillar keeps at least 10% of its margin.

## Evidence

Prototype over the leaderboard corpus:

| check | before | after |
| --- | ---: | ---: |
| SIMPLE pass with score < 50 (30,686 files) | 4,757 | 8 (exemption cases) |
| COMPOSABLE pass with score < 50 (1,061 files) | 302 | 0 |

Advisory shift `S − G` on SIMPLE: p05 −13.6, p50 0.0, p95 +5.1 points.

Variance decomposition of `log1p` metrics over 165 packages and 6 languages:
language 4–10%, project-within-language 7–11%, file-within-project 81–90%.
Size correlates weakly with project means after language (−0.03 to −0.29), so
no size stratum is needed. Ecosystem p90 of max function complexity: C++ 22,
PyPI 19, Cargo 8, npm 5 — language priors are needed, project shrinkage is
enough beyond that.

95% of NAVIGABLE failures (2,266 of 2,376) also fail SIMPLE's max-function-
complexity gate. Whether the two generators are independent is a separate
question, out of scope here.

## Calibration

| item | how it is set |
| --- | --- |
| gate thresholds | unchanged; re-derived only when a metric definition changes |
| `τ = 0.5`, `2g` zero point | fixed by construction |
| `F_lang` tables and `k` | derived once per leaderboard run by `scripts/derive_scoring_priors.py` into `topos/engine/src/evaluation/advisory_priors.json` (`--check` verifies the committed file) |
| COMPOSABLE advisory priors | provisional until the leaderboard rerun |
| score floors, advisory caps | retired |
| medals | unchanged |

Optional: a one-time bootstrap confidence interval per gate percentile
(Foucault et al. 2014).

The v0.5.0 leaderboard used NAVIGABLE gate 6 (now 10) and has no COMPOSABLE
metrics for C++, Go, or MCP, so a one-time rerun is required; see
[`leaderboard-rerun.md`](../calibration/leaderboard-rerun.md).

## Issue mapping

- **#351** (fixed instability band) — closed. The band is removed from scoring;
  instability is a two-sided codebase-relative advisory, skipped when
  `Ca + Ce < 2`.
- **#349** — the score/verdict contradiction is resolved by construction. The
  GOLD-tier cyclomatic requirement it also raises remains a separate decision.
- **#342** — unblocked: its advisories land as codebase-relative readings.

## References

- Derringer & Suich (1980), J. Quality Technology 12(4):214–219. <https://doi.org/10.1080/00224065.1980.11980968>
- Hájek (1998), *Metamathematics of Fuzzy Logic*. <https://doi.org/10.1007/978-94-011-5300-3>
- Roy (1991), Theory and Decision. <https://doi.org/10.1007/BF00134132>
- Yager (1988), IEEE TSMC. <https://doi.org/10.1109/21.87068>
- Efron & Morris (1975), JASA 70(350):311–319.
- Ferguson (1973), Ann. Statist. 1(2):209–230.
- Hyndman & Fan (1996), Am. Stat. 50(4):361–365.
- Alves, Ypma & Visser (2010), ICSM. <https://doi.org/10.1109/ICSM.2010.5609747>
- Alves, Correia & Visser (2011), IWSM-Mensura. <https://doi.org/10.1109/IWSM-MENSURA.2011.15>
- Oliveira, Valente & Lima (2014), CSMR-WCRE. <https://doi.org/10.1109/CSMR-WCRE.2014.6747177>
- Foucault, Palyart, Falleri & Blanc (2014), SAC. <https://doi.org/10.1145/2554850.2554997>
- Zhang, Mockus, Zou, Khomh & Hassan (2013), ICSM. <https://doi.org/10.1109/ICSM.2013.46>
- Ernst (2018), MSR. <https://doi.org/10.1145/3196398.3196443>
- Mori et al. (2018), TechDebt. <https://doi.org/10.1145/3194164.3194173>
- Gil & Lalouche (2016), JOT. <https://doi.org/10.5381/jot.2016.15.1.a2>
- El Emam et al. (2001), TSE. <https://doi.org/10.1109/32.935855>
- Lavazza & Morasca (2016), PROMISE. <https://doi.org/10.1145/2972958.2972965>
- Concas et al. (2007), TSE. <https://doi.org/10.1109/TSE.2007.1019>
- Ferreira et al. (2012), JSS. <https://doi.org/10.1016/j.jss.2011.05.044>
- Mordal et al. (2013), JSEP. <https://doi.org/10.1002/smr.1558>
- Wagner et al. (2015), IST. <https://doi.org/10.1016/j.infsof.2015.02.009>
- Iglewicz & Hoaglin (1993), *How to Detect and Handle Outliers* (robust z; via the NIST handbook).
