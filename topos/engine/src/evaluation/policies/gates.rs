//! Canonical gate specs — the single structural source of truth for
//! pass/fail.
//!
//! [`crate::evaluation::policies::calibration`] owns the *numbers*; this
//! module owns the *structure*: which raw metric belongs to which
//! pillar, which side(s) of a band it is gated on, which exemptions
//! apply, what a failure means in prose, and which refactor operations
//! address it. Every consumer of a gate comparison — the `Φᵢ` scorers,
//! the suggestion engine, and MCP refactor targets — evaluates gates
//! through [`evaluate_gates`] so their verdicts can never diverge.
//!
//! Every registered spec gates its pillar. Each also carries the
//! desirability [`Curve`] its continuous score is read off, anchored so
//! `d = TAU` exactly at the gate threshold: `d ≥ TAU` ⇔
//! [`GateResult::passed`], so a pillar's score can never contradict its
//! verdict (see `docs/decisions/gate-anchored-scoring.md`). Advisory
//! metrics (`cfg.cyclomatic`, `mdg.instability`, `mdg.fan_in`, …) are not
//! gates and are not registered here; they are read relative to their
//! codebase by [`crate::evaluation::advisory`].

use std::collections::BTreeMap;

use super::calibration::{COMPOSABLE, NAVIGABLE, SECURE, SIMPLE};
use super::desirability::{d_band, d_lower_is_better, d_zero_tolerance, TAU};

/// How a metric fared against its gate band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    Pass,
    /// value < low bound
    FailLow,
    /// value > high bound
    FailHigh,
    /// below low, but the exemption predicate held
    ExemptLow,
    /// above high, but the exemption predicate held
    ExemptHigh,
}

impl GateOutcome {
    fn passing(self) -> bool {
        matches!(
            self,
            GateOutcome::Pass | GateOutcome::ExemptLow | GateOutcome::ExemptHigh
        )
    }

    fn low_side(self) -> bool {
        matches!(self, GateOutcome::FailLow | GateOutcome::ExemptLow)
    }
}

/// Everything an exemption predicate may read.
pub struct GateContext<'a> {
    pub value: f64,
    pub metrics: &'a BTreeMap<String, f64>,
    pub is_entrypoint_module: bool,
}

/// Shape of a gate's desirability `d ∈ [0, 1]`, anchored at the gate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Curve {
    /// `1` at `0`, `TAU` at `high`, `0` at `2·high` (see
    /// [`d_lower_is_better`]).
    LowerIsBetter,
    /// `1` at `ideal`, `TAU` at `low`/`high`, `0` at `0` and `1` (see
    /// [`d_band`]).
    Band { ideal: f64 },
    /// Gate at zero: `1` at `0`, `TAU·exp(−v/scale)` above (see
    /// [`d_zero_tolerance`]).
    ZeroTolerance { scale: f64 },
}

/// One raw-metric gate: band, pillar, exemption, remedy, and prose.
pub struct GateSpec {
    pub metric: &'static str,
    pub pillar: &'static str,
    /// Inclusive lower bound; `None` = unbounded below.
    pub low: Option<f64>,
    /// Inclusive upper bound; `None` = unbounded above.
    pub high: Option<f64>,
    pub granularity: &'static str,
    pub interpret: fn(f64, GateOutcome) -> String,
    pub exempt: Option<fn(&GateContext) -> bool>,
    pub operations_low: &'static [&'static str],
    pub operations_high: &'static [&'static str],
    pub curve: Curve,
}

/// A spec applied to a measured value.
pub struct GateResult {
    pub spec: &'static GateSpec,
    pub value: f64,
    pub outcome: GateOutcome,
}

impl GateResult {
    /// True for PASS and for exempted failures (the gate is satisfied).
    pub fn passed(&self) -> bool {
        self.outcome.passing()
    }

    /// The bound on the violated side, or `None` when in band.
    pub fn threshold(&self) -> Option<f64> {
        match self.outcome {
            GateOutcome::Pass => None,
            outcome if outcome.low_side() => self.spec.low,
            _ => self.spec.high,
        }
    }

