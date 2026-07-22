//! Assessment tools — compare current vs. proposed code on the lattice.
//!
//! This is the main tool family for agent refactor loops. Anti-gaming
//! guardrail: if scores moved meaningfully while AST edit distance is near
//! zero, status becomes `SUSPICIOUS_NO_STRUCTURAL_CHANGE`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::Value;
use topos_engine::adapters::discovery::find_git_root;
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::core::omega::{verdict_from_generators, EvaluationValue, Omega};
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::functors::profunctors::ast::compare::calculate_ast_distance;

use crate::diagnostics::{overlay_for_source, SecurityOverlay};
use crate::evaluation::{
    classify_morphism, detect_language, gitnexus_warnings, load_dep_graph, resolve_gitnexus_dir,
};
use crate::formatting::{
    composable_contract_signals, to_evaluation_result, to_tool_result, EvalResultOptions,
};
use crate::schemas::{
    lattice_to_str, priority_str, resolve_priority, AgentContract, AssessChangesetInput,
    AssessImprovementInput, AssessSnapshotInput, AssessWorktreeChangeInput, AssessmentResult,
    AssessmentStatus, BeginRefactorInput, ChangesetFileEntry, ChangesetResult, EvaluationResult,
    GeneratorInput, LatticeElement, PrioritySource, SnapshotResult, UserPreferencesInput,
};
use crate::security::{read_safe_utf8_file, resolve_file_root, resolve_within_root};
use crate::server::ToposServer;
use crate::snapshots::{now as snapshot_now, read_snapshot, sha256_hex, write_snapshot};

const STRUCTURAL_CHANGE_THRESHOLD: f64 = 0.02;
const MEANINGFUL_SCORE_DELTA: f64 = 3.0;
const REGRESSION_DIFF_MAX_LINES: usize = 40;

fn regression_statuses() -> [AssessmentStatus; 3] {
    [
        AssessmentStatus::REGRESSION,
        AssessmentStatus::REGRESSION_SCORE,
        AssessmentStatus::SUSPICIOUS_NO_STRUCTURAL_CHANGE,
    ]
}

fn is_regression(status: AssessmentStatus) -> bool {
    regression_statuses().contains(&status)
}

fn overlay_opts(overlay: Option<&SecurityOverlay>, opts: &mut EvalResultOptions<'_>) {
    if let Some(overlay) = overlay {
        opts.security_findings = overlay.active_findings.clone();
        opts.acknowledged_risks = overlay.acknowledged_risks.clone();
    }
}

// ---------------------------------------------------------------------------
// Status determination
// ---------------------------------------------------------------------------

fn is_suspicious(
    status: AssessmentStatus,
    distance: Option<f64>,
    score_deltas: &HashMap<String, f64>,
) -> bool {
    let Some(distance) = distance else {
        return false;
    };
    if distance >= STRUCTURAL_CHANGE_THRESHOLD {
        return false;
    }
    if !matches!(
        status,
        AssessmentStatus::IMPROVEMENT | AssessmentStatus::IMPROVEMENT_SCORE
    ) {
        return false;
    }
    score_deltas
        .values()
        .any(|d| d.abs() >= MEANINGFUL_SCORE_DELTA)
}

fn determine_lattice_status(
    cur_summary: EvaluationValue,
    prop_summary: EvaluationValue,
    score_deltas: &HashMap<String, f64>,
) -> AssessmentStatus {
    let lattice = Omega::default();
    if cur_summary == prop_summary {
        let score_improved = score_deltas.values().any(|&d| d > 0.0);
        let score_regressed = score_deltas.values().any(|&d| d < 0.0);
        return if score_improved && !score_regressed {
            AssessmentStatus::IMPROVEMENT_SCORE
        } else if score_regressed && !score_improved {
            AssessmentStatus::REGRESSION_SCORE
        } else {
            AssessmentStatus::LATERAL_MOVE
        };
    }
    if lattice.leq(cur_summary, prop_summary) {
        AssessmentStatus::IMPROVEMENT
    } else if lattice.leq(prop_summary, cur_summary) {
        AssessmentStatus::REGRESSION
    } else {
        AssessmentStatus::LATERAL_MOVE
    }
}

fn determine_assessment_status(
    current_res: &ClassificationResult,
    proposed_res: &ClassificationResult,
    score_deltas: &HashMap<String, f64>,
    distance: Option<f64>,
) -> (AssessmentStatus, Option<String>) {
    let mut status =
        determine_lattice_status(current_res.summary(), proposed_res.summary(), score_deltas);
    let mut suspicion = None;
    if is_suspicious(status, distance, score_deltas) {
        status = AssessmentStatus::SUSPICIOUS_NO_STRUCTURAL_CHANGE;
        suspicion = Some(format!(
            "Scores improved (deltas={score_deltas:?}) but normalized AST edit distance is \
             only {:.3} — the tree barely changed. Either the refactor is trivially cosmetic \
             (comment/whitespace shuffle) or the scoring is oscillating. Re-verify with a \
             concrete structural change.",
            distance.unwrap_or(0.0)
        ));
    }
    (status, suspicion)
}

fn calculate_deltas(
    current_eval: &EvaluationResult,
    proposed_eval: &EvaluationResult,
    current_res: &ClassificationResult,
    proposed_res: &ClassificationResult,
) -> (HashMap<String, f64>, HashMap<String, f64>) {
    let mut all_dims: Vec<&String> = current_eval.scores.keys().collect();
    for dim in proposed_eval.scores.keys() {
        if !all_dims.contains(&dim) {
            all_dims.push(dim);
        }
    }
    let score_deltas = all_dims
        .iter()
        .map(|dim| {
            let delta = proposed_eval.scores.get(*dim).copied().unwrap_or(0.0)
                - current_eval.scores.get(*dim).copied().unwrap_or(0.0);
            ((*dim).clone(), (delta * 10.0).round() / 10.0)
        })
        .collect();

    let mut all_metrics: Vec<&String> = current_res.raw_metrics.keys().collect();
    for m in proposed_res.raw_metrics.keys() {
        if !all_metrics.contains(&m) {
            all_metrics.push(m);
        }
    }
    let metric_deltas = all_metrics
        .iter()
        .map(|m| {
            let delta = proposed_res.raw_metrics.get(*m).copied().unwrap_or(0.0)
                - current_res.raw_metrics.get(*m).copied().unwrap_or(0.0);
            ((*m).clone(), (delta * 1000.0).round() / 1000.0)
        })
        .collect();
    (score_deltas, metric_deltas)
}

// ---------------------------------------------------------------------------
// Regression diff
// ---------------------------------------------------------------------------

