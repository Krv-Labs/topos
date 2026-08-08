//! Evaluation tools: code string, single file, and whole project.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::omega::{verdict_from_generators, EvaluationValue, Generator};
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::policies::calibration::SIMPLE;
use topos_engine::evaluation::policies::composable::coupling_gate_input;
use topos_engine::evaluation::policies::gates::{evaluate_gates, GateResult};
use topos_engine::evaluation::weakest_score;

use crate::diagnostics::{overlay_for_file, overlay_for_source, SecurityOverlay};
use crate::evaluation::{
    all_source_suffixes, classify_code_string, classify_file, detect_language, ensure_gitnexus_dir,
    gitnexus_warnings, resolve_mcp_composable_project_root, resolve_override_for_root,
};
use crate::formatting::{
    agent_contract_prelude, build_pillars, error_md, finish_agent_contract, render_evaluation_md,
    to_evaluation_result, to_tool_result, AgentContractPreludeInput, EvalResultOptions,
};
use crate::metric_locations::build_metric_locations;
use crate::refactor_targets::build_refactor_targets;
use crate::schemas::{
    lattice_to_str, priority_str, resolve_priority, AgentContract, EvaluateCodeInput,
    EvaluateFileInput, EvaluateProjectInput, EvaluationResult, LatticeElement, PrioritySource,
    ProjectEvaluationResult, ProjectFileEntry, ProjectLanguageRollup, RefactorTarget,
    SecurityFinding, WorstFileEntry,
};
use crate::security::{composable_default_root, read_resolved_utf8, resolve_project_path};
use crate::server::ToposServer;

pub(crate) fn overlay_opts(overlay: Option<&SecurityOverlay>, opts: &mut EvalResultOptions<'_>) {
    if let Some(overlay) = overlay {
        opts.security_findings = overlay.active_findings.clone();
        opts.acknowledged_risks = overlay.acknowledged_risks.clone();
    }
}

fn err_eval(
    description: &str,
    priority: Priority,
    source: PrioritySource,
    msg: String,
) -> CallToolResult {
    let model = EvaluationResult::error_result(description, priority, source, msg);
    to_tool_result(&model, error_md(&model))
}

