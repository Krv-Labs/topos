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
//! Score *shaping* (normalization caps, quality curves) deliberately
//! stays in the scorers; only the decisive pass/fail comparisons live
//! here.

use std::collections::HashMap;

use super::calibration::{COMPOSABLE, SECURE, SIMPLE};

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
    pub metrics: &'a HashMap<String, f64>,
    pub is_entrypoint_module: bool,
    pub is_stable_leaf_module: bool,
    /// Raw Martin instability, threaded separately from `metrics` because
    /// `mdg.instability` is deliberately absent from the gated metrics map
    /// whenever `mdg.main_sequence_distance` is active (see
    /// `evaluation::policies::composable::score_coupling`) -- re-adding it
    /// under its own key would re-trigger the (now-superseded)
    /// `mdg.instability` GateSpec for the same file.
    pub instability: Option<f64>,
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
}

// --- Exemption predicates (the scorer carve-outs, expressed once) -------

/// Import/export-only entrypoint modules may sit below the entropy floor.
fn entropy_entrypoint_exempt(ctx: &GateContext) -> bool {
    ctx.is_entrypoint_module && ctx.value < SIMPLE.min_entropy
}

/// Entrypoint modules with zero fan-in may sit at maximal instability.
///
/// `metrics.get(...) == Some(0.0)` deliberately fails when fan-in is
/// unmeasured: an absent metric never grants the exemption (mirrors the
/// Python original's `fan_in == 0.0` against a possibly-`None` argument).
fn instability_entrypoint_exempt(ctx: &GateContext) -> bool {
    ctx.is_entrypoint_module
        && ctx.value >= COMPOSABLE.entrypoint_instability_min
        && ctx.metrics.get("mdg.fan_in") == Some(&0.0)
}

/// Frozen, declarations-only leaf modules may sit at maximal distance from
/// the main sequence -- Martin's accepted "Zone of Pain" exception for
/// foundation/utility code (constants, error types) that is stable *and*
/// concrete by design, not because it's poorly layered.
fn distance_stable_leaf_exempt(ctx: &GateContext) -> bool {
    ctx.is_stable_leaf_module
        && ctx
            .instability
            .is_some_and(|i| i <= COMPOSABLE.stable_leaf_instability_max)
}

// --- Interpretation renderers (canonical prose) --------------------------

fn interpret_cyclomatic(value: f64, outcome: GateOutcome) -> String {
    if outcome == GateOutcome::Pass {
        format!(
            "cyclomatic complexity ({value:.0}) within threshold (<= {})",
            SIMPLE.max_cyclomatic
        )
    } else {
        format!(
            "cyclomatic complexity ({value:.0}) exceeds threshold (> {})",
            SIMPLE.max_cyclomatic
        )
    }
}

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
        GateOutcome::FailLow => {
            format!("entropy ({value:.2}) is too low; code may be repetitive or trivial")
        }
        _ => format!("entropy ({value:.2}) is too high; code may be unstructured"),
    }
}

fn interpret_instability(value: f64, outcome: GateOutcome) -> String {
    let (low, high) = (COMPOSABLE.instability_low, COMPOSABLE.instability_high);
    match outcome {
        GateOutcome::Pass => {
            format!("instability ({value:.2}) within balanced range [{low}, {high}]")
        }
        GateOutcome::FailLow => format!("instability ({value:.2}) is too low (module is too stable)"),
        GateOutcome::ExemptHigh => format!(
            "instability ({value:.2}) is high, but tolerated for import/export-only entrypoint modules"
        ),
        _ => format!("instability ({value:.2}) is too high (module depends on too many things)"),
    }
}

fn interpret_main_sequence_distance(value: f64, outcome: GateOutcome) -> String {
    let max_d = COMPOSABLE.main_sequence_distance_max;
    match outcome {
        GateOutcome::Pass => format!(
            "main-sequence distance ({value:.2}) within tolerance (<= {max_d}) -- \
             instability and abstractness are balanced"
        ),
        GateOutcome::ExemptHigh => format!(
            "main-sequence distance ({value:.2}) is high, but tolerated for frozen, \
             declarations-only leaf modules"
        ),
        _ => format!(
            "main-sequence distance ({value:.2}) exceeds threshold (> {max_d}) -- module \
             is too concrete-and-stable (rigid) or too abstract-and-unstable \
             (speculative) for its role"
        ),
    }
}