fn function_complexities(source: &str, language: &str) -> HashMap<String, (usize, Vec<String>)> {
    use topos_engine::functors::probes::ast::complexity::calculate_function_complexity_entries;
    let mut out: HashMap<String, (usize, Vec<String>)> = HashMap::new();
    let morphism = ProgramMorphism::new(source, language);
    let Some(ast) = morphism.ast.as_ref() else {
        return out;
    };
    if !morphism.is_valid() {
        return out;
    }
    let source_lines: Vec<&str> = morphism.source.lines().collect();
    for entry in calculate_function_complexity_entries(&ast.uast_root, &morphism.source) {
        if out.contains_key(&entry.name) {
            continue;
        }
        let start = entry.start_line.saturating_sub(1);
        let end = entry.end_line.min(source_lines.len());
        let body: Vec<String> = source_lines
            .get(start..end)
            .unwrap_or(&[])
            .iter()
            .map(|s| s.to_string())
            .collect();
        out.insert(entry.name.clone(), (entry.complexity, body));
    }
    out
}

/// Unified diff of the single function with the worst complexity increase.
fn regression_diff(current_src: &str, proposed_src: &str, language: &str) -> Option<String> {
    use similar::TextDiff;

    let cur = function_complexities(current_src, language);
    let prop = function_complexities(proposed_src, language);
    if cur.is_empty() || prop.is_empty() {
        return None;
    }

    let mut worst_name: Option<String> = None;
    let mut worst_delta: i64 = 0;
    for (name, (prop_cx, _)) in &prop {
        let Some((cur_cx, _)) = cur.get(name) else {
            continue;
        };
        let delta = *prop_cx as i64 - *cur_cx as i64;
        if delta > worst_delta {
            worst_delta = delta;
            worst_name = Some(name.clone());
        }
    }
    let worst_name = worst_name?;
    let (cur_cx, cur_lines) = &cur[&worst_name];
    let (prop_cx, prop_lines) = &prop[&worst_name];

    let cur_text = cur_lines.join("\n");
    let prop_text = prop_lines.join("\n");
    let diff = TextDiff::from_lines(&cur_text, &prop_text);
    let unified = diff
        .unified_diff()
        .header(
            &format!("{worst_name} (current)"),
            &format!("{worst_name} (proposed)"),
        )
        .to_string();
    let mut body: Vec<String> = unified.lines().map(|s| s.to_string()).collect();
    if body.is_empty() {
        return None;
    }

    let header = format!(
        "# regression in `{worst_name}`: cyclomatic complexity {cur_cx} -> {prop_cx} ({:+})",
        *prop_cx as i64 - *cur_cx as i64
    );
    if body.len() > REGRESSION_DIFF_MAX_LINES {
        let hidden = body.len() - REGRESSION_DIFF_MAX_LINES;
        body.truncate(REGRESSION_DIFF_MAX_LINES);
        body.push(format!("# ... (truncated, {hidden} more lines)"));
    }
    let mut lines = vec![header];
    lines.extend(body);
    Some(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Core assessment
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
struct AssessCoreArgs<'a> {
    baseline_src: String,
    proposed_src: String,
    language: String,
    priority: Priority,
    priority_source: PrioritySource,
    prefs: Option<&'a topos_engine::evaluation::preferences::UserPreferences>,
    dep_graph: Option<&'a topos_engine::graphs::mdg::object::ModuleDependencyGraph>,
    coupling_for_proposed: bool,
    file_path: Option<PathBuf>,
    allow: Vec<String>,
    include_security_findings: bool,
    warnings: Vec<String>,
}

fn assess_core(args: AssessCoreArgs<'_>) -> AssessmentResult {
    let mut baseline_morph = ProgramMorphism::new(&args.baseline_src, &args.language);
    let baseline_res = classify_morphism(&mut baseline_morph, args.priority, args.dep_graph);
    let mut proposed_morph = ProgramMorphism::new(&args.proposed_src, &args.language);
    let proposed_res = classify_morphism(&mut proposed_morph, args.priority, args.dep_graph);

    let current_overlay = overlay_for_source(
        &args.baseline_src,
        &args.language,
        &baseline_res,
        args.file_path.as_deref(),
        &args.allow,
    );
    let proposed_overlay = overlay_for_source(
        &args.proposed_src,
        &args.language,
        &proposed_res,
        args.file_path.as_deref(),
        &args.allow,
    );

    let mut cur_opts = EvalResultOptions::new();
    cur_opts.preferences = args.prefs;
    cur_opts.priority_source = args.priority_source;
    cur_opts.include_agent_contract = false;
    cur_opts.adjusted_verdict = current_overlay.as_ref().map(|o| &o.verdict);
    overlay_opts(current_overlay.as_ref(), &mut cur_opts);
    cur_opts.include_security_findings = args.include_security_findings;
    let current_eval = to_evaluation_result(&baseline_res, args.dep_graph.is_some(), cur_opts);

    let mut prop_opts = EvalResultOptions::new();
    prop_opts.preferences = args.prefs;
    prop_opts.priority_source = args.priority_source;
    prop_opts.include_agent_contract = false;
    prop_opts.adjusted_verdict = proposed_overlay.as_ref().map(|o| &o.verdict);
    overlay_opts(proposed_overlay.as_ref(), &mut prop_opts);
    prop_opts.include_security_findings = args.include_security_findings;
    let proposed_eval = to_evaluation_result(&proposed_res, args.coupling_for_proposed, prop_opts);

    let (score_deltas, metric_deltas) =
        calculate_deltas(&current_eval, &proposed_eval, &baseline_res, &proposed_res);

    let (distance, similarity) = match (
        baseline_res.is_parseable,
        proposed_res.is_parseable,
        baseline_morph.ast.as_ref(),
        proposed_morph.ast.as_ref(),
    ) {
        (true, true, Some(base_ast), Some(prop_ast)) => {
            let dist = calculate_ast_distance(base_ast, prop_ast);
            (
                Some(dist.normalized_distance),
                Some(1.0 - dist.normalized_distance),
            )
        }
        _ => (None, None),
    };

    let (status, suspicion) =
        determine_assessment_status(&baseline_res, &proposed_res, &score_deltas, distance);

    let regression = is_regression(status)
        .then(|| regression_diff(&args.baseline_src, &args.proposed_src, &args.language))
        .flatten();

    let agent_contract = assessment_contract(status, &args.warnings, &proposed_eval);

    AssessmentResult {
        status,
        priority: priority_str(args.priority).to_string(),
        priority_source: args.priority_source,
        current: current_eval,
        proposed: proposed_eval,
        score_deltas,
        metric_deltas,
        structural_distance: distance,
        similarity,
        coupling_available_for_proposed: args.coupling_for_proposed,
        baseline_hash: Some(sha256_hex(&args.baseline_src)),
        current_hash: Some(sha256_hex(&args.proposed_src)),
        warnings: args.warnings,
        agent_contract: Some(agent_contract),
        suspicion_reason: suspicion,
        regression_diff: regression,
        error: None,
    }
}

fn assessment_contract(
    status: AssessmentStatus,
    warnings: &[String],
    proposed_eval: &EvaluationResult,
) -> AgentContract {
    let mut risk_flags: Vec<String> = Vec::new();
    let mut blocked_by: Vec<String> = Vec::new();
    let mut next_actions: Vec<String> = Vec::new();

    let composable = composable_contract_signals(proposed_eval.coupling_available, warnings, false);
    blocked_by.extend(composable.blocked_by.clone());
    risk_flags.extend(composable.risk_flags.clone());
    if !warnings.is_empty() {
        risk_flags.push("warnings".into());
    }
    if proposed_eval.grade_capped {
        risk_flags.push("grade_capped".into());
    }
    if proposed_eval.secure_adjusted == Some(false) {
        risk_flags.push("active_security_findings".into());
    }

    let next_tool = if let Some(action) = composable.next_action {
        next_actions.push(action);
        composable.next_tool
    } else if status == AssessmentStatus::SUSPICIOUS_NO_STRUCTURAL_CHANGE {
        blocked_by.push("suspicious_no_structural_change".into());
        risk_flags.push("metric_gaming_risk".into());
        next_actions.push("make a real structural change before reassessing".into());
        Some("topos_inspect_code".to_string())
    } else if is_regression(status) {
        blocked_by.push("regression".into());
        risk_flags.push("regression".into());
        next_actions.push("discard or revise the proposed change".into());
        Some("topos_inspect_code".to_string())
    } else if matches!(
        status,
        AssessmentStatus::IMPROVEMENT | AssessmentStatus::IMPROVEMENT_SCORE
    ) {
        next_actions.push("run project rollup and behavior checks before accepting".into());
        Some("topos_evaluate_project".to_string())
    } else {
        next_actions.push("try a different focused structural change".into());
        Some("topos_inspect_code".to_string())
    };

    AgentContract {
        next_tool,
        next_actions,
        blocked_by,
        verification_gates: vec![
            "assessment status is IMPROVEMENT or IMPROVEMENT_SCORE".into(),
            "assessment status is not SUSPICIOUS_NO_STRUCTURAL_CHANGE".into(),
            "behavior tests or type/lint checks pass when available".into(),
        ],
        risk_flags,
    }
}

fn err_assessment(
    priority: Priority,
    priority_source: PrioritySource,
    msg: String,
    blocked_by: &str,
) -> AssessmentResult {
    let empty =
        EvaluationResult::error_result("not evaluated", priority, priority_source, String::new());
    let mut empty_no_err = empty;
    empty_no_err.error = None;
    AssessmentResult {
        status: AssessmentStatus::LATERAL_MOVE,
        priority: priority_str(priority).to_string(),
        priority_source,
        current: empty_no_err.clone(),
        proposed: empty_no_err,
        score_deltas: HashMap::new(),
        metric_deltas: HashMap::new(),
        structural_distance: None,
        similarity: None,
        coupling_available_for_proposed: false,
        baseline_hash: None,
        current_hash: None,
        warnings: Vec::new(),
        agent_contract: Some(AgentContract {
            next_tool: None,
            next_actions: Vec::new(),
            blocked_by: vec![blocked_by.to_string()],
            verification_gates: Vec::new(),
            risk_flags: vec![blocked_by.to_string()],
        }),
        suspicion_reason: None,
        regression_diff: None,
        error: Some(msg),
    }
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

fn status_meaning(status: AssessmentStatus) -> &'static str {
    match status {
        AssessmentStatus::IMPROVEMENT => "moved up the lattice",
        AssessmentStatus::IMPROVEMENT_SCORE => "same verdict, scores improved",
        AssessmentStatus::LATERAL_MOVE => "no verdict or score movement",
        AssessmentStatus::REGRESSION => "moved down the lattice",
        AssessmentStatus::REGRESSION_SCORE => "same verdict, scores regressed",
        AssessmentStatus::SUSPICIOUS_NO_STRUCTURAL_CHANGE => {
            "scores moved but the AST barely changed"
        }
    }
}

/// The "## Agent Contract" section of the assessment markdown report.
fn push_assessment_agent_contract_lines(lines: &mut Vec<String>, r: &AssessmentResult) {
    let Some(contract) = &r.agent_contract else {
        return;
    };
    if contract.next_tool.is_none()
        && contract.next_actions.is_empty()
        && contract.blocked_by.is_empty()
    {
        return;
    }
    lines.push(String::new());
    lines.push("## Agent Contract".to_string());
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

pub(crate) fn render_assessment_md(r: &AssessmentResult) -> String {
    if let Some(err) = &r.error {
        return format!("**Error:** {err}");
    }
    let mut lines = vec![
        format!(
            "**Status:** {} — {}",
            r.status.as_str(),
            status_meaning(r.status)
        ),
        format!("**Priority:** `{}`", r.priority),
        format!(
            "**Verdict:** {} → {}",
            r.current.lattice_element.as_str(),
            r.proposed.lattice_element.as_str()
        ),
    ];
    if let Some(distance) = r.structural_distance {
        let sim = r
            .similarity
            .map(|s| format!(", similarity {s:.3}"))
            .unwrap_or_default();
        lines.push(format!("**Structural distance:** {distance:.3}{sim}"));
    }
    push_assessment_agent_contract_lines(&mut lines, r);
    if !r.score_deltas.is_empty() {
        let mut pairs: Vec<_> = r.score_deltas.iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        let deltas = pairs
            .iter()
            .map(|(k, v)| format!("{k}={v:+.1}"))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("**Score deltas:** {deltas}"));
    }
    let mut moved: Vec<_> = r.metric_deltas.iter().filter(|(_, &d)| d != 0.0).collect();
    if !moved.is_empty() {
        moved.sort_by(|a, b| a.0.cmp(b.0));
        let md = moved
            .iter()
            .map(|(m, d)| format!("`{m}`={d:+.3}"))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("**Metric deltas:** {md}"));
    }
    if let Some(suspicion) = &r.suspicion_reason {
        lines.push(format!("> ⚠️ {suspicion}"));
    }
    if let Some(diff) = &r.regression_diff {
        lines.push(String::new());
        lines.push("## Regression diff".to_string());
        lines.push("```diff".to_string());
        lines.push(diff.clone());
        lines.push("```".to_string());
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Baseline loading
// ---------------------------------------------------------------------------

struct Baseline {
    source: String,
    coupling: bool,
    warnings: Vec<String>,
    dep_graph: Option<topos_engine::graphs::mdg::object::ModuleDependencyGraph>,
}

fn load_baseline(params: &AssessImprovementInput) -> Result<Baseline, String> {
    if let Some(filepath) = &params.filepath {
        let resolved = resolve_within_root(filepath)?;
        if !resolved.is_file() {
            return Err(format!("Path is not a file: {}", resolved.display()));
        }
        let current_src = read_safe_utf8_file(filepath)?;
        let project_root = resolve_file_root()?;
        let gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir.as_deref(), &project_root);
        let (dep_graph, load_error) =
            load_dep_graph(gitnexus_dir.as_deref(), &resolved.to_string_lossy());
        let warnings = gitnexus_warnings(
            params.gitnexus_dir.as_deref(),
            &project_root,
            gitnexus_dir.as_deref(),
            dep_graph.is_some(),
            load_error.as_deref(),
        );
        Ok(Baseline {
            source: current_src,
            coupling: dep_graph.is_some(),
            warnings,
            dep_graph,
        })
    } else if let Some(current_code) = &params.current_code {
        Ok(Baseline {
            source: current_code.clone(),
            coupling: false,
            warnings: vec![
                "COMPOSABLE not scored — current_code mode has no filepath or \
                 ModuleDependencyGraph context."
                    .to_string(),
            ],
            dep_graph: None,
        })
    } else {
        Err("Provide either `filepath` or `current_code`.".to_string())
    }
}

fn load_proposed_source(params: &AssessImprovementInput) -> Result<String, String> {
    if let Some(code) = &params.proposed_code {
        return Ok(code.clone());
    }
    if let Some(filepath) = &params.proposed_filepath {
        return read_safe_utf8_file(filepath);
    }
    Err("Provide exactly one of `proposed_code` or `proposed_filepath`.".to_string())
}

/// Read the current on-disk file and assess it against `baseline_src`.
#[allow(clippy::too_many_arguments)]
fn assess_edit_in_place(
    baseline_src: String,
    resolved_path: &Path,
    gitnexus_dir_override: Option<&str>,
    priority: Priority,
    priority_source: PrioritySource,
    prefs: Option<&topos_engine::evaluation::preferences::UserPreferences>,
    allow: Vec<String>,
    include_security_findings: bool,
    extra_warnings: Vec<String>,
) -> AssessmentResult {
    let current_src = match read_safe_utf8_file(&resolved_path.to_string_lossy()) {
        Ok(src) => src,
        Err(err) => return err_assessment(priority, priority_source, err, "file_not_found"),
    };
    let project_root = match resolve_file_root() {
        Ok(root) => root,
        Err(err) => return err_assessment(priority, priority_source, err, "assessment_error"),
    };
    let gitnexus_dir = resolve_gitnexus_dir(gitnexus_dir_override, &project_root);
    let (dep_graph, load_error) =
        load_dep_graph(gitnexus_dir.as_deref(), &resolved_path.to_string_lossy());
    let mut warnings = extra_warnings;
    warnings.extend(gitnexus_warnings(
        gitnexus_dir_override,
        &project_root,
        gitnexus_dir.as_deref(),
        dep_graph.is_some(),
        load_error.as_deref(),
    ));
    assess_core(AssessCoreArgs {
        baseline_src,
        proposed_src: current_src,
        language: detect_language(resolved_path).to_string(),
        priority,
        priority_source,
        prefs,
        dep_graph: dep_graph.as_ref(),
        coupling_for_proposed: dep_graph.is_some(),
        file_path: Some(resolved_path.to_path_buf()),
        allow,
        include_security_findings,
        warnings,
    })
}

// ---------------------------------------------------------------------------
// Git helpers
// ---------------------------------------------------------------------------

/// Read `<ref>:<rel_path>` from git. Returns `Ok(source)` or
/// `Err(blocked_by_code)`.
fn git_show(repo_root: &Path, git_ref: &str, rel_path: &str) -> Result<String, &'static str> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["show", &format!("{git_ref}:{rel_path}")])
        .output();
    match output {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err("git_unavailable"),
        Err(_) => Err("baseline_ref_not_found"),
        Ok(out) if !out.status.success() => Err("baseline_ref_not_found"),
        Ok(out) => Ok(String::from_utf8_lossy(&out.stdout).to_string()),
    }
}

