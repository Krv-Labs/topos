//! Response formatters for the Topos MCP server.
//!
//! Converts `ClassificationResult` and distance results into the wire
//! models defined in [`crate::schemas`], and renders the markdown channel.

use std::collections::{BTreeMap, HashMap};

use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::omega::EvaluationValue;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::policies::gates::PILLAR_METRIC_PREFIXES;
use topos_engine::evaluation::preferences::UserPreferences;
use topos_engine::evaluation::suggestions::suggest_refactors;
use topos_engine::evaluation::suppression::AdjustedVerdict;

use crate::evaluation::{
    BRANCH_NOT_INDEXED_MARKER, INVALID_GITNEXUS_MARKERS, STALE_GITNEXUS_MARKER,
};
use crate::schemas::{
    lattice_to_str, priority_str, AcknowledgedRisk, AgentContract, BindingConstraint,
    EvaluationResult, FunctionEntry, GeneratorInput, PillarResult, PreferenceWalk, PrioritySource,
    RefactorTarget, SecurityFinding, Suggestion,
};

/// `RefactorTarget::severity` for a metric whose failure actually costs its
/// pillar's `achieved`; see `crate::refactor_targets::gate_severity`, which
/// derives it from `GateSpec::gates_achieved`.
const GATING_SEVERITY: &str = "fix";

/// The first target whose metric actually gates a pillar.
///
/// Targets arrive already ordered gating-first within a pillar, so this is
/// normally `targets[0]`; it differs exactly when every remaining failure
/// is advisory. Routing and [`EvaluationResult::binding_constraint`] both
/// go through this one function, so the field an agent reads and the edit
/// it is told to make cannot name different metrics.
fn top_gating_target(targets: &[RefactorTarget]) -> Option<&RefactorTarget> {
    targets.iter().find(|t| t.severity == GATING_SEVERITY)
}

/// Agent-contract fields implied by COMPOSABLE setup state.
#[derive(Debug, Default, Clone)]
pub struct ComposableContractSignals {
    pub blocked_by: Vec<String>,
    pub risk_flags: Vec<String>,
    pub next_tool: Option<String>,
    pub next_action: Option<String>,
}

fn generator_wire(g: topos_engine::evaluation::preferences::Generator) -> GeneratorInput {
    GeneratorInput::from_generator(g)
}

/// Classify COMPOSABLE setup blockers from the shared warning markers.
pub fn composable_contract_signals(
    coupling_available: bool,
    warnings: &[String],
    include_missing: bool,
) -> ComposableContractSignals {
    let mut blocked_by: Vec<String> = Vec::new();
    let mut risk_flags: Vec<String> = Vec::new();

    let invalid_override = warnings
        .iter()
        .any(|w| INVALID_GITNEXUS_MARKERS.iter().any(|m| w.contains(m)));
    let branch_not_indexed = warnings
        .iter()
        .any(|w| w.to_lowercase().contains(BRANCH_NOT_INDEXED_MARKER));
    let stale_graph = warnings.iter().any(|w| w.contains(STALE_GITNEXUS_MARKER));

    if !coupling_available {
        if invalid_override {
            blocked_by.push("invalid_gitnexus_dir".into());
            risk_flags.push("invalid_gitnexus_dir".into());
        } else if branch_not_indexed {
            blocked_by.push("branch_not_indexed_gitnexus_dir".into());
            risk_flags.push("branch_not_indexed_gitnexus_dir".into());
        } else if include_missing {
            blocked_by.push("missing_gitnexus_dir".into());
        }
        if invalid_override || branch_not_indexed || include_missing {
            risk_flags.push("composable_unavailable".into());
        }
    }

    if stale_graph {
        blocked_by.push("stale_gitnexus_dir".into());
        risk_flags.push("stale_gitnexus_dir".into());
    }

    const CONTRACT_ACTIONS: &[(&str, Option<&str>, &str)] = &[
        (
            "invalid_gitnexus_dir",
            None,
            "fix gitnexus_dir — it must resolve inside the file root",
        ),
        (
            "branch_not_indexed_gitnexus_dir",
            Some("topos_generate_depgraph"),
            "run topos_generate_depgraph to index the current branch",
        ),
        (
            "stale_gitnexus_dir",
            Some("topos_generate_depgraph"),
            "run topos_generate_depgraph to refresh COMPOSABLE",
        ),
    ];

    let matched_action = CONTRACT_ACTIONS
        .iter()
        .find(|(reason, _, _)| blocked_by.iter().any(|b| b == reason));
    let (next_tool, next_action) = match matched_action {
        Some((_, tool, action)) => (tool.map(str::to_string), Some(action.to_string())),
        None => (None, None),
    };

    ComposableContractSignals {
        blocked_by,
        risk_flags,
        next_tool,
        next_action,
    }
}

/// Shared `blocked_by` / `risk_flags` prelude for every `AgentContract` builder.
#[derive(Debug, Default, Clone)]
pub struct AgentContractPreludeInput<'a> {
    pub coupling_available: bool,
    pub warnings: &'a [String],
    pub parse_failures: usize,
    pub grade_capped: bool,
    pub active_security_findings: bool,
    pub acknowledged_security_risks: bool,
}

/// Shared prelude output: composable signals plus the common flag extensions.
#[derive(Debug, Clone)]
pub struct AgentContractPrelude {
    pub blocked_by: Vec<String>,
    pub risk_flags: Vec<String>,
    pub composable: ComposableContractSignals,
}