    /// Refactor operations for the violated side (empty when in band).
    pub fn operations(&self) -> &'static [&'static str] {
        match self.outcome {
            GateOutcome::Pass => &[],
            outcome if outcome.low_side() => self.spec.operations_low,
            _ => self.spec.operations_high,
        }
    }

    pub fn interpretation(&self) -> String {
        (self.spec.interpret)(self.value, self.outcome)
    }

    /// Gate-anchored desirability: `d ≥ TAU` ⇔ [`Self::passed`]. An
    /// exempted failure is clamped up to `TAU` — the gate is satisfied,
    /// so its score must sit on the passing side.
    pub fn desirability(&self) -> f64 {
        let v = self.value;
        let d = match self.spec.curve {
            Curve::LowerIsBetter => d_lower_is_better(v, self.spec.high.unwrap_or(f64::INFINITY)),
            Curve::Band { ideal } => d_band(
                v,
                self.spec.low.unwrap_or(0.0),
                ideal,
                self.spec.high.unwrap_or(1.0),
            ),
            Curve::ZeroTolerance { scale } => d_zero_tolerance(v, scale),
        };
        if matches!(
            self.outcome,
            GateOutcome::ExemptLow | GateOutcome::ExemptHigh
        ) {
            d.max(TAU)
        } else {
            d
        }
    }
}

// --- Exemption predicates (the scorer carve-outs, expressed once) -------

/// Import/export-only entrypoint modules may sit below the entropy floor
/// (a short re-export list looks "repetitive") or above the ceiling (a
/// list of distinct crate paths/type names compresses poorly despite
/// having zero control flow to be "unstructured"). Either failure mode
/// is a false signal for a file this shape, so both sides are tolerated
/// -- `classify` has already determined which side failed by the time
/// this predicate runs.
fn entropy_entrypoint_exempt(ctx: &GateContext) -> bool {
    ctx.is_entrypoint_module
}

// --- Interpretation renderers (canonical prose) --------------------------

fn interpret_max_func(value: f64, outcome: GateOutcome) -> String {
    if outcome == GateOutcome::Pass {
        format!(
            "max function complexity ({value:.0}) within threshold (<= {})",
            SIMPLE.max_function_complexity
        )
    } else {
        format!(
            "max function complexity ({value:.0}) exceeds threshold (> {})",
            SIMPLE.max_function_complexity
        )
    }
}

fn interpret_entropy(value: f64, outcome: GateOutcome) -> String {
    match outcome {
        GateOutcome::Pass => format!(
            "entropy ({value:.2}) within structured range [{}, {}]",
            SIMPLE.min_entropy, SIMPLE.max_entropy
        ),
        GateOutcome::ExemptLow => format!(
            "entropy ({value:.2}) is low, but tolerated for import/export-only entrypoint modules"
        ),
        GateOutcome::ExemptHigh => format!(
            "entropy ({value:.2}) is high, but tolerated for import/export-only entrypoint modules"
        ),
        GateOutcome::FailLow => {
            format!("entropy ({value:.2}) is too low; code may be repetitive or trivial")
        }
        GateOutcome::FailHigh => {
            format!("entropy ({value:.2}) is too high; code may be unstructured")
        }
    }
}

fn interpret_fan_out(value: f64, outcome: GateOutcome) -> String {
    if outcome == GateOutcome::Pass {
        format!(
            "fan-out ({value:.0}) within threshold (<= {})",
            COMPOSABLE.max_fan_out
        )
    } else {
        format!(
            "fan-out ({value:.0}) exceeds threshold (> {})",
            COMPOSABLE.max_fan_out
        )
    }
}

fn interpret_danger(value: f64, outcome: GateOutcome) -> String {
    if outcome == GateOutcome::Pass {
        format!(
            "no reachable dangerous-API calls ({value:.0} <= {})",
            SECURE.max_dangerous_calls
        )
    } else {
        format!(
            "{} dangerous-API call site(s) exceeds threshold ({})",
            value as i64, SECURE.max_dangerous_calls
        )
    }
}

fn interpret_taint(value: f64, outcome: GateOutcome) -> String {
    if outcome == GateOutcome::Pass {
        format!(
            "no source→sink taint paths ({value:.0} <= {})",
            SECURE.max_taint_flows
        )
    } else {
        format!(
            "{} taint flow path(s) exceeds threshold ({})",
            value as i64, SECURE.max_taint_flows
        )
    }
}