/// Whether `git_ref` resolves to a commit in `repo_root`. Returns `true`
/// when git cannot run so the caller falls through to per-file handling.
fn ref_exists(repo_root: &Path, git_ref: &str) -> bool {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{git_ref}^{{commit}}"),
        ])
        .output();
    match output {
        Ok(out) => out.status.success(),
        Err(_) => true,
    }
}

// ---------------------------------------------------------------------------
// Snapshot / changeset helpers
// ---------------------------------------------------------------------------

fn priority_from_str(s: &str) -> Option<Priority> {
    match s {
        "simple" => Some(Priority::Simple),
        "composable" => Some(Priority::Composable),
        "secure" => Some(Priority::Secure),
        _ => None,
    }
}

fn generator_input_from_str(s: &str) -> Option<GeneratorInput> {
    match s {
        "simple" => Some(GeneratorInput::Simple),
        "composable" => Some(GeneratorInput::Composable),
        "secure" => Some(GeneratorInput::Secure),
        _ => None,
    }
}

fn lattice_element_from_str(s: &str) -> Option<LatticeElement> {
    match s {
        "SLOP" => Some(LatticeElement::SLOP),
        "SIMPLE" => Some(LatticeElement::SIMPLE),
        "COMPOSABLE" => Some(LatticeElement::COMPOSABLE),
        "SECURE" => Some(LatticeElement::SECURE),
        "SIMPLE_COMPOSABLE" => Some(LatticeElement::SIMPLE_COMPOSABLE),
        "SIMPLE_SECURE" => Some(LatticeElement::SIMPLE_SECURE),
        "COMPOSABLE_SECURE" => Some(LatticeElement::COMPOSABLE_SECURE),
        "IDEAL" => Some(LatticeElement::IDEAL),
        _ => None,
    }
}