#[tool_router(router = evaluate_router, vis = "pub(crate)")]
impl ToposServer {
    /// Score a raw code string on the SIMPLE / SECURE / NAVIGABLE quality
    /// lattice (read-only; never writes or runs the code).
    ///
    /// Use for a snippet not yet on disk. SIMPLE, SECURE, and NAVIGABLE are
    /// reachable here (CFG/CPG/UAST); COMPOSABLE needs a module dependency
    /// graph, so for it use `topos_evaluate_file` with `gitnexus_dir`, or
    /// `topos_evaluate_project` for a whole tree.
    /// Returns an EvaluationResult: the lattice verdict (SLOP…IDEAL),
    /// per-generator scores, and a next-step agent contract.
    #[tool(
        name = "topos_evaluate_code",
        annotations(
            title = "Topos Code Evaluation",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn topos_evaluate_code(
        &self,
        Parameters(params): Parameters<EvaluateCodeInput>,
    ) -> CallToolResult {
        let (priority, priority_source) = resolve_priority(params.preferences.as_ref());
        let result = match classify_code_string(&params.code, &params.language, priority) {
            Ok(result) => result,
            Err(exc) => {
                return err_eval("Evaluation failed", Priority::Simple, priority_source, exc)
            }
        };
        let prefs = match params.preferences.as_ref().map(|p| p.to_preferences()) {
            Some(Err(exc)) => {
                return err_eval("Evaluation failed", Priority::Simple, priority_source, exc)
            }
            Some(Ok(p)) => Some(p),
            None => None,
        };
        let overlay =
            overlay_for_source(&params.code, &params.language, &result, None, &params.allow);
        let mut opts = EvalResultOptions::new();
        opts.preferences = prefs.as_ref();
        opts.priority_source = priority_source;
        opts.adjusted_verdict = overlay.as_ref().map(|o| &o.verdict);
        overlay_opts(overlay.as_ref(), &mut opts);
        opts.verbose = params.verbose;
        opts.metric_locations = build_metric_locations(&params.code, &params.language, &result);
        let model = to_evaluation_result(&result, false, opts);
        let md = render_evaluation_md(&model, None, params.verbose);
        to_tool_result(&model, md)
    }

    /// Score a file on disk on the SIMPLE / COMPOSABLE / SECURE / NAVIGABLE
    /// lattice — the only evaluate tool that can reach COMPOSABLE
    /// (side-effecting).
    ///
    /// Unless `no_composable` is set, this generates/refreshes `.gitnexus`
    /// (given by `gitnexus_dir` or auto-detected at `<root>/.gitnexus`) when
    /// it's missing or stale, then attaches the resulting
    /// ModuleDependencyGraph — the same default behavior as the CLI's
    /// `topos evaluate`. SIMPLE/SECURE/NAVIGABLE always run. When GitNexus
    /// isn't installed or generation fails, `coupling_available` is false
    /// and `warnings` explains why; the rest of the evaluation still
    /// succeeds.
    #[tool(
        name = "topos_evaluate_file",
        annotations(
            title = "Topos Code Evaluation",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn topos_evaluate_file(
        &self,
        Parameters(params): Parameters<EvaluateFileInput>,
    ) -> CallToolResult {
        // Generation can shell out to `gitnexus analyze` (bounded by
        // TOPOS_DEPGRAPH_TIMEOUT, default 300s); offload so a slow/first-time
        // run cannot stall the transport, matching topos_evaluate_project.
        match tokio::task::spawn_blocking(move || evaluate_file_sync(params)).await {
            Ok(tool_result) => tool_result,
            Err(join_err) => err_eval(
                "Evaluation failed",
                Priority::Simple,
                PrioritySource::Default,
                format!("file evaluation panicked: {join_err}"),
            ),
        }
    }

    /// Recursively score every supported source file in a directory on the
    /// SIMPLE / COMPOSABLE / SECURE / NAVIGABLE lattice, with a project
    /// rollup (side-effecting).
    ///
    /// Autodetects all supported languages (Python, Rust, JavaScript,
    /// TypeScript, C++, Go) in one walk — no language argument — and skips
    /// unsupported files. The rollup takes the project-wide minimum per
    /// dimension (weakest file floors it). Returns page-global named lists
    /// (`hard_fails`, `leaf_composable_zeros`, `maintainability_giants`)
    /// plus a paginated per-file table (gate failures first); page with
    /// `limit` / `offset`.
    ///
    /// Unless `no_composable` is set, generates/refreshes `.gitnexus` when
    /// missing or stale before scoring, same as `topos_evaluate_file` and
    /// the CLI's `topos evaluate` — `coupling_available`/`warnings` explain
    /// it when that isn't possible, without failing the evaluation.
    #[tool(
        name = "topos_evaluate_project",
        annotations(
            title = "Topos Code Evaluation",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn topos_evaluate_project(
        &self,
        Parameters(params): Parameters<EvaluateProjectInput>,
    ) -> CallToolResult {
        // The walk + per-file classification is CPU-bound; run it off the
        // async executor so a large project cannot stall the transport.
        let result = tokio::task::spawn_blocking(move || evaluate_project_sync(params)).await;
        match result {
            Ok(tool_result) => tool_result,
            Err(join_err) => {
                let (priority, priority_source) = (Priority::Simple, PrioritySource::Default);
                err_eval(
                    "Evaluation failed",
                    priority,
                    priority_source,
                    format!("project evaluation panicked: {join_err}"),
                )
            }
        }
    }
}

fn evaluate_file_sync(params: EvaluateFileInput) -> CallToolResult {
    let (priority, priority_source) = resolve_priority(params.preferences.as_ref());
    let (resolved, detected_project) = match resolve_project_path(&params.filepath) {
        Ok(context) => context,
        Err(err) => {
            return err_eval(
                "Access denied / path error",
                Priority::Simple,
                priority_source,
                err,
            )
        }
    };
    if !resolved.is_file() {
        return err_eval(
            "Not a file",
            Priority::Simple,
            priority_source,
            format!("Path is not a file: {}", resolved.display()),
        );
    }

    let composable_root = composable_default_root(&detected_project);
    let project_root =
        resolve_mcp_composable_project_root(params.gitnexus_dir.as_deref(), &composable_root);
    // Resolved to an absolute path against `composable_root` — must be used below
    // instead of `params.gitnexus_dir`, since `project_root` above already
    // absorbed a relative override's subdirectory; rejoining the original
    // relative string against it a second time would double that
    // subdirectory.
    let resolved_override =
        resolve_override_for_root(params.gitnexus_dir.as_deref(), &composable_root);
    let gitnexus_outcome = ensure_gitnexus_dir(
        resolved_override.as_deref(),
        &project_root,
        params.no_composable,
        /* capture = */ true,
    );
    let gitnexus_dir = gitnexus_outcome.gitnexus_dir;

    let (result, dep_graph, load_error) =
        match classify_file(&resolved, priority, gitnexus_dir.as_deref()) {
            Ok(triple) => triple,
            Err(exc) => {
                return err_eval("Evaluation failed", Priority::Simple, priority_source, exc)
            }
        };

    let prefs = match params.preferences.as_ref().map(|p| p.to_preferences()) {
        Some(Err(exc)) => {
            return err_eval("Evaluation failed", Priority::Simple, priority_source, exc)
        }
        Some(Ok(p)) => Some(p),
        None => None,
    };
    let mut warnings = gitnexus_warnings(
        resolved_override.as_deref(),
        &project_root,
        gitnexus_dir.as_deref(),
        dep_graph.is_some(),
        load_error.as_deref(),
    );
    if let Some(note) = gitnexus_outcome.generation_note {
        warnings.insert(0, note);
    }
    let overlay = overlay_for_file(&resolved, &result, &params.allow);
    let locations = match read_resolved_utf8(&resolved) {
        Ok(source) => build_metric_locations(&source, detect_language(&resolved), &result),
        Err(_) => HashMap::new(),
    };

    // Targets are computed before the result model so the agent contract
    // can route them natively and `binding_constraint` can project the top
    // gating one. This is the only place targets are built: they are a
    // single-file affordance, and `evaluate_single_file` (the project page)
    // deliberately has no equivalent — a few full targets per row would
    // outweigh the rows.
    let targets: Option<Vec<RefactorTarget>> = if params.refactor_targets > 0 {
        Some(build_refactor_targets(
            &resolved.to_string_lossy(),
            &result,
            overlay
                .as_ref()
                .map(|o| o.active_findings.as_slice())
                .unwrap_or(&[]),
            &locations,
            params.preferences.as_ref().map(|p| p.ranking.as_slice()),
            params.refactor_targets.min(25),
        ))
    } else {
        None
    };

    let mut opts = EvalResultOptions::new();
    opts.preferences = prefs.as_ref();
    opts.priority_source = priority_source;
    opts.warnings = warnings;
    opts.adjusted_verdict = overlay.as_ref().map(|o| &o.verdict);
    overlay_opts(overlay.as_ref(), &mut opts);
    opts.verbose = params.verbose;
    opts.metric_locations = locations;
    // Only reachable via an explicit `refactor_targets=0`, now that the
    // parameter defaults to a non-zero count.
    opts.offer_refactor_targets = targets.is_none();
    opts.refactor_targets = targets;
    opts.include_security_findings = params.include_security_findings;
    let model = to_evaluation_result(&result, dep_graph.is_some(), opts);
    let md = render_evaluation_md(&model, None, params.verbose);
    to_tool_result(&model, md)
}

fn evaluate_project_sync(params: EvaluateProjectInput) -> CallToolResult {
    let (priority, priority_source) = resolve_priority(params.preferences.as_ref());

    let (resolved_root, source_files) = match validate_and_collect_project(&params) {
        Ok(pair) => pair,
        Err(msg) => {
            let model = empty_project_result(&params, priority, priority_source, Some(msg));
            let md = render_project_md(&model);
            return to_tool_result(&model, md);
        }
    };

    let (_requested_path, detected_project) = match resolve_project_path(&params.path) {
        Ok(context) => context,
        Err(err) => {
            let model = empty_project_result(&params, priority, priority_source, Some(err));
            let md = render_project_md(&model);
            return to_tool_result(&model, md);
        }
    };
    let composable_root = composable_default_root(&detected_project);
    let project_root =
        resolve_mcp_composable_project_root(params.gitnexus_dir.as_deref(), &composable_root);
    // See the matching comment in evaluate_file_sync: must use the resolved
    // override below, not `params.gitnexus_dir`, since a relative override's
    // subdirectory is already baked into `project_root` above.
    let resolved_override =
        resolve_override_for_root(params.gitnexus_dir.as_deref(), &composable_root);
    let gitnexus_outcome = ensure_gitnexus_dir(
        resolved_override.as_deref(),
        &project_root,
        params.no_composable,
        /* capture = */ true,
    );
    let gitnexus_dir = gitnexus_outcome.gitnexus_dir;
    let coupling_available = gitnexus_dir.is_some();

    let mut per_file_results: Vec<ClassificationResult> = Vec::new();
    let mut entries: Vec<ProjectFileEntry> = Vec::new();
    let mut parse_failures = 0usize;
    let mut any_dep_graph_loaded = false;
    let mut last_load_error: Option<String> = None;
    let mut per_language_results: HashMap<String, Vec<ClassificationResult>> = HashMap::new();
    let mut per_language_entries: HashMap<String, Vec<ProjectFileEntry>> = HashMap::new();
    let mut per_language_parse_failures: HashMap<String, usize> = HashMap::new();

    for path in &source_files {
        let language = detect_language(path).to_string();
        match evaluate_single_file(
            path,
            &resolved_root,
            priority,
            gitnexus_dir.as_deref(),
            params.include_security_findings,
            params.verbose,
            &params.allow,
        ) {
            Err(_) => {
                parse_failures += 1;
                *per_language_parse_failures.entry(language).or_default() += 1;
            }
            Ok((result, entry, failed, has_dep, load_error)) => {
                if failed {
                    parse_failures += 1;
                    *per_language_parse_failures
                        .entry(language.clone())
                        .or_default() += 1;
                }
                any_dep_graph_loaded |= has_dep;
                if load_error.is_some() {
                    last_load_error = load_error;
                }
                per_file_results.push(result.clone());
                entries.push(entry.clone());
                per_language_results
                    .entry(language.clone())
                    .or_default()
                    .push(result);
                per_language_entries
                    .entry(language)
                    .or_default()
                    .push(entry);
            }
        }
    }

    let model = build_project_result(BuildProjectArgs {
        resolved_root: &resolved_root,
        source_file_count: source_files.len(),
        parse_failures,
        per_file_results,
        entries,
        any_dep_graph_loaded,
        last_load_error,
        per_language_results,
        per_language_entries,
        per_language_parse_failures,
        params: &params,
        priority,
        priority_source,
        coupling_available,
        project_root: &project_root,
        resolved_gitnexus_override: resolved_override.as_deref(),
        gitnexus_dir: gitnexus_dir.as_deref(),
        generation_note: gitnexus_outcome.generation_note,
    });
    let md = render_project_md(&model);
    to_tool_result(&model, md)
}

fn adjusted_result(
    result: &ClassificationResult,
    overlay: Option<&SecurityOverlay>,
) -> ClassificationResult {
    let Some(overlay) = overlay else {
        return result.clone();
    };
    let mut dimensions = result.dimensions.clone();
    let pass = overlay.verdict.adjusted_secure_pass;
    dimensions.insert(
        "secure".to_string(),
        if pass {
            EvaluationValue::Secure
        } else {
            EvaluationValue::Slop
        },
    );
    ClassificationResult {
        is_parseable: result.is_parseable,
        dimensions,
        scores: result.scores.clone(),
        lattice_element: overlay.verdict.adjusted_element,
        priority: result.priority,
        raw_metrics: result.raw_metrics.clone(),
        interpretation: result.interpretation.clone(),
        is_entrypoint_module: result.is_entrypoint_module,
        is_stable_leaf_module: result.is_stable_leaf_module,
    }
}

type SingleFileOutcome = (
    ClassificationResult,
    ProjectFileEntry,
    bool,
    bool,
    Option<String>,
);

/// Score one file and shape its project-page row.
///
/// `verbose` gates `raw_metrics` on the row exactly as
/// [`crate::formatting::to_evaluation_result`] does for the single-file
/// tool: the project markdown renderer already respects the same flag
/// (`render_project_entry`), so shipping the floats unconditionally in the
/// structured channel made the two channels disagree — and at the default
/// page size the raw metrics were the largest block in the response.
fn evaluate_single_file(
    path: &Path,
    resolved_root: &Path,
    priority: Priority,
    gitnexus_dir: Option<&Path>,
    include_security_findings: bool,
    verbose: bool,
    allows: &[String],
) -> Result<SingleFileOutcome, String> {
    let (result, dep_graph, load_error) = classify_file(path, priority, gitnexus_dir)?;

    let is_parse_failure = !result.is_parseable;
    let overlay = overlay_for_file(path, &result, allows);
    let result_for_rollup = adjusted_result(&result, overlay.as_ref());

    let findings: Vec<SecurityFinding> = overlay
        .as_ref()
        .map(|o| o.active_findings.clone())
        .unwrap_or_default();
    let adjusted = overlay.as_ref().map(|o| &o.verdict);
    let entry = ProjectFileEntry {
        filepath: path
            .strip_prefix(resolved_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string(),
        language: detect_language(path).to_string(),
        lattice_element: lattice_to_str(result_for_rollup.summary()),
        scores: result
            .scores
            .iter()
            .map(|(dim, s)| (dim.clone(), (s * 1000.0).round() / 10.0))
            .collect(),
        pillars: build_pillars(&result_for_rollup, dep_graph.is_some()),
        raw_metrics: if verbose {
            result.raw_metrics.clone()
        } else {
            HashMap::new()
        },
        warnings: Vec::new(),
        security_findings: if include_security_findings {
            findings
        } else {
            Vec::new()
        },
        acknowledged_risks: overlay
            .as_ref()
            .map(|o| o.acknowledged_risks.clone())
            .unwrap_or_default(),
        raw_lattice_element: adjusted.map(|v| lattice_to_str(v.raw_element)),
        adjusted_lattice_element: adjusted.map(|v| lattice_to_str(v.adjusted_element)),
        secure_raw: adjusted.map(|v| v.raw_secure_pass),
        secure_adjusted: adjusted.map(|v| v.adjusted_secure_pass),
        grade_capped: adjusted.map(|v| v.grade_capped).unwrap_or(false),
        is_parseable: result.is_parseable,
    };
    Ok((
        result_for_rollup,
        entry,
        is_parse_failure,
        dep_graph.is_some(),
        load_error,
    ))
}

fn validate_and_collect_project(
    params: &EvaluateProjectInput,
) -> Result<(PathBuf, Vec<PathBuf>), String> {
    let (resolved_root, _) = resolve_project_path(&params.path)?;
    if !resolved_root.is_dir() {
        return Err(format!(
            "Path is not a directory: {}",
            resolved_root.display()
        ));
    }
    let suffixes = all_source_suffixes();
    let source_files =
        topos_engine::adapters::discovery::collect_source_files(&[&resolved_root], &suffixes, true);
    if source_files.is_empty() {
        return Err("No supported source files found.".to_string());
    }
    Ok((resolved_root, source_files))
}

fn min_scores_by_dim(results: &[ClassificationResult]) -> HashMap<String, f64> {
    let mut min_scores: HashMap<String, f64> = HashMap::new();
    for r in results {
        for (dim, &s) in &r.scores {
            let entry = min_scores.entry(dim.clone()).or_insert(f64::INFINITY);
            *entry = entry.min(s);
        }
    }
    min_scores
        .into_iter()
        .map(|(dim, s)| (dim, (s * 1000.0).round() / 10.0))
        .collect()
}

fn aggregate_floor_verdict(rolled: &HashMap<String, EvaluationValue>) -> LatticeElement {
    let satisfied: Vec<Generator> = Generator::ALL
        .into_iter()
        .filter(|g| rolled.get(g.as_str()) == Some(&g.value()))
        .collect();
    lattice_to_str(verdict_from_generators(&satisfied))
}

#[derive(Clone)]
struct ScoredProjectRow {
    entry: ProjectFileEntry,
    result: ClassificationResult,
}

/// Gate-input map for a file — mirrors the scorers and suggestion engine.
fn gate_metrics_for(result: &ClassificationResult) -> HashMap<String, f64> {
    let instability = result.raw_metrics.get("mdg.instability").copied();
    let fan_in = result.raw_metrics.get("mdg.fan_in").copied();
    let fan_out = result.raw_metrics.get("mdg.fan_out").copied();
    let mut gate_metrics = result.raw_metrics.clone();
    gate_metrics.remove("mdg.instability");
    gate_metrics.extend(coupling_gate_input(
        instability,
        fan_in,
        fan_out,
        result.raw_metrics.get("mdg.abstractness").copied(),
    ));
    gate_metrics
}

fn has_coupling_signal(fan_in: Option<f64>, fan_out: Option<f64>) -> bool {
    !(fan_in == Some(0.0) && fan_out == Some(0.0))
}

/// `mdg.instability = 0.0` with no measured coupling — a structural leaf.
fn is_leaf_composable_zero(result: &ClassificationResult) -> bool {
    if !result.is_parseable {
        return false;
    }
    if result.raw_metrics.get("mdg.instability").copied() != Some(0.0) {
        return false;
    }
    let fan_in = result.raw_metrics.get("mdg.fan_in").copied();
    let fan_out = result.raw_metrics.get("mdg.fan_out").copied();
    !has_coupling_signal(fan_in, fan_out)
}

fn gating_gate_failures(result: &ClassificationResult) -> Vec<GateResult> {
    let gate_metrics = gate_metrics_for(result);
    let instability = result.raw_metrics.get("mdg.instability").copied();
    evaluate_gates(
        &gate_metrics,
        None,
        result.is_entrypoint_module,
        result.is_stable_leaf_module,
        instability,
    )
    .into_iter()
    .filter(|r| r.spec.gates_achieved && !r.passed())
    .collect()
}

fn is_hard_fail(result: &ClassificationResult) -> bool {
    if !result.is_parseable {
        return true;
    }
    let failures = gating_gate_failures(result);
    if failures.is_empty() {
        return false;
    }
    if is_leaf_composable_zero(result) {
        return failures.iter().any(|r| r.spec.pillar != "composable");
    }
    true
}

fn is_maintainability_giant(result: &ClassificationResult) -> bool {
    if !result.is_parseable || is_hard_fail(result) || is_leaf_composable_zero(result) {
        return false;
    }
    gate_metrics_for(result)
        .get("cfg.cyclomatic")
        .is_some_and(|v| *v > SIMPLE.max_cyclomatic)
}

fn weakest_score_from_result(result: &ClassificationResult) -> f64 {
    let scores_pct: HashMap<String, f64> = result
        .scores
        .iter()
        .map(|(dim, s)| (dim.clone(), s * 100.0))
        .collect();
    weakest_score(&scores_pct)
}

fn hard_fail_sort_key(result: &ClassificationResult) -> (usize, f64) {
    let failures = gating_gate_failures(result);
    let count = if is_leaf_composable_zero(result) {
        failures
            .iter()
            .filter(|r| r.spec.pillar != "composable")
            .count()
    } else {
        failures.len()
    };
    (count, weakest_score_from_result(result))
}

fn to_worst_entry(entry: &ProjectFileEntry) -> WorstFileEntry {
    WorstFileEntry {
        filepath: entry.filepath.clone(),
        lattice_element: entry.lattice_element,
    }
}

fn classify_project_rows(
    rows: &[ScoredProjectRow],
) -> (
    Vec<WorstFileEntry>,
    Vec<WorstFileEntry>,
    Vec<WorstFileEntry>,
) {
    let mut hard: Vec<&ScoredProjectRow> = rows
        .iter()
        .filter(|row| is_hard_fail(&row.result))
        .collect();
    hard.sort_by(|a, b| {
        let (ca, sa) = hard_fail_sort_key(&a.result);
        let (cb, sb) = hard_fail_sort_key(&b.result);
        cb.cmp(&ca)
            .then_with(|| sa.partial_cmp(&sb).unwrap_or(Ordering::Equal))
    });

    let mut leaves: Vec<&ScoredProjectRow> = rows
        .iter()
        .filter(|row| is_leaf_composable_zero(&row.result))
        .collect();
    leaves.sort_by(|a, b| a.entry.filepath.cmp(&b.entry.filepath));

    let mut giants: Vec<&ScoredProjectRow> = rows
        .iter()
        .filter(|row| is_maintainability_giant(&row.result))
        .collect();
    giants.sort_by(|a, b| {
        let ca = gate_metrics_for(&a.result)
            .get("cfg.cyclomatic")
            .copied()
            .unwrap_or(0.0);
        let cb = gate_metrics_for(&b.result)
            .get("cfg.cyclomatic")
            .copied()
            .unwrap_or(0.0);
        cb.partial_cmp(&ca).unwrap_or(Ordering::Equal)
    });

    (
        hard.iter().map(|row| to_worst_entry(&row.entry)).collect(),
        leaves
            .iter()
            .map(|row| to_worst_entry(&row.entry))
            .collect(),
        giants
            .iter()
            .map(|row| to_worst_entry(&row.entry))
            .collect(),
    )
}

fn project_file_sort_key(row: &ScoredProjectRow) -> (u8, f64, f64) {
    if is_hard_fail(&row.result) {
        let (count, score) = hard_fail_sort_key(&row.result);
        (0, -(count as f64), score)
    } else if is_maintainability_giant(&row.result) {
        let cyclomatic = gate_metrics_for(&row.result)
            .get("cfg.cyclomatic")
            .copied()
            .unwrap_or(0.0);
        (1, -cyclomatic, 0.0)
    } else if is_leaf_composable_zero(&row.result) {
        (3, worst_key(&row.entry), 0.0)
    } else {
        (2, worst_key(&row.entry), 0.0)
    }
}

fn worst_key(entry: &ProjectFileEntry) -> f64 {
    weakest_score(&entry.scores)
}

fn build_language_rollups(
    per_language_results: &HashMap<String, Vec<ClassificationResult>>,
    per_language_entries: &HashMap<String, Vec<ProjectFileEntry>>,
    per_language_parse_failures: &HashMap<String, usize>,
) -> Vec<ProjectLanguageRollup> {
    let classifier = CharacteristicMorphism;
    let mut languages: Vec<&String> = per_language_results.keys().collect();
    languages.sort();
    languages
        .into_iter()
        .map(|language| {
            let results = &per_language_results[language];
            let rolled = classifier.combine_dimensions(results);
            let rolled_scores = min_scores_by_dim(results);
            let lang_entries = per_language_entries.get(language);
            let lang_results = per_language_results.get(language);
            let mut rows: Vec<ScoredProjectRow> = match (lang_entries, lang_results) {
                (Some(entries), Some(results)) if entries.len() == results.len() => entries
                    .iter()
                    .cloned()
                    .zip(results.iter().cloned())
                    .map(|(entry, result)| ScoredProjectRow { entry, result })
                    .collect(),
                (Some(entries), _) => entries
                    .iter()
                    .cloned()
                    .map(|entry| {
                        let result = ClassificationResult {
                            is_parseable: entry.is_parseable,
                            ..Default::default()
                        };
                        ScoredProjectRow { entry, result }
                    })
                    .collect(),
                _ => Vec::new(),
            };
            rows.sort_by(|a, b| {
                project_file_sort_key(a)
                    .partial_cmp(&project_file_sort_key(b))
                    .unwrap_or(Ordering::Equal)
            });
            let worst = rows.first().map(|row| &row.entry);
            ProjectLanguageRollup {
                language: language.clone(),
                file_count: rows.len(),
                parse_failures: per_language_parse_failures
                    .get(language)
                    .copied()
                    .unwrap_or(0),
                rolled_up_dimensions: rolled
                    .iter()
                    .map(|(dim, &val)| (dim.clone(), lattice_to_str(val)))
                    .collect(),
                rolled_up_scores: rolled_scores,
                aggregate_floor_verdict: aggregate_floor_verdict(&rolled),
                worst_file_path: worst.map(|w| w.filepath.clone()),
                worst_file_verdict: worst.map(|w| w.lattice_element),
            }
        })
        .collect()
}

struct BuildProjectArgs<'a> {
    resolved_root: &'a Path,
    source_file_count: usize,
    parse_failures: usize,
    per_file_results: Vec<ClassificationResult>,
    entries: Vec<ProjectFileEntry>,
    any_dep_graph_loaded: bool,
    last_load_error: Option<String>,
    per_language_results: HashMap<String, Vec<ClassificationResult>>,
    per_language_entries: HashMap<String, Vec<ProjectFileEntry>>,
    per_language_parse_failures: HashMap<String, usize>,
    params: &'a EvaluateProjectInput,
    priority: Priority,
    priority_source: PrioritySource,
    coupling_available: bool,
    project_root: &'a Path,
    /// The `gitnexus_dir` override resolved to an absolute path against the
    /// MCP file root (see [`resolve_override_for_root`]) — pass this to
    /// [`gitnexus_warnings`] instead of `params.gitnexus_dir`, which
    /// `project_root` above has already derived a non-default root from.
    resolved_gitnexus_override: Option<&'a str>,
    gitnexus_dir: Option<&'a Path>,
    generation_note: Option<String>,
}

fn build_project_result(args: BuildProjectArgs<'_>) -> ProjectEvaluationResult {
    let classifier = CharacteristicMorphism;
    let rolled = classifier.combine_dimensions(&args.per_file_results);
    let rolled_scores = min_scores_by_dim(&args.per_file_results);
    let language_rollups = build_language_rollups(
        &args.per_language_results,
        &args.per_language_entries,
        &args.per_language_parse_failures,
    );

    let overall = aggregate_floor_verdict(&rolled);
    let mut rows: Vec<ScoredProjectRow> = if args.entries.len() == args.per_file_results.len() {
        args.entries
            .into_iter()
            .zip(args.per_file_results)
            .map(|(entry, result)| ScoredProjectRow { entry, result })
            .collect()
    } else {
        args.entries
            .into_iter()
            .map(|entry| {
                let result = ClassificationResult {
                    is_parseable: entry.is_parseable,
                    ..Default::default()
                };
                ScoredProjectRow { entry, result }
            })
            .collect()
    };

    let mut score_sorted = rows.clone();
    score_sorted.sort_by(|a, b| {
        worst_key(&a.entry)
            .partial_cmp(&worst_key(&b.entry))
            .unwrap_or(Ordering::Equal)
    });
    let worst_files: Vec<WorstFileEntry> = score_sorted
        .iter()
        .take(3)
        .map(|row| to_worst_entry(&row.entry))
        .collect();

    let (hard_fails, leaf_composable_zeros, maintainability_giants) = classify_project_rows(&rows);
    let hard_fail_head_owned = rows
        .iter()
        .find(|row| is_hard_fail(&row.result))
        .map(|row| row.entry.clone());
    let hard_fail_head = hard_fail_head_owned.as_ref();

    rows.sort_by(|a, b| {
        project_file_sort_key(a)
            .partial_cmp(&project_file_sort_key(b))
            .unwrap_or(Ordering::Equal)
    });
    let entries: Vec<ProjectFileEntry> = rows.iter().map(|row| row.entry.clone()).collect();

    let aggregate_explanation =
        aggregate_explanation(&rolled, &rolled_scores, hard_fail_head, &entries);
    let worst_file_verdict = hard_fails
        .first()
        .map(|w| w.lattice_element)
        .or_else(|| worst_files.first().map(|w| w.lattice_element));
    let guidance = project_guidance(hard_fail_head, &entries);

    let page: Vec<ProjectFileEntry> = entries
        .iter()
        .skip(args.params.offset)
        .take(args.params.limit.clamp(1, 500))
        .cloned()
        .collect();
    let has_more = args.params.offset + page.len() < entries.len();
    let next_offset = has_more.then_some(args.params.offset + page.len());

    let mut project_warnings = gitnexus_warnings(
        args.resolved_gitnexus_override,
        args.project_root,
        args.gitnexus_dir,
        args.any_dep_graph_loaded,
        args.last_load_error.as_deref(),
    );
    if let Some(note) = args.generation_note {
        project_warnings.insert(0, note);
    }
    let contract = project_contract(
        overall,
        hard_fail_head,
        &entries,
        args.coupling_available,
        &project_warnings,
        args.parse_failures,
    );

    ProjectEvaluationResult {
        root: args.resolved_root.to_string_lossy().to_string(),
        file_count: args.source_file_count,
        parse_failures: args.parse_failures,
        rolled_up_dimensions: rolled
            .iter()
            .map(|(dim, &val)| (dim.clone(), lattice_to_str(val)))
            .collect(),
        rolled_up_scores: rolled_scores,
        aggregate_floor_verdict: overall,
        language_rollups,
        aggregate_explanation,
        worst_file_verdict,
        hard_fails,
        leaf_composable_zeros,
        maintainability_giants,
        worst_files,
        guidance,
        priority: priority_str(args.priority).to_string(),
        priority_source: args.priority_source,
        coupling_available: args.coupling_available,
        warnings: project_warnings,
        agent_contract: Some(contract),
        count: page.len(),
        offset: args.params.offset,
        total: entries.len(),
        has_more,
        next_offset,
        files: page,
        verbose: args.params.verbose,
        error: None,
    }
}

fn empty_project_result(
    params: &EvaluateProjectInput,
    priority: Priority,
    priority_source: PrioritySource,
    error: Option<String>,
) -> ProjectEvaluationResult {
    ProjectEvaluationResult {
        root: params.path.clone(),
        file_count: 0,
        parse_failures: 0,
        rolled_up_dimensions: HashMap::new(),
        rolled_up_scores: HashMap::new(),
        aggregate_floor_verdict: LatticeElement::SLOP,
        language_rollups: Vec::new(),
        aggregate_explanation: "No files were evaluated, so the aggregate floor is SLOP."
            .to_string(),
        worst_file_verdict: None,
        hard_fails: Vec::new(),
        leaf_composable_zeros: Vec::new(),
        maintainability_giants: Vec::new(),
        worst_files: Vec::new(),
        guidance: error
            .clone()
            .unwrap_or_else(|| "No project guidance available.".to_string()),
        priority: priority_str(priority).to_string(),
        priority_source,
        coupling_available: false,
        warnings: Vec::new(),
        agent_contract: Some(AgentContract {
            next_tool: None,
            next_actions: Vec::new(),
            blocked_by: if error.is_some() {
                vec!["project_evaluation_error".to_string()]
            } else {
                Vec::new()
            },
            verification_gates: Vec::new(),
            risk_flags: if error.is_some() {
                vec!["project_evaluation_error".to_string()]
            } else {
                Vec::new()
            },
        }),
        count: 0,
        offset: params.offset,
        total: 0,
        has_more: false,
        next_offset: None,
        files: Vec::new(),
        verbose: params.verbose,
        error,
    }
}

fn aggregate_explanation(
    rolled: &HashMap<String, EvaluationValue>,
    rolled_scores: &HashMap<String, f64>,
    hard_fail_head: Option<&ProjectFileEntry>,
    entries: &[ProjectFileEntry],
) -> String {
    if entries.is_empty() {
        return "No files were evaluated, so the aggregate floor is SLOP.".to_string();
    }
    let mut failed: Vec<&String> = rolled
        .iter()
        .filter(|(_, &val)| lattice_to_str(val) == LatticeElement::SLOP)
        .map(|(dim, _)| dim)
        .collect();
    failed.sort();
    let worst = hard_fail_head
        .or_else(|| {
            entries.iter().min_by(|a, b| {
                worst_key(a)
                    .partial_cmp(&worst_key(b))
                    .unwrap_or(Ordering::Equal)
            })
        })
        .expect("entries is non-empty");
    if !failed.is_empty() {
        let dim = failed
            .iter()
            .min_by(|a, b| {
                let sa = rolled_scores.get(**a).copied().unwrap_or(100.0);
                let sb = rolled_scores.get(**b).copied().unwrap_or(100.0);
                sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("failed is non-empty");
        let score_text = rolled_scores
            .get(*dim)
            .map(|s| format!(" ({s:.1}%)"))
            .unwrap_or_default();
        return format!(
            "Aggregate floor is SLOP because at least one file fails {dim}{score_text}; \
             worst current target is {} ({}).",
            worst.filepath,
            worst.lattice_element.as_str()
        );
    }
    format!(
        "Aggregate floor satisfies every measured generator; worst current target is {} ({}).",
        worst.filepath,
        worst.lattice_element.as_str()
    )
}

fn project_guidance(
    hard_fail_head: Option<&ProjectFileEntry>,
    entries: &[ProjectFileEntry],
) -> String {
    let score_worst = entries.iter().min_by(|a, b| {
        worst_key(a)
            .partial_cmp(&worst_key(b))
            .unwrap_or(Ordering::Equal)
    });
    let Some(worst) = hard_fail_head.or(score_worst) else {
        return "No files were evaluated.".to_string();
    };
    if let Some(warning) = worst.warnings.first() {
        return format!("Start with `{}`: {warning}", worst.filepath);
    }
    if !worst.scores.is_empty() {
        let dim = worst
            .scores
            .iter()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(dim, _)| dim.clone())
            .unwrap_or_default();
        return format!(
            "Start with `{}`; weakest measured generator is {dim}.",
            worst.filepath
        );
    }
    format!(
        "Start with `{}`; inspect parseability and raw metrics.",
        worst.filepath
    )
}

fn project_contract(
    overall: LatticeElement,
    hard_fail_head: Option<&ProjectFileEntry>,
    entries: &[ProjectFileEntry],
    coupling_available: bool,
    warnings: &[String],
    parse_failures: usize,
) -> AgentContract {
    let scan_rows: Vec<&ProjectFileEntry> =
        hard_fail_head.into_iter().chain(entries.iter()).collect();
    let prelude = agent_contract_prelude(AgentContractPreludeInput {
        coupling_available,
        warnings,
        parse_failures,
        grade_capped: scan_rows.iter().any(|f| f.grade_capped),
        active_security_findings: scan_rows.iter().any(|f| f.secure_adjusted == Some(false)),
        ..Default::default()
    });

    let verification_gates = vec![
        "topos_assess_worktree_change validates each accepted in-place refactor".to_string(),
        "project rollup does not regress after non-trivial changes".to_string(),
        "behavior tests or type/lint checks pass when available".to_string(),
    ];
    if let Some(action) = prelude.composable.next_action {
        return finish_agent_contract(
            prelude.blocked_by,
            prelude.risk_flags,
            prelude.composable.next_tool,
            vec![action],
            verification_gates,
        );
    }
    let Some(head) = hard_fail_head else {
        let mut next_actions = Vec::new();
        let next_tool = if overall == LatticeElement::IDEAL {
            next_actions.push("preserve behavior checks before accepting".into());
            None
        } else if let Some(fallback) = entries.first() {
            next_actions.push(format!(
                "start with {} using language {}",
                fallback.filepath, fallback.language
            ));
            Some("topos_inspect_code".to_string())
        } else {
            None
        };
        return finish_agent_contract(
            prelude.blocked_by,
            prelude.risk_flags,
            next_tool,
            next_actions,
            verification_gates,
        );
    };
    let mut next_actions = vec![format!(
        "evaluate `{}` with refactor_targets to surface gating targets",
        head.filepath
    )];
    let next_tool = if overall == LatticeElement::IDEAL {
        next_actions.push("preserve behavior checks before accepting".into());
        None
    } else {
        Some("topos_evaluate_file".to_string())
    };

    finish_agent_contract(
        prelude.blocked_by,
        prelude.risk_flags,
        next_tool,
        next_actions,
        verification_gates,
    )
}

/// The "## Agent Contract" section of the project markdown report.
fn push_agent_contract_lines(lines: &mut Vec<String>, r: &ProjectEvaluationResult) {
    let Some(contract) = &r.agent_contract else {
        return;
    };
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

/// The "## Per-language rollups" section of the project markdown report.
fn push_language_rollup_lines(lines: &mut Vec<String>, r: &ProjectEvaluationResult) {
    if r.language_rollups.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("## Per-language rollups".to_string());
    for rollup in &r.language_rollups {
        lines.push(format!(
            "- **{}**: {} (files={}, parse_failures={})",
            rollup.language,
            rollup.aggregate_floor_verdict.as_str(),
            rollup.file_count,
            rollup.parse_failures
        ));
        if let (Some(path), Some(verdict)) = (&rollup.worst_file_path, rollup.worst_file_verdict) {
            lines.push(format!("  - worst: `{path}` ({})", verdict.as_str()));
        }
    }
}

fn render_project_entry(entry: &ProjectFileEntry, verbose: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let mut score_pairs: Vec<(&String, &f64)> = entry.scores.iter().collect();
    score_pairs.sort_by(|a, b| a.0.cmp(b.0));
    let s_str = score_pairs
        .iter()
        .map(|(k, v)| format!("{k}={v:.0}"))
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!(
        "- `{}` — {} ({s_str})",
        entry.filepath,
        entry.lattice_element.as_str()
    ));
    if verbose && !entry.raw_metrics.is_empty() {
        let mut metric_pairs: Vec<(&String, &f64)> = entry.raw_metrics.iter().collect();
        metric_pairs.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in metric_pairs {
            lines.push(format!("  - `{k}`: {v:.3}"));
        }
    }
    lines
}

pub(crate) fn render_project_md(r: &ProjectEvaluationResult) -> String {
    let mut lines = vec![format!("# Project Evaluation — {}", r.root), String::new()];
    lines.push(format!(
        "**Overall:** {}",
        r.aggregate_floor_verdict.as_str()
    ));
    lines.push(format!(
        "**Files scanned:** {} (parse failures: {})",
        r.file_count, r.parse_failures
    ));
    lines.push(format!("**Priority:** `{}`", r.priority));
    if !r.coupling_available {
        lines.push("> ⚠️ No `.gitnexus/` present — coupling dimension not scored.".to_string());
    }
    push_agent_contract_lines(&mut lines, r);
    lines.push(String::new());
    lines.push("## Rolled-up dimensions".to_string());
    let mut dims: Vec<(&String, &LatticeElement)> = r.rolled_up_dimensions.iter().collect();
    dims.sort_by(|a, b| a.0.cmp(b.0));
    for (dim, val) in dims {
        let score = r
            .rolled_up_scores
            .get(dim)
            .map(|s| format!(" ({s:.1}%)"))
            .unwrap_or_default();
        lines.push(format!("- **{dim}**: {}{score}", val.as_str()));
    }
    push_language_rollup_lines(&mut lines, r);
    lines.push(String::new());
    lines.push(format!(
        "## Worst files (showing {} of {}, offset {})",
        r.count, r.total, r.offset
    ));
    for entry in &r.files {
        lines.extend(render_project_entry(entry, r.verbose));
    }
    if r.has_more {
        lines.push(format!(
            "\n_more files available: pass offset={} to continue._",
            r.next_offset.unwrap_or_default()
        ));
    }
    if let Some(error) = &r.error {
        lines.push(format!("\n> error: {error}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemas::PillarResult;

    fn entry(filepath: &str, simple_score: f64) -> ProjectFileEntry {
        ProjectFileEntry {
            filepath: filepath.to_string(),
            language: "python".to_string(),
            lattice_element: LatticeElement::SIMPLE,
            scores: HashMap::from([("simple".to_string(), simple_score)]),
            pillars: HashMap::from([(
                "simple".to_string(),
                PillarResult {
                    achieved: true,
                    score: simple_score,
                },
            )]),
            raw_metrics: HashMap::from([("cfg.cyclomatic".to_string(), 3.0)]),
            warnings: Vec::new(),
            security_findings: Vec::new(),
            acknowledged_risks: Vec::new(),
            raw_lattice_element: None,
            adjusted_lattice_element: None,
            secure_raw: None,
            secure_adjusted: None,
            grade_capped: false,
            is_parseable: true,
        }
    }

    fn project_params(offset: usize) -> EvaluateProjectInput {
        EvaluateProjectInput {
            path: ".".to_string(),
            preferences: None,
            gitnexus_dir: None,
            no_composable: true,
            limit: 2,
            offset,
            verbose: false,
            include_security_findings: false,
            allow: Vec::new(),
        }
    }

    fn project_result(params: &EvaluateProjectInput) -> ProjectEvaluationResult {
        let entries = vec![
            entry("mid.py", 50.0),
            entry("worst.py", 10.0),
            entry("best.py", 90.0),
            entry("second.py", 20.0),
            entry("third.py", 30.0),
        ];
        let root = Path::new("/tmp/topos-project-test");
        build_project_result(BuildProjectArgs {
            resolved_root: root,
            source_file_count: entries.len(),
            parse_failures: 0,
            per_file_results: Vec::new(),
            entries,
            any_dep_graph_loaded: false,
            last_load_error: None,
            per_language_results: HashMap::new(),
            per_language_entries: HashMap::new(),
            per_language_parse_failures: HashMap::new(),
            params,
            priority: Priority::Simple,
            priority_source: PrioritySource::Default,
            coupling_available: false,
            project_root: root,
            resolved_gitnexus_override: None,
            gitnexus_dir: None,
            generation_note: None,
        })
    }

    /// Named lists and deprecated `worst_files` are page-global: paging must
    /// not shift them under the agent.
    #[test]
    fn worst_files_are_page_global_and_slim() {
        let first_page = project_result(&project_params(0));
        let second_page = project_result(&project_params(2));

        let paths: Vec<&str> = first_page
            .worst_files
            .iter()
            .map(|w| w.filepath.as_str())
            .collect();
        assert_eq!(paths, vec!["worst.py", "second.py", "third.py"]);
        assert_eq!(
            paths,
            second_page
                .worst_files
                .iter()
                .map(|w| w.filepath.as_str())
                .collect::<Vec<_>>(),
            "worst_files must not follow offset/limit"
        );
        assert_eq!(
            first_page.hard_fails, second_page.hard_fails,
            "hard_fails must not follow offset/limit"
        );
        assert_eq!(
            first_page.leaf_composable_zeros, second_page.leaf_composable_zeros,
            "leaf_composable_zeros must not follow offset/limit"
        );
        assert_eq!(
            first_page.maintainability_giants, second_page.maintainability_giants,
            "maintainability_giants must not follow offset/limit"
        );
        // Page 2 does not even contain the worst file, so the compact list
        // is the only place it is named.
        assert!(!second_page.files.iter().any(|f| f.filepath == "worst.py"));

        // Slim by construction: identity plus verdict, no row payload.
        for list in [
            &first_page.worst_files,
            &first_page.hard_fails,
            &first_page.leaf_composable_zeros,
            &first_page.maintainability_giants,
        ] {
            if let Some(entry) = list.first() {
                let json = serde_json::to_value(entry).expect("serialize");
                let keys: Vec<&String> = json.as_object().expect("object").keys().collect();
                assert_eq!(keys, vec!["filepath", "lattice_element"]);
            }
        }
    }

    fn classified_row(result: ClassificationResult, entry: ProjectFileEntry) -> ScoredProjectRow {
        ScoredProjectRow { entry, result }
    }

    fn composable_leaf_result() -> ClassificationResult {
        let mut result = ClassificationResult {
            is_parseable: true,
            ..Default::default()
        };
        result
            .raw_metrics
            .insert("mdg.instability".to_string(), 0.0);
        result.raw_metrics.insert("mdg.fan_in".to_string(), 0.0);
        result.raw_metrics.insert("mdg.fan_out".to_string(), 0.0);
        result
    }

    #[test]
    fn leaf_composable_zero_is_excluded_from_hard_fails() {
        let entry = entry("leaf.py", 95.0);
        let rows = vec![classified_row(composable_leaf_result(), entry)];
        let (hard, leaves, giants) = classify_project_rows(&rows);
        assert!(hard.is_empty(), "structural leaf must not hard-fail");
        assert_eq!(leaves.len(), 1);
        assert!(giants.is_empty());
    }

    #[test]
    fn hard_fails_surface_simple_gate_failures() {
        let mut result = ClassificationResult {
            is_parseable: true,
            ..Default::default()
        };
        result
            .raw_metrics
            .insert("ast.max_function_complexity".to_string(), 30.0);
        let entry = entry("fail.py", 10.0);
        let rows = vec![classified_row(result, entry.clone())];
        let (hard, leaves, _) = classify_project_rows(&rows);
        assert_eq!(hard.len(), 1);
        assert_eq!(hard[0].filepath, "fail.py");
        assert!(leaves.is_empty());
        let contract = project_contract(
            LatticeElement::SLOP,
            Some(&entry),
            std::slice::from_ref(&entry),
            false,
            &[],
            0,
        );
        assert_eq!(contract.next_tool.as_deref(), Some("topos_evaluate_file"));
    }

    #[test]
    fn maintainability_giants_rank_advisory_cyclomatic() {
        let mut result = ClassificationResult {
            is_parseable: true,
            ..Default::default()
        };
        result
            .raw_metrics
            .insert("cfg.cyclomatic".to_string(), SIMPLE.max_cyclomatic + 5.0);
        let entry = entry("big.py", 90.0);
        let rows = vec![classified_row(result, entry)];
        let (hard, _, giants) = classify_project_rows(&rows);
        assert!(hard.is_empty());
        assert_eq!(giants.len(), 1);
        assert_eq!(giants[0].filepath, "big.py");
    }

    /// The guidance and contract still read the *full* worst rows, which
    /// is why the slimming happens at the wire boundary only.
    #[test]
    fn project_guidance_still_names_the_worst_file() {
        let model = project_result(&project_params(0));
        assert!(
            model.guidance.contains("worst.py"),
            "guidance was: {}",
            model.guidance
        );
        assert_eq!(model.worst_file_verdict, Some(LatticeElement::SIMPLE));
    }

    /// Ranked targets are a single-file affordance: three per row would
    /// dwarf the rows themselves, so no project row may carry them.
    #[test]
    fn project_rows_carry_no_refactor_targets() {
        let model = project_result(&project_params(0));
        let json = serde_json::to_value(&model.files[0]).expect("serialize");
        let obj = json.as_object().expect("object");
        assert!(obj.get("refactor_targets").is_none());
        assert!(obj.get("binding_constraint").is_none());
    }

    fn write_temp_source(name: &str, source: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_mcp_evaluate_test_{}_{}",
            std::process::id(),
            name
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join(format!("{name}.py"));
        std::fs::write(&path, source).expect("write temp source");
        path
    }

    fn single_row(name: &str, verbose: bool) -> ProjectFileEntry {
        let path = write_temp_source(
            name,
            "def f(x):\n    if x:\n        return 1\n    return 0\n",
        );
        let dir = path.parent().expect("parent");
        let (_, entry, ..) = evaluate_single_file(
            &path,
            dir,
            Priority::Simple,
            None,
            /* include_security_findings = */ false,
            verbose,
            &[],
        )
        .expect("classify temp file");
        entry
    }

    /// Both channels agree on `verbose`: the project markdown renderer
    /// already gated the raw metrics, while the structured row shipped
    /// them unconditionally.
    #[test]
    fn project_rows_gate_raw_metrics_on_verbose() {
        let compact = single_row("compact", false);
        assert!(
            compact.raw_metrics.is_empty(),
            "default project rows must not carry raw metrics"
        );
        assert!(
            !compact.scores.is_empty(),
            "scores are the row's payload and always stay"
        );
        let json = serde_json::to_value(&compact).expect("serialize");
        assert!(json.get("raw_metrics").is_none());

        let verbose = single_row("verbose", true);
        assert!(
            verbose.raw_metrics.contains_key("cfg.cyclomatic"),
            "verbose=true must restore the previous row shape"
        );
    }

    /// #232: an active security overlay must only flip `achieved`/lattice,
    /// never the numeric score — `entry.scores` and `entry.pillars` are
    /// two different channels for the same raw classification.
    #[test]
    fn project_row_scores_and_pillars_agree_under_security_overlay() {
        let path = write_temp_source("overlay_secure", "def f(expr):\n    return eval(expr)\n");
        let dir = path.parent().expect("parent");
        let (_, entry, ..) = evaluate_single_file(
            &path,
            dir,
            Priority::Simple,
            None,
            /* include_security_findings = */ false,
            /* verbose = */ false,
            &["eval".to_string()],
        )
        .expect("classify temp file");

        let raw_score = entry
            .scores
            .get("secure")
            .copied()
            .expect("secure score present");
        let pillar = entry.pillars.get("secure").expect("secure pillar present");

        assert_eq!(
            raw_score, pillar.score,
            "entry.scores and pillars must report the same (raw) secure score"
        );
        assert!(
            raw_score > 0.0 && raw_score < 100.0,
            "fixture must produce a non-trivial raw score, not 0/100 by coincidence: {raw_score}"
        );
        assert!(
            pillar.achieved,
            "the allowlisted eval call must still achieve SECURE via the overlay verdict"
        );
    }
}
