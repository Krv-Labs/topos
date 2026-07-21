//! `topos_preference_walk` — convert a strict generator ordering into a
//! concrete relaxation walk on Ω.
//!
//! Given a ranking (and optionally a current verdict), it returns the
//! descending preference-ordered sequence of lattice verdicts to aim for,
//! annotated with the satisfied-generator sets so the agent can see at a
//! glance what changing to each step requires. Purely a computation — no
//! source code, no I/O.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_engine::core::omega::EvaluationValue;
use topos_engine::evaluation::preferences::{Generator, UserPreferences};

use crate::formatting::to_tool_result;
use crate::schemas::{
    lattice_to_str, str_to_lattice, GeneratorInput, LatticeElement, PreferenceWalkInput,
    PreferenceWalkResult, WalkStep,
};
use crate::server::ToposServer;

fn generators_satisfied(value: EvaluationValue) -> Vec<GeneratorInput> {
    let bits = value.bits();
    let mut out = Vec::new();
    if bits & 0b001 != 0 {
        out.push(GeneratorInput::Simple);
    }
    if bits & 0b010 != 0 {
        out.push(GeneratorInput::Composable);
    }
    if bits & 0b100 != 0 {
        out.push(GeneratorInput::Secure);
    }
    out
}

fn to_step(prefs: &UserPreferences, value: EvaluationValue) -> WalkStep {
    WalkStep {
        verdict: lattice_to_str(value),
        preference_score: prefs.score(value),
        generators_satisfied: generators_satisfied(value),
    }
}

fn ranking_wire(ranking: &[GeneratorInput]) -> Vec<GeneratorInput> {
    ranking.to_vec()
}

fn render_step(s: &WalkStep) -> String {
    let sat = if s.generators_satisfied.is_empty() {
        "—".to_string()
    } else {
        s.generators_satisfied
            .iter()
            .map(|g| g.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "- `{}` (score {}) — satisfies: {sat}",
        s.verdict.as_str(),
        s.preference_score
    )
}

pub(crate) fn render_preference_walk_md(r: &PreferenceWalkResult) -> String {
    let ranking_str = r
        .ranking
        .iter()
        .map(|g| g.as_str())
        .collect::<Vec<_>>()
        .join(" ≻ ");
    let mut lines = vec![
        "# Preference Walk".to_string(),
        format!("**Ranking:** {ranking_str}"),
        format!(
            "**Aspirational target:** `{}` (aim here first)",
            r.aspirational_target.as_str()
        ),
        format!(
            "**Fallback target:** `{}` — divert here if the aspirational target plateaus",
            r.fallback_target.as_str()
        ),
    ];
    if let Some(current) = r.current {
        lines.push(format!("**Current verdict:** `{}`", current.as_str()));
        lines.push(format!(
            "**Progress to target:** {:.0}%",
            r.progress * 100.0
        ));
        if let Some(next_step) = r.next_step {
            lines.push(format!("**Immediate next step:** `{}`", next_step.as_str()));
        } else {
            lines.push("_Already at or beyond the aspirational target — no walk._".to_string());
        }
    }
    if !r.walk.is_empty() {
        lines.push("\n## Walk (descending preference)".to_string());
        lines.extend(r.walk.iter().map(render_step));
    }
    lines.push("\n## Full induced order on Ω".to_string());
    lines.extend(r.induced_order.iter().map(render_step));
    if let Some(error) = &r.error {
        lines.push(format!("\n> error: {error}"));
    }
    lines.join("\n")
}

#[tool_router(router = preferences_router, vis = "pub(crate)")]
impl ToposServer {
    /// Turn a generator ranking into a preference-ordered relaxation walk.
    ///
    /// Pure and read-only (lattice math only; no files, no scoring). Call
    /// after an evaluation to pick the next verdict to aim for, or to relax
    /// the goal gracefully under a token/time budget. Returns a
    /// PreferenceWalkResult: `walk` (steps from target down to just above
    /// `current`), `next_step`, `progress` in [0, 1],
    /// `aspirational_target`/`fallback_target`, and `induced_order` (all 8
    /// verdicts ranked).
    #[tool(
        name = "topos_preference_walk",
        annotations(
            title = "Topos Preference Walk",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_preference_walk(
        &self,
        Parameters(params): Parameters<PreferenceWalkInput>,
    ) -> CallToolResult {
        let ranking: Vec<Generator> = params.ranking.iter().map(|g| g.to_generator()).collect();
        let arr: Result<[Generator; 3], _> = ranking.clone().try_into();
        let prefs = match arr {
            Ok(arr) => {
                let target = params.target.map(str_to_lattice);
                UserPreferences::with_target(arr, target).map_err(|e| e.to_string())
            }
            Err(_) => Err("ranking must contain exactly simple, composable, secure".to_string()),
        };
        let prefs = match prefs {
            Ok(prefs) => prefs,
            Err(exc) => {
                let model = PreferenceWalkResult {
                    ranking: ranking_wire(&params.ranking),
                    aspirational_target: LatticeElement::IDEAL,
                    fallback_target: LatticeElement::IDEAL,
                    current: params.current,
                    next_step: None,
                    progress: 0.0,
                    walk: Vec::new(),
                    induced_order: Vec::new(),
                    warnings: Vec::new(),
                    error: Some(exc),
                };
                let md = render_preference_walk_md(&model);
                return to_tool_result(&model, md);
            }
        };

        let current_value = params.current.map(str_to_lattice);
        let walk_values = prefs.relaxation_walk(current_value);
        let next_value = current_value.and_then(|c| prefs.next_step(c));
        let progress = current_value
            .map(|c| (prefs.progress(c) * 1000.0).round() / 1000.0)
            .unwrap_or(0.0);

        let model = PreferenceWalkResult {
            ranking: prefs
                .ranking()
                .iter()
                .map(|&g| match g {
                    Generator::Simple => GeneratorInput::Simple,
                    Generator::Composable => GeneratorInput::Composable,
                    Generator::Secure => GeneratorInput::Secure,
                })
                .collect(),
            aspirational_target: lattice_to_str(prefs.aspirational_target()),
            fallback_target: lattice_to_str(prefs.fallback_target()),
            current: params.current,
            next_step: next_value.map(lattice_to_str),
            progress,
            walk: walk_values.iter().map(|&v| to_step(&prefs, v)).collect(),
            induced_order: prefs
                .induced_total_order()
                .iter()
                .map(|&v| to_step(&prefs, v))
                .collect(),
            warnings: Vec::new(),
            error: None,
        };
        let md = render_preference_walk_md(&model);
        to_tool_result(&model, md)
    }
}