fn priority_from_meta(
    meta: &HashMap<String, Value>,
) -> (
    Priority,
    PrioritySource,
    Option<topos_engine::evaluation::preferences::UserPreferences>,
) {
    let ranking = meta.get("ranking").and_then(Value::as_array);
    match ranking {
        None => {
            let priority = meta
                .get("priority")
                .and_then(Value::as_str)
                .and_then(priority_from_str)
                .unwrap_or(Priority::Simple);
            (priority, PrioritySource::Default, None)
        }
        Some(ranking) => {
            let generators: Vec<GeneratorInput> = ranking
                .iter()
                .filter_map(Value::as_str)
                .filter_map(generator_input_from_str)
                .collect();
            let target = meta
                .get("target")
                .and_then(Value::as_str)
                .and_then(lattice_element_from_str);
            let prefs_input = UserPreferencesInput {
                ranking: generators,
                target,
            };
            let priority = prefs_input.to_priority();
            let prefs = prefs_input.to_preferences().ok();
            (priority, PrioritySource::Preferences, prefs)
        }
    }
}

fn is_complexity_relocated(metric_deltas: &HashMap<String, f64>) -> bool {
    let func_delta = metric_deltas
        .get("ast.max_function_complexity")
        .copied()
        .unwrap_or(0.0);
    let file_delta = metric_deltas.get("cfg.cyclomatic").copied().unwrap_or(0.0);
    func_delta < 0.0 && file_delta > 0.0
}