fn interpret_fan_in(value: f64, outcome: GateOutcome) -> String {
    if outcome == GateOutcome::Pass {
        format!(
            "fan-in ({value:.0}) within threshold (<= {})",
            COMPOSABLE.max_fan_in
        )
    } else {
        format!(
            "fan-in ({value:.0}) exceeds threshold (> {})",
            COMPOSABLE.max_fan_in
        )
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

// --- The registry ---------------------------------------------------------
// Ordered to match the scorers' interpretation insertion order.

pub static GATE_SPECS: &[GateSpec] = &[
    GateSpec {
        metric: "cfg.cyclomatic",
        pillar: "simple",
        low: None,
        high: Some(SIMPLE.max_cyclomatic),
        granularity: "function",
        interpret: interpret_cyclomatic,
        exempt: None,
        operations_low: &[],
        operations_high: &["extract_helper", "split_decision_logic"],
    },
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
    },
    GateSpec {
        metric: "mdg.instability",
        pillar: "composable",
        low: Some(COMPOSABLE.instability_low),
        high: Some(COMPOSABLE.instability_high),
        granularity: "module",
        interpret: interpret_instability,
        exempt: Some(instability_entrypoint_exempt),
        operations_low: &["rebalance_dependencies", "extract_boundary"],
        operations_high: &["rebalance_dependencies", "extract_boundary"],
    },
    GateSpec {
        metric: "mdg.main_sequence_distance",
        pillar: "composable",
        low: None,
        high: Some(COMPOSABLE.main_sequence_distance_max),
        granularity: "module",
        interpret: interpret_main_sequence_distance,
        exempt: Some(distance_stable_leaf_exempt),
        operations_low: &[],
        operations_high: &["rebalance_dependencies", "extract_boundary"],
    },
    GateSpec {
        metric: "mdg.fan_in",
        pillar: "composable",
        low: None,
        high: Some(COMPOSABLE.max_fan_in),
        granularity: "module",
        interpret: interpret_fan_in,
        exempt: None,
        operations_low: &[],
        operations_high: &["split_module"],
    },
    GateSpec {
        metric: "mdg.fan_out",
        pillar: "composable",
        low: None,
        high: Some(COMPOSABLE.max_fan_out),
        granularity: "module",
        interpret: interpret_fan_out,
        exempt: None,
        operations_low: &[],
        operations_high: &["reduce_fanout", "invert_dependency"],
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
    },
];

fn gate_for_metric(metric: &str) -> Option<&'static GateSpec> {
    GATE_SPECS.iter().find(|spec| spec.metric == metric)
}

/// Metric-key namespacing shared with the agent-contract/pillar layers.
const PILLAR_METRIC_PREFIXES: &[(&str, &[&str])] = &[
    ("simple", &["cfg.", "ast."]),
    ("composable", &["mdg."]),
    ("secure", &["cpg."]),
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
    metrics: &HashMap<String, f64>,
    pillar: Option<&str>,
    is_entrypoint_module: bool,
    is_stable_leaf_module: bool,
    instability: Option<f64>,
) -> Vec<GateResult> {
    GATE_SPECS
        .iter()
        .filter(|spec| pillar.is_none_or(|p| spec.pillar == p))
        .filter_map(|spec| {
            let value = *metrics.get(spec.metric)?;
            let outcome = classify(
                spec,
                value,
                metrics,
                is_entrypoint_module,
                is_stable_leaf_module,
                instability,
            );
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
    let empty = HashMap::new();
    let outcome = classify(spec, value, &empty, false, false, None);
    (spec.interpret)(value, outcome)
}

fn classify(
    spec: &GateSpec,
    value: f64,
    metrics: &HashMap<String, f64>,
    is_entrypoint_module: bool,
    is_stable_leaf_module: bool,
    instability: Option<f64>,
) -> GateOutcome {
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
        is_stable_leaf_module,
        instability,
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
    fn cyclomatic_within_threshold_passes() {
        let metrics = HashMap::from([("cfg.cyclomatic".to_string(), 5.0)]);
        let results = evaluate_gates(&metrics, Some("simple"), false, false, None);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed());
    }

    #[test]
    fn cyclomatic_over_threshold_fails_with_extract_helper_operation() {
        let metrics = HashMap::from([("cfg.cyclomatic".to_string(), 20.0)]);
        let results = evaluate_gates(&metrics, Some("simple"), false, false, None);
        assert!(!results[0].passed());
        assert!(results[0].operations().contains(&"extract_helper"));
    }

    #[test]
    fn entropy_low_is_exempt_for_entrypoint_modules() {
        let metrics = HashMap::from([("ast.entropy".to_string(), 0.05)]);
        let results = evaluate_gates(&metrics, Some("simple"), true, false, None);
        let entropy = results
            .iter()
            .find(|r| r.spec.metric == "ast.entropy")
            .unwrap();
        assert!(entropy.passed());
        assert_eq!(entropy.outcome, GateOutcome::ExemptLow);
    }

    #[test]
    fn entropy_low_fails_for_ordinary_modules() {
        let metrics = HashMap::from([("ast.entropy".to_string(), 0.05)]);
        let results = evaluate_gates(&metrics, Some("simple"), false, false, None);
        let entropy = results
            .iter()
            .find(|r| r.spec.metric == "ast.entropy")
            .unwrap();
        assert!(!entropy.passed());
    }

    #[test]
    fn instability_exemption_requires_zero_fan_in_not_missing_fan_in() {
        let metrics = HashMap::from([("mdg.instability".to_string(), 0.99)]);
        let results = evaluate_gates(&metrics, Some("composable"), true, false, None);
        let instability = results
            .iter()
            .find(|r| r.spec.metric == "mdg.instability")
            .unwrap();
        // fan_in is absent (not 0.0) -> exemption must NOT apply.
        assert!(!instability.passed());
    }

    #[test]
    fn pillar_for_metric_matches_prefix() {
        assert_eq!(pillar_for_metric("cfg.cyclomatic"), "simple");
        assert_eq!(pillar_for_metric("mdg.fan_in"), "composable");
        assert_eq!(pillar_for_metric("cpg.taint_flows"), "secure");
    }
}