/// Always uses `include_missing = true` (D2a).
pub fn agent_contract_prelude(input: AgentContractPreludeInput<'_>) -> AgentContractPrelude {
    let composable = composable_contract_signals(input.coupling_available, input.warnings, true);
    let mut blocked_by = composable.blocked_by.clone();
    let mut risk_flags = composable.risk_flags.clone();

    if input.parse_failures > 0 {
        blocked_by.push("parse_failures".into());
        risk_flags.push("parse_failures".into());
    }
    if input.active_security_findings {
        risk_flags.push("active_security_findings".into());
    }
    if input.acknowledged_security_risks {
        risk_flags.push("acknowledged_security_risk".into());
    }
    if input.grade_capped {
        risk_flags.push("grade_capped".into());
    }
    if !input.warnings.is_empty() {
        risk_flags.push("warnings".into());
    }

    AgentContractPrelude {
        blocked_by,
        risk_flags,
        composable,
    }
}

/// Wire code for an unparseable single-file evaluation.
pub fn agent_contract_parse_blocked() -> AgentContract {
    AgentContract {
        next_tool: None,
        next_actions: vec!["restore parseable source".into()],
        blocked_by: vec!["parse_failures".into()],
        verification_gates: Vec::new(),
        risk_flags: vec!["parse_failures".into()],
    }
}

/// Assemble the final contract from a shared prelude and tool-specific fields.
pub fn finish_agent_contract(
    blocked_by: Vec<String>,
    risk_flags: Vec<String>,
    next_tool: Option<String>,
    next_actions: Vec<String>,
    verification_gates: Vec<String>,
) -> AgentContract {
    AgentContract {
        next_tool,
        next_actions,
        blocked_by,
        verification_gates,
        risk_flags,
    }
}

/// Risk flags for depgraph responses when COMPOSABLE is not scorable.
pub fn depgraph_unavailable_risk_flags(blocked_by: &[String]) -> Vec<String> {
    let mut flags = vec!["composable_unavailable".into()];
    flags.extend(blocked_by.iter().cloned());
    flags
}

/// Materialize a `PreferenceWalk` for the result schema.
pub fn build_preference_walk(prefs: &UserPreferences, current: EvaluationValue) -> PreferenceWalk {
    let target = prefs.aspirational_target();
    let fallback = prefs.fallback_target();
    let walk = prefs.relaxation_walk(Some(current));
    let next = prefs.next_step(current);
    PreferenceWalk {
        ranking: prefs.ranking().iter().map(|&g| generator_wire(g)).collect(),
        target: lattice_to_str(target),
        fallback_target: lattice_to_str(fallback),
        walk: walk.into_iter().map(lattice_to_str).collect(),
        next_step: next.map(lattice_to_str),
        progress: (prefs.progress(current) * 1000.0).round() / 1000.0,
    }
}

/// Priority-aware next-step hint for agents.
pub fn build_guidance(result: &ClassificationResult) -> String {
    let simple_ok = result.dimensions.get("simple") == Some(&EvaluationValue::Simple);
    let composable_ok = result.dimensions.get("composable") == Some(&EvaluationValue::Composable);
    let secure_ok = result.dimensions.get("secure") == Some(&EvaluationValue::Secure);
    let navigable_ok = result.dimensions.get("navigable") == Some(&EvaluationValue::Navigable);

    match result.priority {
        Priority::Composable => {
            if !result.dimensions.contains_key("composable") {
                "COMPOSABLE not measured — provide a ModuleDependencyGraph (gitnexus_dir) \
                 to score the composable generator."
                    .into()
            } else if !composable_ok {
                "Balance instability (aim for 0.3–0.7) and reduce fan-in/fan-out (aim for \
                 <= 15) to satisfy COMPOSABLE."
                    .into()
            } else {
                "COMPOSABLE satisfied.  Simplify CFG/functions, flatten deep nesting, and \
                 address any CPG security findings to reach PLATINUM."
                    .into()
            }
        }
        Priority::Simple => {
            if !simple_ok {
                "Reduce CFG/function cyclomatic complexity (aim for <= 15/10) and ensure \
                 AST entropy is structured (0.2–0.8) to satisfy SIMPLE."
                    .into()
            } else {
                "SIMPLE satisfied.  Add COMPOSABLE / SECURE / NAVIGABLE checks to reach \
                 PLATINUM."
                    .into()
            }
        }
        Priority::Secure => {
            if !secure_ok {
                "Eliminate all dangerous-API calls and source→sink taint flows to satisfy \
                 SECURE."
                    .into()
            } else {
                "SECURE satisfied.  Address SIMPLE / COMPOSABLE / NAVIGABLE generators to \
                 reach PLATINUM."
                    .into()
            }
        }
        Priority::Navigable => {
            if !navigable_ok {
                "Flatten the deepest nested block in the worst function — extract it into a \
                 top-level helper — to satisfy NAVIGABLE."
                    .into()
            } else {
                "NAVIGABLE satisfied.  Address SIMPLE / COMPOSABLE / SECURE generators to \
                 reach PLATINUM."
                    .into()
            }
        }
    }
}