struct RollupOut {
    dims: HashMap<String, LatticeElement>,
    scores: HashMap<String, f64>,
    achieved: HashMap<String, bool>,
}

fn dim_value(dim: &str) -> Option<EvaluationValue> {
    match dim {
        "simple" => Some(EvaluationValue::Simple),
        "composable" => Some(EvaluationValue::Composable),
        "secure" => Some(EvaluationValue::Secure),
        _ => None,
    }
}

fn rollup(evals: &[EvaluationResult]) -> RollupOut {
    let n = evals.len();
    let mut ok_count: HashMap<String, usize> = HashMap::new();
    let mut present_count: HashMap<String, usize> = HashMap::new();
    let mut min_scores: HashMap<String, f64> = HashMap::new();

    for e in evals {
        for (dim, verdict) in &e.dimensions {
            if dim_value(dim).is_none() {
                continue;
            }
            *present_count.entry(dim.clone()).or_insert(0) += 1;
            let passed = e.is_parseable && *verdict != LatticeElement::SLOP;
            *ok_count.entry(dim.clone()).or_insert(0) += usize::from(passed);
        }
        for (dim, &score) in &e.scores {
            let entry = min_scores.entry(dim.clone()).or_insert(f64::INFINITY);
            *entry = entry.min(score);
        }
    }

    let mut dims = HashMap::new();
    let mut achieved = HashMap::new();
    for (dim, &present) in &present_count {
        let ok = present == n && ok_count.get(dim).copied().unwrap_or(0) == n;
        achieved.insert(dim.clone(), ok);
        dims.insert(
            dim.clone(),
            if ok {
                lattice_to_str(dim_value(dim).unwrap())
            } else {
                LatticeElement::SLOP
            },
        );
    }
    let scores = min_scores
        .into_iter()
        .map(|(dim, s)| (dim, (s * 10.0).round() / 10.0))
        .collect();
    RollupOut {
        dims,
        scores,
        achieved,
    }
}

fn aggregate(achieved: &HashMap<String, bool>) -> LatticeElement {
    lattice_to_str(verdict_from_generators(
        achieved.get("simple").copied().unwrap_or(false),
        achieved.get("composable").copied().unwrap_or(false),
        achieved.get("secure").copied().unwrap_or(false),
    ))
}

pub(crate) fn render_snapshot_md(r: &SnapshotResult) -> String {
    if let Some(err) = &r.error {
        return format!("**Error:** {err}");
    }
    format!(
        "**Snapshot captured:** `{}`\n**File:** `{}`\n\nEdit the file in place, then call \
         `topos_assess_snapshot(snapshot_id=\"{}\", filepath=\"{}\")`.",
        r.snapshot_id, r.filepath, r.snapshot_id, r.filepath
    )
}

fn err_snapshot(filepath: &str, msg: String) -> SnapshotResult {
    SnapshotResult {
        snapshot_id: String::new(),
        filepath: filepath.to_string(),
        baseline_hash: String::new(),
        created_at: 0.0,
        warnings: Vec::new(),
        agent_contract: Some(AgentContract {
            next_tool: None,
            next_actions: Vec::new(),
            blocked_by: vec!["snapshot_error".to_string()],
            verification_gates: Vec::new(),
            risk_flags: vec!["snapshot_error".to_string()],
        }),
        error: Some(msg),
    }
}