fn interpret_divergence(value: f64, outcome: GateOutcome) -> String {
    if outcome == GateOutcome::Pass {
        format!(
            "worst-function nesting divergence ({value:.1}) within threshold (<= {})",
            NAVIGABLE.max_function_divergence
        )
    } else {
        format!(
            "worst-function nesting divergence ({value:.1}) exceeds threshold (> {}); \
             flatten the deepest nested block",
            NAVIGABLE.max_function_divergence
        )
    }
}

// --- The registry ---------------------------------------------------------
// Ordered to match the scorers' interpretation insertion order.

pub static GATE_SPECS: &[GateSpec] = &[
    GateSpec {
        metric: "ast.entropy",
        pillar: "simple",
        low: Some(SIMPLE.min_entropy),
        high: Some(SIMPLE.max_entropy),
        granularity: "module",
        interpret: interpret_entropy,
        exempt: Some(entropy_entrypoint_exempt),
        operations_low: &["consolidate_boilerplate"],
        operations_high: &["decompose_dense_logic"],
        curve: Curve::Band {
            ideal: SIMPLE.entropy_ideal,
        },
    },
    GateSpec {
        metric: "ast.max_function_complexity",
        pillar: "simple",
        low: None,
        high: Some(SIMPLE.max_function_complexity),
        granularity: "function",
        interpret: interpret_max_func,
        exempt: None,
        operations_low: &[],
        operations_high: &["extract_helper", "split_decision_logic"],
        curve: Curve::LowerIsBetter,
    },
    GateSpec {
        metric: "mdg.fan_out",
        pillar: "composable",
        low: None,
        high: Some(COMPOSABLE.max_fan_out),
        granularity: "file",
        interpret: interpret_fan_out,
        exempt: None,
        operations_low: &[],
        operations_high: &["reduce_fanout", "invert_dependency"],
        // File-level COMPOSABLE is deliberately narrower than Martin's
        // package metrics: it asks how much external behavior this file must
        // coordinate. Distinct external callees are a local response/outward
        // coupling measure with class/module-level precedent (Chidamber &
        // Kemerer 1994; Henry & Kafura 1981). The numeric cap is calibrated
        // empirically by Topos, not claimed as a literature constant.
        curve: Curve::LowerIsBetter,
    },
    GateSpec {
        metric: "cpg.dangerous_calls",
        pillar: "secure",
        low: None,
        high: Some(SECURE.max_dangerous_calls),
        granularity: "module",
        interpret: interpret_danger,
        exempt: None,
        operations_low: &[],
        operations_high: &[],
        curve: Curve::ZeroTolerance {
            scale: SECURE.danger_scale,
        },
    },
    GateSpec {
        metric: "cpg.taint_flows",
        pillar: "secure",
        low: None,
        high: Some(SECURE.max_taint_flows),
        granularity: "module",
        interpret: interpret_taint,
        exempt: None,
        operations_low: &[],
        operations_high: &[],
        curve: Curve::ZeroTolerance {
            scale: SECURE.taint_scale,
        },
    },
    GateSpec {
        metric: "nav.max_function_divergence",
        pillar: "navigable",
        low: None,
        high: Some(NAVIGABLE.max_function_divergence),
        granularity: "function",
        interpret: interpret_divergence,
        exempt: None,
        operations_low: &[],
        // Same fix as a complexity failure — pull the nested block out
        // into its own function — so it reuses the same operation
        // vocabulary rather than inventing a NAVIGABLE-only verb.
        operations_high: &["extract_helper", "split_decision_logic"],
        curve: Curve::LowerIsBetter,
    },
];

fn gate_for_metric(metric: &str) -> Option<&'static GateSpec> {
    GATE_SPECS.iter().find(|spec| spec.metric == metric)
}