/// Compact loop-control fields for MCP agents.
///
/// Invariant: `next_tool`/`next_actions` never contradict `blocked_by` —
/// when a target coexists with a setup blocker, `next_actions` carries both
/// the edit step and the setup remedy.
#[allow(clippy::too_many_arguments)]
pub fn build_agent_contract(
    result: &ClassificationResult,
    coupling_available: bool,
    security_findings: &[SecurityFinding],
    acknowledged_risks: &[AcknowledgedRisk],
    grade_capped: bool,
    warnings: &[String],
    refactor_targets: Option<&[RefactorTarget]>,
    offer_refactor_targets: bool,
) -> AgentContract {
    if !result.is_parseable {
        return agent_contract_parse_blocked();
    }

    let prelude = agent_contract_prelude(AgentContractPreludeInput {
        coupling_available,
        warnings,
        active_security_findings: !security_findings.is_empty(),
        acknowledged_security_risks: !acknowledged_risks.is_empty(),
        grade_capped,
        ..Default::default()
    });

    let summary = result.summary();
    let simple_ok = result.dimensions.get("simple") == Some(&EvaluationValue::Simple);
    let missing_gitnexus = prelude
        .blocked_by
        .iter()
        .any(|b| b == "missing_gitnexus_dir");

    let (next_tool, step_actions) = next_step_for_contract(
        &prelude.composable,
        refactor_targets,
        summary,
        simple_ok,
        security_findings,
        missing_gitnexus,
    );
    let mut next_actions = step_actions;

    // Reachable only when the caller passed `refactor_targets=0`, since the
    // parameter now defaults to a non-zero count: this is a re-enable hint
    // for a caller that opted out, not an upsell for the default path.
    if offer_refactor_targets && summary != EvaluationValue::Ideal {
        next_actions.push(
            "ranked edit targets are off for this call — re-run \
             topos_evaluate_file with refactor_targets=3 (the default) for concrete spans"
                .into(),
        );
    }

    finish_agent_contract(
        prelude.blocked_by,
        prelude.risk_flags,
        next_tool,
        next_actions,
        vec![
            "verify in-place edits with topos_assess_worktree_change".into(),
            "assessment status is IMPROVEMENT or IMPROVEMENT_SCORE".into(),
            "assessment status is not SUSPICIOUS_NO_STRUCTURAL_CHANGE".into(),
            "behavior tests or type/lint checks pass when available".into(),
        ],
    )
}

/// The follow-up action implied by a COMPOSABLE contract signal, or the
/// fallback prompt to generate a depgraph when COMPOSABLE is unmeasured
/// for another reason.
///
/// Extracted so the gating-target branch of [`next_step_for_contract`] stays
/// flat — nesting an `if`/`else if` inside that branch is what pushed the
/// function's NAVIGABLE divergence over the gate.
fn supplementary_action_for_gating_target(
    composable: &ComposableContractSignals,
    missing_gitnexus: bool,
) -> Option<String> {
    if let Some(action) = &composable.next_action {
        Some(action.clone())
    } else if missing_gitnexus {
        Some("run topos_generate_depgraph to score COMPOSABLE".into())
    } else {
        None
    }
}

/// Priority-ordered next-tool/next-action dispatch for the agent contract:
/// an in-progress refactor target, then a COMPOSABLE setup blocker, then
/// IDEAL confirmation, then the weakest unmet pillar, in that fixed order.
///
/// The edit step is offered for a *gating* target only. An advisory-only
/// target list means no verdict is waiting on an edit, so the contract
/// falls through to the ordinary dispatch instead of routing the agent
/// into an assess loop over a metric that cannot change a pillar.
fn next_step_for_contract(
    composable: &ComposableContractSignals,
    refactor_targets: Option<&[RefactorTarget]>,
    summary: EvaluationValue,
    simple_ok: bool,
    security_findings: &[SecurityFinding],
    missing_gitnexus: bool,
) -> (Option<String>, Vec<String>) {
    if let Some(first) = refactor_targets.and_then(top_gating_target) {
        let mut actions = vec![format!(
            "edit target {} ({}) — one focused structural change",
            first.target_id, first.metric
        )];
        if let Some(action) = supplementary_action_for_gating_target(composable, missing_gitnexus) {
            actions.push(action);
        }
        return (Some("topos_assess_worktree_change".into()), actions);
    }
    if let Some(action) = &composable.next_action {
        return (composable.next_tool.clone(), vec![action.clone()]);
    }
    if summary == EvaluationValue::Ideal {
        return (
            Some("topos_evaluate_project".into()),
            vec!["confirm project rollup and behavior tests before accepting".into()],
        );
    }
    if !simple_ok {
        return (
            Some("topos_inspect_code".into()),
            vec!["inspect weakest measured pillar, then verify a focused patch".into()],
        );
    }
    if !security_findings.is_empty() {
        return (
            Some("topos_inspect_code".into()),
            vec!["remove active SECURE findings or acknowledge intentional risk".into()],
        );
    }
    if missing_gitnexus {
        return (
            Some("topos_generate_depgraph".into()),
            vec!["run topos_generate_depgraph to score COMPOSABLE".into()],
        );
    }
    (
        Some("topos_inspect_code".into()),
        vec!["inspect weakest measured pillar, then verify a focused patch".into()],
    )
}

/// The 'COMPOSABLE not scored' note surfaced when no MDG is available.
pub fn mdg_unavailable_message(warnings: &[String]) -> String {
    warnings.first().cloned().unwrap_or_else(|| {
        "unavailable — no ModuleDependencyGraph; run 'topos depgraph generate' to score \
         COMPOSABLE."
            .to_string()
    })
}