pub(crate) fn render_changeset_md(r: &ChangesetResult) -> String {
    if let Some(err) = &r.error {
        return format!("**Error:** {err}");
    }
    let mut lines = vec![format!(
        "**Changeset vs `{}`** — {} → {}",
        r.baseline_ref,
        r.aggregate_before.as_str(),
        r.aggregate_after.as_str()
    )];
    if r.project_regression {
        lines.push("> Project rollup REGRESSED.".to_string());
    }
    lines.push(String::new());
    lines.push("## Files".to_string());
    lines.push("| File | Status | Before | After | Relocated |".to_string());
    lines.push("| --- | --- | --- | --- | --- |".to_string());
    for e in &r.files {
        let before = e.baseline_verdict.map(|v| v.as_str()).unwrap_or("—");
        let after = e.current_verdict.map(|v| v.as_str()).unwrap_or("—");
        let reloc = if e.complexity_relocated_within_file {
            "yes"
        } else {
            ""
        };
        let safe = e.filepath.replace('|', "\\|");
        lines.push(format!(
            "| `{safe}` | {} | {before} | {after} | {reloc} |",
            e.status.as_str()
        ));
    }
    if !r.complexity_relocated_files.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "**Complexity relocated within file:** {}",
            r.complexity_relocated_files
                .iter()
                .map(|f| format!("`{f}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let new_slop: Vec<&String> = r
        .files
        .iter()
        .filter(|e| e.is_new && e.current_verdict == Some(LatticeElement::SLOP))
        .map(|e| &e.filepath)
        .collect();
    if r.project_regression && !new_slop.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "> Rollup floor dragged down by new, not-yet-clean file(s): {} — regression may \
             reflect unfinished new modules, not the edits.",
            new_slop
                .iter()
                .map(|f| format!("`{f}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

#[tool_router(router = assess_router, vis = "pub(crate)")]
impl ToposServer {
    /// Compare a baseline to a side-by-side proposed variant (read-only).
    ///
    /// For normal edit-in-place loops, use `topos_assess_worktree_change`
    /// or snapshot first with `topos_begin_refactor` then
    /// `topos_assess_snapshot`. This tool is for variants supplied as
    /// `proposed_code` or `proposed_filepath`. Returns an AssessmentResult
    /// with `status` and score/metric deltas.
    #[tool(
        name = "topos_assess_improvement",
        annotations(
            title = "Topos Refactor Assessment",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_assess_improvement(
        &self,
        Parameters(params): Parameters<AssessImprovementInput>,
    ) -> CallToolResult {
        let (priority, priority_source) = resolve_priority(params.preferences.as_ref());
        if let Err(msg) = params.validate() {
            let model = err_assessment(priority, priority_source, msg, "assessment_error");
            return to_tool_result(&model, render_assessment_md(&model));
        }
        let baseline = match load_baseline(&params) {
            Ok(b) => b,
            Err(exc) => {
                let model = err_assessment(priority, priority_source, exc, "assessment_error");
                return to_tool_result(&model, render_assessment_md(&model));
            }
        };
        let proposed_src = match load_proposed_source(&params) {
            Ok(src) => src,
            Err(exc) => {
                let model = err_assessment(priority, priority_source, exc, "assessment_error");
                return to_tool_result(&model, render_assessment_md(&model));
            }
        };
        let prefs = match params.preferences.as_ref().map(|p| p.to_preferences()) {
            Some(Err(exc)) => {
                let model = err_assessment(priority, priority_source, exc, "assessment_error");
                return to_tool_result(&model, render_assessment_md(&model));
            }
            Some(Ok(p)) => Some(p),
            None => None,
        };
        let file_path = params
            .filepath
            .as_ref()
            .and_then(|f| resolve_within_root(f).ok());

        let model = assess_core(AssessCoreArgs {
            baseline_src: baseline.source,
            proposed_src,
            language: params.language.clone(),
            priority,
            priority_source,
            prefs: prefs.as_ref(),
            dep_graph: baseline.dep_graph.as_ref(),
            coupling_for_proposed: baseline.coupling,
            file_path,
            allow: params.allow.clone(),
            include_security_findings: params.include_security_findings,
            warnings: baseline.warnings,
        });
        to_tool_result(&model, render_assessment_md(&model))
    }

    /// Assess an in-place edit against a git revision — the common refactor
    /// loop.
    ///
    /// Stateless: the baseline is read from git (`git show
    /// <baseline_ref>:<path>`, default `HEAD`) and compared to the current
    /// working-tree file. No prior call required. For untracked/new files
    /// or an uncommitted pre-edit baseline, use `topos_begin_refactor` +
    /// `topos_assess_snapshot`.
    #[tool(
        name = "topos_assess_worktree_change",
        annotations(
            title = "Topos Refactor Assessment",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_assess_worktree_change(
        &self,
        Parameters(params): Parameters<AssessWorktreeChangeInput>,
    ) -> CallToolResult {
        let (priority, priority_source) = resolve_priority(params.preferences.as_ref());
        let resolved = match resolve_within_root(&params.filepath) {
            Ok(path) => path,
            Err(err) => {
                let model = err_assessment(priority, priority_source, err, "assessment_error");
                return to_tool_result(&model, render_assessment_md(&model));
            }
        };
        let Some(git_root) = find_git_root(&resolved) else {
            let model = err_assessment(
                priority,
                priority_source,
                format!("Not inside a git repository: {}", resolved.display()),
                "not_a_git_repo",
            );
            return to_tool_result(&model, render_assessment_md(&model));
        };
        let rel_path = resolved
            .strip_prefix(&git_root)
            .unwrap_or(&resolved)
            .to_string_lossy()
            .replace('\\', "/");
        let baseline_src = match git_show(&git_root, &params.baseline_ref, &rel_path) {
            Ok(src) => src,
            Err(code) => {
                let model = err_assessment(
                    priority,
                    priority_source,
                    format!(
                        "Could not read `{rel_path}` at ref `{}`.",
                        params.baseline_ref
                    ),
                    code,
                );
                return to_tool_result(&model, render_assessment_md(&model));
            }
        };
        let prefs = params
            .preferences
            .as_ref()
            .and_then(|p| p.to_preferences().ok());
        let model = assess_edit_in_place(
            baseline_src,
            &resolved,
            params.gitnexus_dir.as_deref(),
            priority,
            priority_source,
            prefs.as_ref(),
            params.allow.clone(),
            params.include_security_findings,
            Vec::new(),
        );
        to_tool_result(&model, render_assessment_md(&model))
    }

    /// Persist the file's current source as a baseline snapshot before you
    /// edit it (writes a snapshot record — its only side effect).
    ///
    /// Returns a `snapshot_id`; edit the file in place, then call
    /// `topos_assess_snapshot(snapshot_id, filepath)`. Use this when the
    /// baseline is not a committed git revision.
    #[tool(
        name = "topos_begin_refactor",
        annotations(
            title = "Topos Begin Refactor",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_begin_refactor(
        &self,
        Parameters(params): Parameters<BeginRefactorInput>,
    ) -> CallToolResult {
        let resolved = match resolve_within_root(&params.filepath) {
            Ok(path) => path,
            Err(err) => {
                let model = err_snapshot(&params.filepath, err);
                return to_tool_result(&model, render_snapshot_md(&model));
            }
        };
        if !resolved.is_file() {
            let model = err_snapshot(
                &params.filepath,
                format!("Path is not a file: {}", resolved.display()),
            );
            return to_tool_result(&model, render_snapshot_md(&model));
        }
        let baseline_src = match read_safe_utf8_file(&params.filepath) {
            Ok(src) => src,
            Err(err) => {
                let model = err_snapshot(&params.filepath, err);
                return to_tool_result(&model, render_snapshot_md(&model));
            }
        };
        let (priority, _) = resolve_priority(params.preferences.as_ref());
        let mut meta: HashMap<String, Value> = HashMap::new();
        meta.insert(
            "filepath".to_string(),
            Value::from(resolved.to_string_lossy().to_string()),
        );
        meta.insert("priority".to_string(), Value::from(priority_str(priority)));
        meta.insert(
            "ranking".to_string(),
            params
                .preferences
                .as_ref()
                .map(|p| {
                    Value::from(
                        p.ranking
                            .iter()
                            .map(|g| Value::from(g.as_str()))
                            .collect::<Vec<_>>(),
                    )
                })
                .unwrap_or(Value::Null),
        );
        meta.insert(
            "target".to_string(),
            params
                .preferences
                .as_ref()
                .and_then(|p| p.target)
                .map(|t| Value::from(t.as_str()))
                .unwrap_or(Value::Null),
        );
        meta.insert(
            "gitnexus_dir".to_string(),
            params
                .gitnexus_dir
                .clone()
                .map(Value::from)
                .unwrap_or(Value::Null),
        );
        let created_at = snapshot_now();
        let project_root = match resolve_file_root() {
            Ok(root) => root,
            Err(err) => {
                let model = err_snapshot(&params.filepath, err);
                return to_tool_result(&model, render_snapshot_md(&model));
            }
        };
        let snapshot_id = match write_snapshot(&project_root, &baseline_src, meta, created_at) {
            Ok(id) => id,
            Err(err) => {
                let model = err_snapshot(&params.filepath, err.to_string());
                return to_tool_result(&model, render_snapshot_md(&model));
            }
        };
        let model = SnapshotResult {
            snapshot_id: snapshot_id.clone(),
            filepath: resolved.to_string_lossy().to_string(),
            baseline_hash: sha256_hex(&baseline_src),
            created_at,
            warnings: Vec::new(),
            agent_contract: Some(AgentContract {
                next_tool: Some("topos_assess_snapshot".to_string()),
                next_actions: vec![
                    "edit the file in place, then call topos_assess_snapshot with this \
                     snapshot_id"
                        .to_string(),
                ],
                blocked_by: Vec::new(),
                verification_gates: Vec::new(),
                risk_flags: Vec::new(),
            }),
            error: None,
        };
        to_tool_result(&model, render_snapshot_md(&model))
    }

    /// Assess the current file against a baseline captured by
    /// topos_begin_refactor.
    ///
    /// Loads the stored baseline by `snapshot_id` and compares it to the
    /// current on-disk file, with the same status semantics as
    /// `topos_assess_improvement`. A missing or expired snapshot is
    /// reported via `blocked_by`.
    #[tool(
        name = "topos_assess_snapshot",
        annotations(
            title = "Topos Refactor Assessment",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_assess_snapshot(
        &self,
        Parameters(params): Parameters<AssessSnapshotInput>,
    ) -> CallToolResult {
        let resolved = match resolve_within_root(&params.filepath) {
            Ok(path) => path,
            Err(err) => {
                let model = err_assessment(
                    Priority::Simple,
                    PrioritySource::Default,
                    err,
                    "assessment_error",
                );
                return to_tool_result(&model, render_assessment_md(&model));
            }
        };
        let project_root = match resolve_file_root() {
            Ok(root) => root,
            Err(err) => {
                let model = err_assessment(
                    Priority::Simple,
                    PrioritySource::Default,
                    err,
                    "assessment_error",
                );
                return to_tool_result(&model, render_assessment_md(&model));
            }
        };
        let load = read_snapshot(&project_root, &params.snapshot_id, snapshot_now());
        let (Some(baseline_src), Some(meta)) = (load.baseline_src, load.meta) else {
            let model = err_assessment(
                Priority::Simple,
                PrioritySource::Default,
                format!("Snapshot `{}` is unavailable.", params.snapshot_id),
                load.blocked_by.unwrap_or("snapshot_not_found"),
            );
            return to_tool_result(&model, render_assessment_md(&model));
        };
        if meta.get("filepath").and_then(Value::as_str) != Some(&resolved.to_string_lossy()) {
            let model = err_assessment(
                Priority::Simple,
                PrioritySource::Default,
                format!(
                    "Snapshot was taken from `{}`, not `{}`.",
                    meta.get("filepath").and_then(Value::as_str).unwrap_or(""),
                    resolved.display()
                ),
                "snapshot_stale",
            );
            return to_tool_result(&model, render_assessment_md(&model));
        }
        let (priority, priority_source, prefs) = priority_from_meta(&meta);
        let model = assess_edit_in_place(
            baseline_src,
            &resolved,
            meta.get("gitnexus_dir").and_then(Value::as_str),
            priority,
            priority_source,
            prefs.as_ref(),
            params.allow.clone(),
            params.include_security_findings,
            Vec::new(),
        );
        to_tool_result(&model, render_assessment_md(&model))
    }

    /// Assess a multi-file changeset against a git baseline and roll the
    /// per-file verdicts into a project before/after (read-only).
    ///
    /// Use for a module split or any edit spanning several files. Each file
    /// is compared to `baseline_ref` (new files have no baseline). Flags
    /// `complexity_relocated_within_file` and `project_regression`. Returns
    /// a ChangesetResult.
    #[tool(
        name = "topos_assess_changeset",
        annotations(
            title = "Topos Changeset Assessment",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_assess_changeset(
        &self,
        Parameters(params): Parameters<AssessChangesetInput>,
    ) -> CallToolResult {
        let (priority, priority_source) = resolve_priority(params.preferences.as_ref());
        let prefs = params
            .preferences
            .as_ref()
            .and_then(|p| p.to_preferences().ok());
        let project_root = match resolve_file_root() {
            Ok(root) => root,
            Err(err) => {
                let model = changeset_error(priority, priority_source, &params.baseline_ref, err);
                return to_tool_result(&model, render_changeset_md(&model));
            }
        };
        let gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir.as_deref(), &project_root);

        let mut entries: Vec<ChangesetFileEntry> = Vec::new();
        let mut before_evals: Vec<EvaluationResult> = Vec::new();
        let mut after_evals: Vec<EvaluationResult> = Vec::new();
        let mut any_coupling = false;

        for filepath in &params.files {
            match assess_one_changeset_file(
                filepath,
                &params,
                priority,
                priority_source,
                prefs.as_ref(),
                &project_root,
                gitnexus_dir.as_deref(),
            ) {
                Err(fatal) => {
                    let model =
                        changeset_error(priority, priority_source, &params.baseline_ref, fatal);
                    return to_tool_result(&model, render_changeset_md(&model));
                }
                Ok(outcome) => {
                    entries.push(outcome.entry);
                    any_coupling |= outcome.coupling;
                    if let Some(base) = outcome.baseline_eval {
                        before_evals.push(base);
                    }
                    if let Some(cur) = outcome.current_eval {
                        after_evals.push(cur);
                    }
                }
            }
        }

        let load_error_placeholder = None;
        let warnings = gitnexus_warnings(
            params.gitnexus_dir.as_deref(),
            &project_root,
            gitnexus_dir.as_deref(),
            any_coupling,
            load_error_placeholder,
        );
        let model = build_changeset_result(
            &params,
            priority,
            priority_source,
            entries,
            &before_evals,
            &after_evals,
            any_coupling,
            warnings,
        );
        to_tool_result(&model, render_changeset_md(&model))
    }
}

struct ChangesetFileOutcome {
    entry: ChangesetFileEntry,
    baseline_eval: Option<EvaluationResult>,
    current_eval: Option<EvaluationResult>,
    coupling: bool,
}

fn file_error(filepath: &str, message: String, code: &str) -> ChangesetFileOutcome {
    ChangesetFileOutcome {
        entry: ChangesetFileEntry {
            filepath: filepath.to_string(),
            status: AssessmentStatus::LATERAL_MOVE,
            is_new: false,
            baseline_verdict: None,
            current_verdict: None,
            score_deltas: HashMap::new(),
            metric_deltas: HashMap::new(),
            complexity_relocated_within_file: false,
            warnings: Vec::new(),
            blocked_by: Some(code.to_string()),
            error: Some(message),
        },
        baseline_eval: None,
        current_eval: None,
        coupling: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn assess_one_changeset_file(
    filepath: &str,
    params: &AssessChangesetInput,
    priority: Priority,
    priority_source: PrioritySource,
    prefs: Option<&topos_engine::evaluation::preferences::UserPreferences>,
    project_root: &Path,
    gitnexus_dir: Option<&Path>,
) -> Result<ChangesetFileOutcome, String> {
    let resolved = match resolve_within_root(filepath) {
        Ok(path) => path,
        Err(err) => return Ok(file_error(filepath, err, "path_error")),
    };
    let Some(git_root) = find_git_root(&resolved) else {
        return Ok(file_error(
            filepath,
            "not inside a git repo".to_string(),
            "not_a_git_repo",
        ));
    };
    if !ref_exists(&git_root, &params.baseline_ref) {
        return Err(format!("baseline ref not found: {}", params.baseline_ref));
    }
    let rel_path = resolved
        .strip_prefix(&git_root)
        .unwrap_or(&resolved)
        .to_string_lossy()
        .replace('\\', "/");
    let (baseline_src, is_new) = match git_show(&git_root, &params.baseline_ref, &rel_path) {
        Ok(src) => (src, false),
        Err("git_unavailable") => return Err("git is not available.".to_string()),
        Err(_) => (String::new(), true),
    };
    let current_src = match read_safe_utf8_file(filepath) {
        Ok(src) => src,
        Err(err) => return Ok(file_error(filepath, err, "file_not_found")),
    };
    let (dep_graph, load_error) = load_dep_graph(gitnexus_dir, &resolved.to_string_lossy());
    let warnings = gitnexus_warnings(
        params.gitnexus_dir.as_deref(),
        project_root,
        gitnexus_dir,
        dep_graph.is_some(),
        load_error.as_deref(),
    );
    let assessment = assess_core(AssessCoreArgs {
        baseline_src,
        proposed_src: current_src,
        language: detect_language(&resolved).to_string(),
        priority,
        priority_source,
        prefs,
        dep_graph: dep_graph.as_ref(),
        coupling_for_proposed: dep_graph.is_some(),
        file_path: Some(resolved.clone()),
        allow: params.allow.clone(),
        include_security_findings: params.include_security_findings,
        warnings,
    });

    let relocated = is_complexity_relocated(&assessment.metric_deltas);
    let entry = ChangesetFileEntry {
        filepath: filepath.to_string(),
        status: assessment.status,
        is_new,
        baseline_verdict: (!is_new).then_some(assessment.current.lattice_element),
        current_verdict: Some(assessment.proposed.lattice_element),
        score_deltas: assessment.score_deltas.clone(),
        metric_deltas: assessment.metric_deltas.clone(),
        complexity_relocated_within_file: relocated,
        warnings: assessment.warnings.clone(),
        blocked_by: None,
        error: None,
    };
    Ok(ChangesetFileOutcome {
        entry,
        baseline_eval: (!is_new).then_some(assessment.current),
        current_eval: Some(assessment.proposed),
        coupling: dep_graph.is_some(),
    })
}

#[allow(clippy::too_many_arguments)]
fn build_changeset_result(
    params: &AssessChangesetInput,
    priority: Priority,
    priority_source: PrioritySource,
    entries: Vec<ChangesetFileEntry>,
    before_evals: &[EvaluationResult],
    after_evals: &[EvaluationResult],
    coupling_available: bool,
    warnings: Vec<String>,
) -> ChangesetResult {
    let before = rollup(before_evals);
    let after = rollup(after_evals);

    let project_regression = before
        .achieved
        .iter()
        .any(|(dim, &ok)| ok && !after.achieved.get(dim).copied().unwrap_or(false));
    let relocated_files: Vec<String> = entries
        .iter()
        .filter(|e| e.complexity_relocated_within_file)
        .map(|e| e.filepath.clone())
        .collect();

    let contract = changeset_contract(
        project_regression,
        &relocated_files,
        coupling_available,
        &warnings,
    );

    ChangesetResult {
        baseline_ref: params.baseline_ref.clone(),
        files: entries,
        project_before: before.dims,
        project_after: after.dims,
        project_scores_before: before.scores,
        project_scores_after: after.scores,
        aggregate_before: aggregate(&before.achieved),
        aggregate_after: aggregate(&after.achieved),
        project_regression,
        complexity_relocated_files: relocated_files,
        coupling_available,
        priority: priority_str(priority).to_string(),
        priority_source,
        warnings,
        agent_contract: Some(contract),
        error: None,
    }
}

fn changeset_contract(
    project_regression: bool,
    relocated_files: &[String],
    coupling_available: bool,
    warnings: &[String],
) -> AgentContract {
    let mut blocked_by: Vec<String> = Vec::new();
    let mut risk_flags: Vec<String> = Vec::new();
    let mut next_actions: Vec<String> = Vec::new();

    let composable = composable_contract_signals(coupling_available, warnings, true);
    blocked_by.extend(composable.blocked_by.clone());
    risk_flags.extend(composable.risk_flags.clone());
    if !warnings.is_empty() {
        risk_flags.push("warnings".into());
    }
    if !relocated_files.is_empty() {
        risk_flags.push("complexity_relocated_within_file".into());
        next_actions.push(
            "move extracted logic across a module boundary instead of within one file".into(),
        );
    }
    let next_tool = if let Some(action) = composable.next_action {
        next_actions.push(action);
        composable.next_tool
    } else if project_regression {
        blocked_by.push("project_regression".into());
        risk_flags.push("project_regression".into());
        next_actions.push("revise the split; the project rollup regressed".into());
        Some("topos_inspect_code".to_string())
    } else {
        next_actions.push("run project rollup and behavior checks before accepting".into());
        Some("topos_evaluate_project".to_string())
    };

    AgentContract {
        next_tool,
        next_actions,
        blocked_by,
        verification_gates: vec![
            "no project_regression in the rollup".into(),
            "no complexity_relocated_within_file flags remain".into(),
            "behavior tests or type/lint checks pass when available".into(),
        ],
        risk_flags,
    }
}

fn changeset_error(
    priority: Priority,
    priority_source: PrioritySource,
    baseline_ref: &str,
    message: String,
) -> ChangesetResult {
    ChangesetResult {
        baseline_ref: baseline_ref.to_string(),
        files: Vec::new(),
        project_before: HashMap::new(),
        project_after: HashMap::new(),
        project_scores_before: HashMap::new(),
        project_scores_after: HashMap::new(),
        aggregate_before: LatticeElement::SLOP,
        aggregate_after: LatticeElement::SLOP,
        project_regression: false,
        complexity_relocated_files: Vec::new(),
        coupling_available: false,
        priority: priority_str(priority).to_string(),
        priority_source,
        warnings: Vec::new(),
        agent_contract: Some(AgentContract {
            next_tool: None,
            next_actions: Vec::new(),
            blocked_by: vec!["changeset_error".to_string()],
            verification_gates: Vec::new(),
            risk_flags: vec!["changeset_error".to_string()],
        }),
        error: Some(message),
    }
}