/// Desirability of a measured value on a registered gate, with no
/// exemption clamp. `None` when `metric` is not a gate.
pub fn metric_desirability(metric: &str, value: f64) -> Option<f64> {
    let spec = gate_for_metric(metric)?;
    let d = match spec.curve {
        Curve::LowerIsBetter => d_lower_is_better(value, spec.high.unwrap_or(f64::INFINITY)),
        Curve::Band { ideal } => d_band(
            value,
            spec.low.unwrap_or(0.0),
            ideal,
            spec.high.unwrap_or(1.0),
        ),
        Curve::ZeroTolerance { scale } => d_zero_tolerance(value, scale),
    };
    Some(d)
}

/// The failing gate that holds `pillar` at its gate score: the minimum
/// desirability among failures on that pillar. `None` when every measured
/// gate on the pillar passed.
pub fn binding_failure<'a>(gates: &'a [GateResult], pillar: &str) -> Option<&'a GateResult> {
    gates
        .iter()
        .filter(|result| result.spec.pillar == pillar && !result.passed())
        .min_by(|a, b| a.desirability().total_cmp(&b.desirability()))
}

/// Metric-key namespacing shared with the agent-contract/pillar layers.
pub const PILLAR_METRIC_PREFIXES: &[(&str, &[&str])] = &[
    ("simple", &["cfg.", "ast."]),
    ("composable", &["mdg."]),
    ("secure", &["cpg."]),
    ("navigable", &["nav."]),
];

/// Map a namespaced raw-metric key to its pillar (default `"simple"`).
pub fn pillar_for_metric(metric: &str) -> &'static str {
    for (pillar, prefixes) in PILLAR_METRIC_PREFIXES {
        if prefixes.iter().any(|prefix| metric.starts_with(prefix)) {
            return pillar;
        }
    }
    "simple"
}

/// Apply every (optionally pillar-filtered) spec whose metric is present.
pub fn evaluate_gates(
    metrics: &BTreeMap<String, f64>,
    pillar: Option<&str>,
    is_entrypoint_module: bool,
) -> Vec<GateResult> {
    GATE_SPECS
        .iter()
        .filter(|spec| pillar.is_none_or(|p| spec.pillar == p))
        .filter_map(|spec| {
            let value = *metrics.get(spec.metric)?;
            let outcome = classify(spec, value, metrics, is_entrypoint_module);
            Some(GateResult {
                spec,
                value,
                outcome,
            })
        })
        .collect()
}

/// Canonical prose for a single metric value (no exemption context).
pub fn interpret_metric(metric: &str, value: f64) -> String {
    let spec = gate_for_metric(metric).expect("metric must have a registered GateSpec");
    let empty = BTreeMap::new();
    let outcome = classify(spec, value, &empty, false);
    (spec.interpret)(value, outcome)
}