fn generator_for_dim(dim: &str) -> EvaluationValue {
    match dim {
        "composable" => EvaluationValue::Composable,
        "secure" => EvaluationValue::Secure,
        _ => EvaluationValue::Simple,
    }
}

/// Build the lean per-pillar (simple, composable, secure) summary.
pub fn build_pillars(
    result: &ClassificationResult,
    coupling_available: bool,
) -> BTreeMap<String, PillarResult> {
    let mut pillars = BTreeMap::new();
    for (dim, prefixes) in PILLAR_METRIC_PREFIXES {
        let has_metrics = result
            .raw_metrics
            .keys()
            .any(|k| prefixes.iter().any(|p| k.starts_with(p)));
        let achieved = result.dimensions.get(*dim) == Some(&generator_for_dim(dim));
        let score = result.scores.get(*dim).copied().unwrap_or(0.0);
        if has_metrics || (*dim == "composable" && !coupling_available) {
            pillars.insert(
                dim.to_string(),
                PillarResult {
                    achieved,
                    score: (score * 1000.0).round() / 10.0,
                },
            );
        }
    }
    pillars
}

fn dim_for_metric_key(key: &str) -> Option<&'static str> {
    for (dim, prefixes) in PILLAR_METRIC_PREFIXES {
        if prefixes.iter().any(|p| key.starts_with(p)) {
            return Some(dim);
        }
    }
    None
}