fn classify(
    spec: &GateSpec,
    value: f64,
    metrics: &BTreeMap<String, f64>,
    is_entrypoint_module: bool,
) -> GateOutcome {
    // A NaN metric must fail closed: `NaN < low` and `NaN > high` are both
    // false, so without this guard NaN would silently `Pass` every gate —
    // including the zero-tolerance SECURE gates. Report it as an
    // out-of-band failure on whichever side is bounded.
    if value.is_nan() {
        return if spec.high.is_some() {
            GateOutcome::FailHigh
        } else {
            GateOutcome::FailLow
        };
    }
    let (fail, exempt) = if spec.low.is_some_and(|low| value < low) {
        (GateOutcome::FailLow, GateOutcome::ExemptLow)
    } else if spec.high.is_some_and(|high| value > high) {
        (GateOutcome::FailHigh, GateOutcome::ExemptHigh)
    } else {
        return GateOutcome::Pass;
    };
    let ctx = GateContext {
        value,
        metrics,
        is_entrypoint_module,
    };
    match spec.exempt {
        Some(predicate) if predicate(&ctx) => exempt,
        _ => fail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_function_complexity_over_threshold_fails_with_extract_helper_operation() {
        let metrics = BTreeMap::from([("ast.max_function_complexity".to_string(), 20.0)]);
        let results = evaluate_gates(&metrics, Some("simple"), false);
        assert_eq!(results.len(), 1);
        assert!(!results[0].passed());
        assert!(results[0].operations().contains(&"extract_helper"));
    }

    #[test]
    fn advisory_metrics_are_not_gates() {
        let metrics = BTreeMap::from([
            ("cfg.cyclomatic".to_string(), 99.0),
            ("mdg.instability".to_string(), 1.0),
            ("mdg.fan_in".to_string(), 99.0),
        ]);
        assert!(evaluate_gates(&metrics, None, false).is_empty());
    }

    #[test]
    fn entropy_low_is_exempt_for_entrypoint_modules() {
        let metrics = BTreeMap::from([("ast.entropy".to_string(), 0.05)]);
        let results = evaluate_gates(&metrics, Some("simple"), true);
        let entropy = results
            .iter()
            .find(|r| r.spec.metric == "ast.entropy")
            .unwrap();
        assert!(entropy.passed());
        assert_eq!(entropy.outcome, GateOutcome::ExemptLow);
    }

    #[test]
    fn entropy_low_fails_for_ordinary_modules() {
        let metrics = BTreeMap::from([("ast.entropy".to_string(), 0.05)]);
        let results = evaluate_gates(&metrics, Some("simple"), false);
        let entropy = results
            .iter()
            .find(|r| r.spec.metric == "ast.entropy")
            .unwrap();
        assert!(!entropy.passed());
    }

    #[test]
    fn entropy_high_is_exempt_for_entrypoint_modules() {
        let metrics = BTreeMap::from([("ast.entropy".to_string(), 0.95)]);
        let results = evaluate_gates(&metrics, Some("simple"), true);
        let entropy = results
            .iter()
            .find(|r| r.spec.metric == "ast.entropy")
            .unwrap();
        assert!(entropy.passed());
        assert_eq!(entropy.outcome, GateOutcome::ExemptHigh);
    }

    #[test]
    fn binding_failure_is_the_lowest_desirability_on_the_pillar() {
        let metrics = BTreeMap::from([
            ("ast.max_function_complexity".to_string(), 20.0),
            ("ast.entropy".to_string(), 0.9),
        ]);
        let results = evaluate_gates(&metrics, Some("simple"), false);
        let binding = binding_failure(&results, "simple").unwrap();
        assert_eq!(binding.spec.metric, "ast.max_function_complexity");
        assert!(binding.desirability() < binding_failure_other(&results));
    }

    fn binding_failure_other(results: &[GateResult]) -> f64 {
        results
            .iter()
            .find(|result| result.spec.metric == "ast.entropy")
            .unwrap()
            .desirability()
    }

    /// The anchoring invariant: for every registered gate, over a grid of
    /// values spanning both sides of every bound (plus `NaN`), in both
    /// exemption contexts, `d ≥ TAU` exactly when the gate passed.
    #[test]
    fn desirability_crosses_tau_exactly_at_the_gate() {
        for spec in GATE_SPECS {
            let mut grid: Vec<f64> = (-4..=400).map(|i| f64::from(i) * 0.05).collect();
            for bound in [spec.low, spec.high].into_iter().flatten() {
                grid.extend([bound, bound - 1e-9, bound + 1e-9, 2.0 * bound]);
            }
            grid.extend([f64::NAN, f64::INFINITY, 1e9]);
            for value in grid {
                for entrypoint in [false, true] {
                    let metrics = BTreeMap::from([(spec.metric.to_string(), value)]);
                    let results = evaluate_gates(&metrics, Some(spec.pillar), entrypoint);
                    let r = results
                        .iter()
                        .find(|r| r.spec.metric == spec.metric)
                        .unwrap();
                    let d = r.desirability();
                    assert!((0.0..=1.0).contains(&d), "{} d({value}) = {d}", spec.metric);
                    assert_eq!(
                        d >= TAU,
                        r.passed(),
                        "{} value={value} entrypoint={entrypoint} d={d} outcome={:?}",
                        spec.metric,
                        r.outcome
                    );
                }
            }
        }
    }

    #[test]
    fn pillar_for_metric_matches_prefix() {
        assert_eq!(pillar_for_metric("cfg.cyclomatic"), "simple");
        assert_eq!(pillar_for_metric("mdg.fan_in"), "composable");
        assert_eq!(pillar_for_metric("cpg.taint_flows"), "secure");
    }
}