/// Keep only interpretation strings for generators that were NOT satisfied,
/// plus notes with no pillar mapping (e.g. `mdg.unavailable`).
fn failing_interpretation(
    result: &ClassificationResult,
    interpretation: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut achieved: HashMap<&str, bool> = HashMap::new();
    for (dim, _) in PILLAR_METRIC_PREFIXES {
        achieved.insert(
            dim,
            result.dimensions.get(*dim) == Some(&generator_for_dim(dim)),
        );
    }
    interpretation
        .iter()
        .filter(|(key, _)| match dim_for_metric_key(key) {
            None => true,
            Some(dim) => !achieved.get(dim).copied().unwrap_or(false),
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Options for [`to_evaluation_result`] beyond the classification itself.
#[derive(Default)]
pub struct EvalResultOptions<'a> {
    pub preferences: Option<&'a UserPreferences>,
    pub priority_source: PrioritySource,
    pub warnings: Vec<String>,
    pub security_findings: Vec<SecurityFinding>,
    pub acknowledged_risks: Vec<AcknowledgedRisk>,
    pub adjusted_verdict: Option<&'a AdjustedVerdict>,
    pub include_agent_contract: bool,
    /// Attach the raw-metric floats and the full interpretation map.
    ///
    /// Defaults to `false` (see [`EvalResultOptions::new`]) because the raw
    /// metrics are the single largest block in the structured channel and
    /// every caller that wants them has a `verbose` parameter to forward.
    pub verbose: bool,
    pub metric_locations: BTreeMap<String, Vec<FunctionEntry>>,
    pub refactor_targets: Option<Vec<RefactorTarget>>,
    pub offer_refactor_targets: bool,
    pub include_security_findings: bool,
}

impl<'a> EvalResultOptions<'a> {
    /// Defaults chosen so the *compact* shape is what a caller gets by
    /// omission: `verbose` is opt-in, not opt-out. It used to default to
    /// `true`, which silently shipped the full raw-metric map from every
    /// construction site that never mentioned it (notably both assessment
    /// evaluations, which have no `verbose` input at all and expose metric
    /// movement through `metric_deltas` instead).
    pub fn new() -> Self {
        EvalResultOptions {
            include_agent_contract: true,
            verbose: false,
            include_security_findings: true,
            ..Default::default()
        }
    }
}

/// Convert a `ClassificationResult` into the wire model.
///
/// When `verbose` is false the structured channel omits the raw-metric
/// floats and trims `interpretation` to failing generators only.
/// `include_security_findings` gates only the findings payload field —
/// routing and guidance always derive from the true findings.
pub fn to_evaluation_result(
    result: &ClassificationResult,
    coupling_available: bool,
    opts: EvalResultOptions<'_>,
) -> EvaluationResult {
    let summary = match opts.adjusted_verdict {
        Some(v) => v.adjusted_element,
        None => result.summary(),
    };
    let walk = opts
        .preferences
        .map(|prefs| build_preference_walk(prefs, summary));

    let mut interpretation = result.interpretation.clone();
    if !coupling_available {
        interpretation
            .entry("mdg.unavailable".to_string())
            .or_insert_with(|| mdg_unavailable_message(&opts.warnings));
    }

    let raw_metrics;
    if opts.verbose {
        raw_metrics = result.raw_metrics.clone();
    } else {
        interpretation = failing_interpretation(result, &interpretation);
        raw_metrics = BTreeMap::new();
    }

    let mut dimensions = result.dimensions.clone();
    if let Some(v) = opts.adjusted_verdict {
        if v.adjusted_secure_pass {
            dimensions.insert("secure".to_string(), EvaluationValue::Secure);
        }
    }
    let display_result = ClassificationResult {
        is_parseable: result.is_parseable,
        dimensions: dimensions.clone(),
        scores: result.scores.clone(),
        lattice_element: summary,
        priority: result.priority,
        raw_metrics: result.raw_metrics.clone(),
        interpretation: result.interpretation.clone(),
        is_entrypoint_module: result.is_entrypoint_module,
        is_stable_leaf_module: result.is_stable_leaf_module,
    };

    let grade_capped = opts
        .adjusted_verdict
        .map(|v| v.grade_capped)
        .unwrap_or(false);

    let agent_contract = if opts.include_agent_contract {
        Some(build_agent_contract(
            &display_result,
            coupling_available,
            &opts.security_findings,
            &opts.acknowledged_risks,
            grade_capped,
            &opts.warnings,
            opts.refactor_targets.as_deref(),
            opts.offer_refactor_targets,
        ))
    } else {
        None
    };

    // Derived from the same ordered list `refactor_targets` is served from,
    // so the two can never name different metrics.
    let binding_constraint = opts
        .refactor_targets
        .as_deref()
        .and_then(top_gating_target)
        .map(|target| BindingConstraint {
            pillar: target
                .failing_generators
                .first()
                .cloned()
                .unwrap_or_else(|| "simple".to_string()),
            metric: target.metric.clone(),
            value: target.current_value,
            threshold: target.threshold,
            symbol: target.symbol.clone(),
            line_start: target.line_start,
            line_end: target.line_end,
        });

    let core_findings: Vec<topos_engine::evaluation::security_guidance::SecurityFinding> =
        opts.security_findings.iter().map(|f| f.to_core()).collect();
    let suggestions: Vec<Suggestion> = suggest_refactors(result, &core_findings)
        .into_iter()
        .map(|s| Suggestion {
            pillar: s.pillar,
            metric: s.metric,
            severity: s.severity,
            message: s.message,
        })
        .collect();

    EvaluationResult {
        is_parseable: result.is_parseable,
        lattice_element: lattice_to_str(summary),
        lattice_symbol: summary.symbol().to_string(),
        lattice_description: summary.description().to_string(),
        dimensions: dimensions
            .iter()
            .map(|(dim, &val)| (dim.clone(), lattice_to_str(val)))
            .collect(),
        scores: result
            .scores
            .iter()
            .map(|(dim, s)| (dim.clone(), (s * 1000.0).round() / 10.0))
            .collect(),
        pillars: build_pillars(&display_result, coupling_available),
        priority: priority_str(result.priority).to_string(),
        priority_source: opts.priority_source,
        guidance: build_guidance(&display_result),
        coupling_available,
        raw_metrics,
        interpretation,
        metric_locations: opts.metric_locations,
        warnings: opts.warnings.clone(),
        agent_contract,
        security_findings: if opts.include_security_findings {
            opts.security_findings.clone()
        } else {
            Vec::new()
        },
        acknowledged_risks: opts.acknowledged_risks,
        raw_lattice_element: opts.adjusted_verdict.map(|v| lattice_to_str(v.raw_element)),
        adjusted_lattice_element: opts
            .adjusted_verdict
            .map(|v| lattice_to_str(v.adjusted_element)),
        secure_raw: opts.adjusted_verdict.map(|v| v.raw_secure_pass),
        secure_adjusted: opts.adjusted_verdict.map(|v| v.adjusted_secure_pass),
        grade_capped,
        suggestions,
        preference_walk: walk,
        refactor_targets: opts.refactor_targets.unwrap_or_default(),
        binding_constraint,
        error: None,
    }
}

// ---------------------------------------------------------------------------
// Dual-channel converter
// ---------------------------------------------------------------------------

/// Return a dual-channel tool result: markdown for the LLM plus the model's
/// JSON dump as `structured_content` for programmatic clients.
pub fn to_tool_result<T: Serialize>(model: &T, markdown: String) -> CallToolResult {
    // Every tool funnels through here, so this is the one place a
    // server-level condition can reach an agent mid-loop. The banner is
    // emitted *only* when the binary on disk has outrun this process, so a
    // healthy call pays nothing — the structured channel is untouched either
    // way. Staleness cannot be advertised in `instructions` instead: those
    // are built during the handshake (`initialize` or `server/discover`),
    // when a just-started process is never stale.
    let markdown = match crate::build_info::stale_banner() {
        Some(banner) => format!("{banner}\n\n{markdown}"),
        None => markdown,
    };
    let mut result = CallToolResult::success(vec![ContentBlock::text(markdown)]);
    result.structured_content = serde_json::to_value(model).ok();
    result
}

// ---------------------------------------------------------------------------
// Markdown renderers
// ---------------------------------------------------------------------------

/// Compact markdown for an error/early-return EvaluationResult.
pub fn error_md(model: &EvaluationResult) -> String {
    format!(
        "**Error:** {}",
        model
            .error
            .clone()
            .unwrap_or_else(|| model.lattice_description.clone())
    )
}

pub fn render_evaluation_md(e: &EvaluationResult, title: Option<&str>, verbose: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(title) = title {
        lines.push(format!("# {title}"));
    }
    lines.push(format!(
        "**Lattice:** {} {} — {}",
        e.lattice_symbol,
        e.lattice_element.as_str(),
        e.lattice_description
    ));
    if !e.is_parseable {
        lines.push("> ⚠️ Code failed to parse.".into());
        return lines.join("\n");
    }

    push_generators_section(&mut lines, e);

    lines.push(String::new());
    lines.push(format!("**Priority:** `{}`", e.priority));
    lines.push(format!("**Guidance:** {}", e.guidance));
    push_agent_contract_section(&mut lines, e);
    push_preference_walk_section(&mut lines, e);
    push_secure_overlay_section(&mut lines, e);
    push_acknowledged_risks_section(&mut lines, e);
    if e.grade_capped {
        lines.push(
            "> Max grade capped below IDEAL because an acknowledged security risk is active."
                .into(),
        );
    }
    push_metric_locations_section(&mut lines, e);
    push_refactor_targets_section(&mut lines, e);
    push_suggestions_section(&mut lines, e);
    if verbose {
        push_raw_metrics_section(&mut lines, e);
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// render_evaluation_md section renderers
// ---------------------------------------------------------------------------
//
// Each renders one independent markdown section, pushing onto the shared
// `lines` buffer and no-opping when its section has nothing to show.

fn push_generators_section(lines: &mut Vec<String>, e: &EvaluationResult) {
    lines.push(String::new());
    lines.push("## Generators".into());
    for (dim, val) in e.dimensions.iter() {
        let score = e.scores.get(dim).copied().unwrap_or(0.0);
        lines.push(format!("- **{dim}**: {} ({score:.1}%)", val.as_str()));
    }
    if !e.coupling_available {
        lines.push(
            "- _composable: not measured (no ModuleDependencyGraph available — COMPOSABLE \
             / IDEAL unreachable)._"
                .into(),
        );
    }
}

fn push_agent_contract_section(lines: &mut Vec<String>, e: &EvaluationResult) {
    let Some(contract) = &e.agent_contract else {
        return;
    };
    if contract.next_tool.is_none()
        && contract.next_actions.is_empty()
        && contract.blocked_by.is_empty()
    {
        return;
    }
    lines.push(String::new());
    lines.push("## Agent Contract".into());
    if let Some(next_tool) = &contract.next_tool {
        lines.push(format!("- **Next tool:** `{next_tool}`"));
    }
    for action in &contract.next_actions {
        lines.push(format!("- **Action:** {action}"));
    }
    for blocked in &contract.blocked_by {
        lines.push(format!("- **Blocked by:** `{blocked}`"));
    }
}

fn push_preference_walk_section(lines: &mut Vec<String>, e: &EvaluationResult) {
    let Some(pw) = &e.preference_walk else {
        return;
    };
    let ranking = pw
        .ranking
        .iter()
        .map(|g| g.as_str())
        .collect::<Vec<_>>()
        .join(" ≻ ");
    lines.push(String::new());
    lines.push("## Preference Walk".into());
    lines.push(format!("- **Ranking:** {ranking}"));
    lines.push(format!(
        "- **Target (aspirational):** {} ({:.0}% of the way)",
        pw.target.as_str(),
        pw.progress * 100.0
    ));
    lines.push(format!(
        "- **Fallback (ideal intersection):** {} — divert here if IDEAL plateaus",
        pw.fallback_target.as_str()
    ));
    if let Some(next_step) = pw.next_step {
        lines.push(format!("- **Next step:** aim for `{}`", next_step.as_str()));
    }
    if pw.walk.is_empty() {
        lines.push("- **Walk:** _at or beyond target — no further steps._".into());
    } else {
        let walk_str = pw
            .walk
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" → ");
        lines.push(format!("- **Walk:** {walk_str}"));
    }
}

fn push_secure_overlay_section(lines: &mut Vec<String>, e: &EvaluationResult) {
    let (Some(raw), adjusted) = (e.secure_raw, e.secure_adjusted) else {
        return;
    };
    if Some(raw) == adjusted {
        return;
    }
    let raw_str = if raw { "PASS" } else { "FAIL" };
    let adjusted_str = if adjusted == Some(true) {
        "PASS"
    } else {
        "FAIL"
    };
    lines.push(String::new());
    lines.push(format!(
        "**SECURE overlay:** {raw_str} (raw) -> {adjusted_str} (acknowledged)"
    ));
}

fn push_acknowledged_risks_section(lines: &mut Vec<String>, e: &EvaluationResult) {
    if e.acknowledged_risks.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("## Acknowledged Risks".into());
    for risk in &e.acknowledged_risks {
        let name = risk.callee.clone().unwrap_or_else(|| risk.kind.clone());
        lines.push(format!("- `{name}` line {}: {}", risk.line, risk.reason));
    }
}

fn push_metric_locations_section(lines: &mut Vec<String>, e: &EvaluationResult) {
    if e.metric_locations.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("## Metric Locations".into());
    for (metric, entries) in e.metric_locations.iter() {
        lines.push(format!("- `{metric}`:"));
        for func in entries.iter() {
            let where_str = if func.kind.as_deref() == Some("module") {
                "module-level (not attributable to a function)".to_string()
            } else {
                format!(
                    "`{}` ({}) lines {}-{}",
                    func.qualified_name
                        .clone()
                        .unwrap_or_else(|| func.name.clone()),
                    func.kind.clone().unwrap_or_default(),
                    func.start_line.map(|l| l.to_string()).unwrap_or_default(),
                    func.end_line.map(|l| l.to_string()).unwrap_or_default()
                )
            };
            lines.push(format!("  - {where_str} — complexity {}", func.complexity));
        }
    }
}

fn push_refactor_targets_section(lines: &mut Vec<String>, e: &EvaluationResult) {
    if e.refactor_targets.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("## Refactor Targets".into());
    // Severity is a column, not just a sort input: the rows are ordered
    // gating-first, but nothing in a rendered table says so, and an agent
    // reading only the markdown would otherwise treat an advisory row as a
    // gate failure.
    lines.push("| Target | Kind | Metric | Location | Severity | Operations |".into());
    lines.push("| --- | --- | --- | --- | --- | --- |".into());
    for target in &e.refactor_targets {
        let loc = match (target.line_start, target.line_end) {
            (Some(start), Some(end)) if end != start => format!("{start}-{end}"),
            (Some(start), _) => start.to_string(),
            _ => "?".to_string(),
        };
        let symbol = target
            .symbol
            .clone()
            .unwrap_or_else(|| "<module>".to_string())
            .replace('|', "\\|");
        let ops = target
            .recommended_operations
            .iter()
            .map(|op| format!("`{op}`"))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!(
            "| `{}` `{symbol}` | {} | `{}` | {loc} | {} | {ops} |",
            target.target_id, target.kind, target.metric, target.severity
        ));
    }
}

fn push_suggestions_section(lines: &mut Vec<String>, e: &EvaluationResult) {
    if e.suggestions.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("## Suggestions".into());
    for s in &e.suggestions {
        lines.push(format!("- [ ] ({}) {}", s.pillar, s.message));
    }
}

fn push_raw_metrics_section(lines: &mut Vec<String>, e: &EvaluationResult) {
    if e.raw_metrics.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("## Raw Metrics".into());
    for (k, v) in e.raw_metrics.iter() {
        lines.push(format!("- `{k}`: {v:.3}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::classify_code_string;

    #[test]
    fn evaluation_result_round_trip() {
        let result =
            classify_code_string("def f():\n    return 1\n", "python", Priority::Simple).unwrap();
        let mut opts = EvalResultOptions::new();
        // Explicit: `new()` is the compact shape, so the raw-metrics
        // assertions below only hold once verbose is opted into.
        opts.verbose = true;
        let model = to_evaluation_result(&result, false, opts);
        assert!(model.is_parseable);
        assert!(!model.coupling_available);
        assert!(model.interpretation.contains_key("mdg.unavailable"));
        let md = render_evaluation_md(&model, Some("Test"), true);
        assert!(md.contains("# Test"));
        assert!(md.contains("**Lattice:**"));
        assert!(md.contains("## Raw Metrics"));
        // Structured channel carries the full model.
        let wire = to_tool_result(&model, md);
        assert!(wire.structured_content.is_some());
    }

    /// The default shape is the cheap one: a caller that never mentions
    /// `verbose` must not pay for the raw-metric floats.
    #[test]
    fn default_options_omit_raw_metrics() {
        let result =
            classify_code_string("def f():\n    return 1\n", "python", Priority::Simple).unwrap();
        let model = to_evaluation_result(&result, false, EvalResultOptions::new());
        assert!(
            model.raw_metrics.is_empty(),
            "EvalResultOptions::new() must default to the compact shape; \
             callers that want raw metrics forward their own verbose flag"
        );
        // Omitted from the wire entirely, not serialized as `{}`.
        let json = serde_json::to_value(&model).expect("serialize");
        assert!(json.get("raw_metrics").is_none());
    }

    #[test]
    fn non_verbose_drops_raw_metrics() {
        let result =
            classify_code_string("def f():\n    return 1\n", "python", Priority::Simple).unwrap();
        let mut opts = EvalResultOptions::new();
        opts.verbose = false;
        let model = to_evaluation_result(&result, false, opts);
        assert!(model.raw_metrics.is_empty());
        let md = render_evaluation_md(&model, None, false);
        assert!(!md.contains("## Raw Metrics"));
    }

    /// A target list shaped like the one `build_refactor_targets`
    /// produces when the preferred pillar's only failure is advisory: the
    /// raw first entry is `improve`, and the real gate failure sits behind
    /// it in another pillar.
    fn advisory_then_gating_targets() -> Vec<RefactorTarget> {
        vec![
            refactor_target("cfg.cyclomatic", "improve", "simple", "<module>"),
            refactor_target("os.system", "fix", "secure", "run_cmd"),
        ]
    }

    fn refactor_target(metric: &str, severity: &str, pillar: &str, symbol: &str) -> RefactorTarget {
        RefactorTarget {
            target_id: format!("rt_{metric}"),
            kind: "function".to_string(),
            filepath: "a.py".to_string(),
            symbol: Some(symbol.to_string()),
            line_start: Some(12),
            line_end: Some(40),
            failing_generators: vec![pillar.to_string()],
            metric: metric.to_string(),
            current_value: Some(80.0),
            threshold: Some(15.0),
            severity: severity.to_string(),
            recommended_operations: vec!["extract_helper".to_string()],
            constraints: Vec::new(),
            evidence: BTreeMap::new(),
        }
    }

    fn model_with_targets(targets: Vec<RefactorTarget>) -> EvaluationResult {
        let result =
            classify_code_string("def f():\n    return 1\n", "python", Priority::Simple).unwrap();
        let mut opts = EvalResultOptions::new();
        opts.refactor_targets = Some(targets);
        to_evaluation_result(&result, false, opts)
    }

    /// `binding_constraint` names the top *gating* target, not the top
    /// target — and it is a projection of that same entry, so the two can
    /// never point an agent at different metrics.
    #[test]
    fn binding_constraint_names_the_top_gating_target() {
        let model = model_with_targets(advisory_then_gating_targets());

        let binding = model.binding_constraint.expect("a gating target exists");
        assert_eq!(binding.metric, "os.system");
        assert_eq!(binding.pillar, "secure");
        assert_eq!(binding.symbol.as_deref(), Some("run_cmd"));
        assert_eq!(binding.value, Some(80.0));
        assert_eq!(binding.threshold, Some(15.0));
        // The advisory entry keeps its place in the served list; only the
        // binding view and the routing skip it.
        assert_eq!(model.refactor_targets[0].metric, "cfg.cyclomatic");
        let contract = model.agent_contract.expect("contract present");
        assert!(
            contract
                .next_actions
                .iter()
                .any(|a| a.starts_with("edit target rt_os.system")),
            "the edit step must name the gating target: {:?}",
            contract.next_actions
        );
        assert_eq!(
            contract.next_tool.as_deref(),
            Some("topos_assess_worktree_change")
        );
    }

    /// Advisory-only failures bind nothing: no verdict is waiting on an
    /// edit, so the contract must not open an edit/assess loop over a
    /// metric that cannot move a pillar.
    #[test]
    fn advisory_only_targets_neither_bind_nor_route() {
        let model = model_with_targets(vec![refactor_target(
            "cfg.cyclomatic",
            "improve",
            "simple",
            "<module>",
        )]);

        assert!(model.binding_constraint.is_none());
        assert_eq!(
            model.refactor_targets.len(),
            1,
            "the advisory target is still served, just not promoted"
        );
        let contract = model.agent_contract.expect("contract present");
        assert!(
            !contract
                .next_actions
                .iter()
                .any(|a| a.starts_with("edit target")),
            "no gating target, so no edit step: {:?}",
            contract.next_actions
        );
        assert_ne!(
            contract.next_tool.as_deref(),
            Some("topos_assess_worktree_change"),
            "assess-the-edit routing requires an edit to assess"
        );
        // Routing still happens, and the setup blocker still surfaces.
        assert!(!contract.next_actions.is_empty());
        assert!(contract
            .blocked_by
            .contains(&"missing_gitnexus_dir".to_string()));
    }

    /// The rendered table carries `severity` too: the rows are ordered
    /// gating-first, but ordering alone is invisible in markdown.
    #[test]
    fn refactor_target_markdown_shows_severity() {
        let model = model_with_targets(advisory_then_gating_targets());
        let md = render_evaluation_md(&model, None, false);
        // Table rows only — the agent contract also names these targets.
        let row = md
            .lines()
            .find(|l| l.starts_with("| `rt_cfg.cyclomatic`"))
            .expect("advisory row rendered");
        assert!(row.contains("| improve |"), "row was: {row}");
        let gating = md
            .lines()
            .find(|l| l.starts_with("| `rt_os.system`"))
            .expect("gating row rendered");
        assert!(gating.contains("| fix |"), "row was: {gating}");
    }

    /// Both binding cases, built through the real producers
    /// (`build_metric_locations` -> `build_refactor_targets`) instead of
    /// hand-written targets. That pins the producer/consumer contract:
    /// `top_gating_target` matches on the `severity` string
    /// `refactor_targets::gate_severity` emits, and if those two ever
    /// drifted, every result would silently lose its binding constraint
    /// and its edit routing.
    fn model_for_source(source: &str) -> EvaluationResult {
        let result = classify_code_string(source, "python", Priority::Simple).unwrap();
        let locations = crate::metric_locations::build_metric_locations(source, "python", &result);
        let targets = crate::refactor_targets::build_refactor_targets(
            "a.py",
            &result,
            &[],
            &locations,
            None,
            3,
        );
        let mut opts = EvalResultOptions::new();
        opts.refactor_targets = Some(targets);
        to_evaluation_result(&result, false, opts)
    }

    #[test]
    fn a_passing_file_binds_nothing() {
        let model = model_for_source("def f():\n    return 1\n");
        assert!(
            model.refactor_targets.is_empty(),
            "no gate is out of band, so there is nothing to target: {:?}",
            model.refactor_targets
        );
        assert!(model.binding_constraint.is_none());
    }

    #[test]
    fn a_failing_gate_binds_the_metric_the_producer_labeled() {
        // Eleven independent branches in one function: past
        // `ast.max_function_complexity`'s bound of 10, and the whole-file
        // `cfg.cyclomatic` sum trips its advisory bound of 15 too.
        let body: String = (0..11)
            .map(|i| format!("    if x == {i}:\n        return {i}\n"))
            .collect();
        let model = model_for_source(&format!("def f(x):\n{body}    return -1\n"));

        let binding = model
            .binding_constraint
            .as_ref()
            .expect("a gating metric is out of band");
        assert_eq!(binding.metric, "ast.max_function_complexity");
        assert_eq!(binding.pillar, "simple");
        assert_eq!(
            binding.metric, model.refactor_targets[0].metric,
            "the producer already ranks gating first, so the binding view \
             and the served list agree on the top entry"
        );
        assert_eq!(model.refactor_targets[0].severity, "fix");
    }

    #[test]
    fn parse_failure_contract_blocks() {
        let result = classify_code_string("def f(:\n", "python", Priority::Simple).unwrap();
        let model = to_evaluation_result(&result, false, EvalResultOptions::new());
        if !model.is_parseable {
            let contract = model.agent_contract.expect("contract present");
            assert!(contract.blocked_by.contains(&"parse_failures".to_string()));
            assert!(contract.risk_flags.contains(&"parse_failures".to_string()));
        }
    }
}
