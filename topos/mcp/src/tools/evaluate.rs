//! Evaluation tools: code string, single file, and whole project.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResponse, CallToolResult, ErrorData as McpError};
use rmcp::service::RequestContext;
use rmcp::{tool, tool_router, RoleServer};
use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::omega::{verdict_from_generators, EvaluationValue, Generator};
use topos_engine::evaluation::advisory::{advisories_json, rescore_population};
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::policies::calibration::SIMPLE;
use topos_engine::evaluation::weakest_score;

use crate::diagnostics::{
    opt_in_security_report, overlay_for_file, overlay_for_source, SecurityOverlay,
};
use crate::evaluation::{
    all_source_suffixes, classify_code_string, classify_file, detect_language,
    ensure_gitnexus_dir_with_cancel, gitnexus_warnings, resolve_mcp_composable_project_root,
    resolve_override_for_root,
};
use crate::execution::{is_cancelled, EvalCancelled, StatusReporter};
use crate::formatting::append_path_note;
use crate::formatting::{
    agent_contract_prelude, build_pillars, error_md, finish_agent_contract, render_evaluation_md,
    to_evaluation_result, to_tool_result, AgentContractPreludeInput, EvalResultOptions,
};
use crate::metric_locations::build_metric_locations;
use crate::schemas::{
    lattice_to_str, priority_str, resolve_priority, AgentContract, Detail, EvaluateCodeInput,
    EvaluateFileInput, EvaluateProjectInput, EvaluationResult, LatticeElement, PrioritySource,
    ProjectEvaluationResult, ProjectFileEntry, ProjectLanguageRollup, ProjectPillar,
    RefactorTarget, SecurityFinding, WorstFileEntry,
};
use crate::security::{
    composable_default_root, read_resolved_utf8, requested_absolute, resolution_note,
    resolve_project_path,
};
use crate::security_findings::SecurityReport;
use crate::server::ToposServer;

mod file_targets;
mod large_tree;
mod summary;
mod worst;

use self::worst::{failing_gates, worst_entry, BindingInputs};
use crate::shaping;

pub(crate) fn overlay_opts(overlay: Option<&SecurityOverlay>, opts: &mut EvalResultOptions<'_>) {
    if let Some(overlay) = overlay {
        opts.security_findings = overlay.active_findings.clone();
        opts.security_advisory = overlay.advisory.clone();
        opts.acknowledged_risks = overlay.acknowledged_risks.clone();
    }
}

/// `security_scan` opt-in: attach the scanner report to the payload only,
/// after the result (verdict, pillars, agent contract) is built, so the
/// coverage pass can never change routing or scores.
fn attach_opt_in_report(
    model: &mut EvaluationResult,
    report: Option<SecurityReport>,
    include_findings: bool,
) {
    if let Some(report) = report {
        if include_findings {
            model.security_findings = report.findings;
        }
        model.security_advisory = report.advisory;
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
    pub async fn topos_evaluate_code(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(params): Parameters<EvaluateCodeInput>,
    ) -> Result<CallToolResult, McpError> {
        self.executor()
            .run_inline(&context, move |cancel_flag| {
                if is_cancelled(Some(cancel_flag)) {
                    return Err(EvalCancelled);
                }
                Ok(evaluate_code_sync(params))
            })
            .await
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
    ///
    /// Do not call again when `agent_contract.next_tool` is absent: the
    /// file already passes, or this call already returned the edit span.
    #[tool(
        name = "topos_evaluate_file",
        annotations(
            title = "Topos Code Evaluation",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn topos_evaluate_file(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(params): Parameters<EvaluateFileInput>,
    ) -> Result<CallToolResult, McpError> {
        // Generation can shell out to `gitnexus analyze` (bounded by
        // TOPOS_DEPGRAPH_TIMEOUT, default 300s); offload so a slow/first-time
        // run cannot stall the transport, matching topos_evaluate_project.
        self.executor()
            .run_inline(&context, move |cancel_flag| {
                evaluate_file_sync_with_cancel(params, Some(cancel_flag))
            })
            .await
    }

    /// Recursively score every supported source file in a directory on the
    /// SIMPLE / COMPOSABLE / SECURE / NAVIGABLE lattice, with a project
    /// rollup (side-effecting).
    ///
    /// Autodetects all supported languages in one walk. Returns a summary
    /// by default: the rollup (`aggregate_floor_verdict` is the pillars
    /// every scored file holds), the five `worst` files with absolute paths
    /// and the metric behind each, counts, and warnings. `detail: "full"`
    /// returns per-file rows (`limit` / `offset`) and the named lists.
    ///
    /// Unless `no_composable` is set, generates/refreshes `.gitnexus` when
    /// missing or stale before scoring, same as `topos_evaluate_file` and
    /// the CLI's `topos evaluate` — `coupling_available`/`warnings` explain
    /// it when that isn't possible, without failing the evaluation.
    ///
    /// Do not call this for one file, or after a file already passed. It
    /// walks the tree. A person asks for the project rollup; an agent
    /// editing one file does not.
    #[tool(
        name = "topos_evaluate_project",
        annotations(
            title = "Topos Code Evaluation",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn topos_evaluate_project(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(params): Parameters<EvaluateProjectInput>,
    ) -> Result<CallToolResponse, McpError> {
        let (priority, priority_source) = resolve_priority(params.preferences.as_ref());
        if let Some(Err(exc)) = params.preferences.as_ref().map(|p| p.to_preferences()) {
            return Ok(CallToolResponse::Complete(project_error(
                &params,
                priority,
                priority_source,
                exc,
            )));
        }
        match resolve_project_path(&params.path) {
            Ok((resolved_root, _)) => {
                if !resolved_root.is_dir() {
                    let msg = format!("Path is not a directory: {}", resolved_root.display());
                    return Ok(CallToolResponse::Complete(project_error(
                        &params,
                        priority,
                        priority_source,
                        msg,
                    )));
                }
            }
            Err(msg) => {
                return Ok(CallToolResponse::Complete(project_error(
                    &params,
                    priority,
                    priority_source,
                    msg,
                )));
            }
        }

        // The walk + per-file classification is CPU-bound; run it off the
        // async executor so a large project cannot stall the transport.
        self.executor()
            .run_task_or_inline(
                self.task_manager(),
                &context,
                Duration::from_secs(300),
                "Evaluating project",
                move |cancel_flag, reporter| {
                    evaluate_project_sync_with_cancel(params, Some(cancel_flag), Some(reporter))
                },
            )
            .await
    }
}

pub(crate) fn evaluate_code_sync(params: EvaluateCodeInput) -> CallToolResult {
    let (priority, priority_source) = resolve_priority(params.preferences.as_ref());
    let result = match classify_code_string(&params.code, &params.language, priority) {
        Ok(result) => result,
        Err(exc) => return err_eval("Evaluation failed", Priority::Simple, priority_source, exc),
    };
    let prefs = match params.preferences.as_ref().map(|p| p.to_preferences()) {
        Some(Err(exc)) => {
            return err_eval("Evaluation failed", Priority::Simple, priority_source, exc)
        }
        Some(Ok(p)) => Some(p),
        None => None,
    };
    let overlay = overlay_for_source(&params.code, &params.language, &result, None, &params.allow);
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

pub(crate) fn evaluate_file_sync_with_cancel(
    params: EvaluateFileInput,
    cancel_flag: Option<&AtomicBool>,
) -> Result<CallToolResult, EvalCancelled> {
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    let (priority, priority_source) = resolve_priority(params.preferences.as_ref());
    let (resolved, detected_project) = match resolve_project_path(&params.filepath) {
        Ok(context) => context,
        Err(err) => {
            return Ok(err_eval(
                "Access denied / path error",
                Priority::Simple,
                priority_source,
                err,
            ))
        }
    };
    if !resolved.is_file() {
        return Ok(err_eval(
            "Not a file",
            Priority::Simple,
            priority_source,
            format!("Path is not a file: {}", resolved.display()),
        ));
    }
    let _cache_lease = match crate::cache::lease_managed_path(&resolved) {
        Ok(lease) => lease,
        Err(error) => {
            return Ok(err_eval(
                "PR cache in use",
                Priority::Simple,
                priority_source,
                format!("could not lease the managed PR cache: {error}"),
            ))
        }
    };

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
    let gitnexus_outcome = ensure_gitnexus_dir_with_cancel(
        resolved_override.as_deref(),
        &project_root,
        params.no_composable,
        /* capture = */ true,
        cancel_flag,
        None,
    )?;
    let gitnexus_dir = gitnexus_outcome.gitnexus_dir;

    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }

    let (result, dep_graph, load_error) =
        match classify_file(&resolved, priority, gitnexus_dir.as_deref()) {
            Ok(triple) => triple,
            Err(exc) => {
                return Ok(err_eval(
                    "Evaluation failed",
                    Priority::Simple,
                    priority_source,
                    exc,
                ))
            }
        };

    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }

    let prefs = match params.preferences.as_ref().map(|p| p.to_preferences()) {
        Some(Err(exc)) => {
            return Ok(err_eval(
                "Evaluation failed",
                Priority::Simple,
                priority_source,
                exc,
            ))
        }
        Some(Ok(p)) => Some(p),
        None => None,
    };
    // Opting out is not a missing store. The CLI prints nothing in this
    // case; the "no .gitnexus directory" warning would be false when a
    // graph is already on disk.
    let file_matched_dep = dep_graph
        .as_ref()
        .is_some_and(|g| g.file_node_id().is_some());
    let mut warnings = if params.no_composable {
        resolved_override
            .as_deref()
            .and_then(|raw| crate::evaluation::check_override_warning(raw, &project_root))
            .unwrap_or_default()
    } else {
        gitnexus_warnings(
            resolved_override.as_deref(),
            &project_root,
            gitnexus_dir.as_deref(),
            file_matched_dep,
            load_error.as_deref(),
        )
    };
    if dep_graph.is_some() && !file_matched_dep {
        warnings.insert(
            0,
            "COMPOSABLE not scored — dependency graph contains no File node matching the \
             evaluated path; re-run 'topos depgraph generate'."
                .to_string(),
        );
    }
    if let Some(note) = gitnexus_outcome.generation_note {
        warnings.insert(0, note);
    }
    // A relative path names no project, so say which file was actually read.
    let path_note = resolution_note(&params.filepath, &resolved);
    if let Some(note) = path_note.clone() {
        // Appended, not prepended: `warnings.first()` is the COMPOSABLE
        // explanation (`mdg_unavailable_message` reads it), so putting a path
        // note at the front would replace the reason COMPOSABLE is missing.
        warnings.push(note);
    }
    let overlay = overlay_for_file(&resolved, &result, &params.allow);
    let source = read_resolved_utf8(&resolved).ok();
    let locations = file_targets::locations(&resolved, source.as_deref(), &result);

    // Targets are computed before the result model so the agent contract
    // can route them natively and `binding_constraint` can project the top
    // gating one. This is the only place targets are built: they are a
    // single-file affordance, and `evaluate_single_file` (the project page)
    // deliberately has no equivalent — a few full targets per row would
    // outweigh the rows.
    let targets: Option<Vec<RefactorTarget>> = if params.refactor_targets > 0 {
        Some(file_targets::targets(
            &resolved,
            &result,
            overlay.as_ref(),
            &locations,
            params.preferences.as_ref(),
            prefs.as_ref(),
            params.refactor_targets,
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
    let mut model = to_evaluation_result(&result, file_matched_dep, opts);
    run_opt_in_security_scan(
        &mut model,
        &params,
        overlay.is_some(),
        &resolved,
        source.as_deref(),
        &result,
    );
    let mut md = render_evaluation_md(&model, None, params.verbose);
    append_path_note(&mut md, path_note.as_deref());
    Ok(to_tool_result(&model, md))
}

/// `security_scan`: run Sighthound although SECURE passed and attach the
/// report-only advisory block; the verdict and scores are already built.
/// A failed SECURE already carries the overlay's report, so it is skipped.
fn run_opt_in_security_scan(
    model: &mut EvaluationResult,
    params: &EvaluateFileInput,
    has_overlay: bool,
    resolved: &Path,
    source: Option<&str>,
    result: &ClassificationResult,
) {
    if !params.security_scan || has_overlay {
        return;
    }
    let report = source.and_then(|source| {
        opt_in_security_report(
            source,
            detect_language(resolved),
            result,
            Some(resolved),
            &params.allow,
        )
    });
    attach_opt_in_report(model, report, params.include_security_findings);
}

/// An error response in the requested shape: the summary schema with zero
/// counts and `error` set (default), or the full model. `structuredContent.error`
/// is populated either way.
fn project_error(
    params: &EvaluateProjectInput,
    priority: Priority,
    priority_source: PrioritySource,
    error: String,
) -> CallToolResult {
    let model = empty_project_result(params, priority, priority_source, Some(error));
    project_response(params, &model)
}

/// `model` as the tool result of the requested `detail`.
fn project_response(
    params: &EvaluateProjectInput,
    model: &ProjectEvaluationResult,
) -> CallToolResult {
    match params.detail {
        Detail::Summary => {
            let summary = summary::build(model, params);
            to_tool_result(&summary, summary.to_markdown())
        }
        Detail::Full => to_tool_result(model, render_project_md(model)),
    }
}

/// The `root` of an error response: the absolute directory that was asked
/// for, resolved the way the server resolves it (against the file root, or
/// the startup project), whether or not it exists. Empty when the path is a
/// file or cannot be placed, so a file is never called a root.
fn error_root(path: &str) -> String {
    let resolved = match resolve_project_path(path) {
        Ok((resolved, _)) => Some(resolved),
        Err(_) => requested_absolute(path),
    };
    resolved
        .filter(|path| !path.is_file())
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub(crate) fn evaluate_project_sync_with_cancel(
    params: EvaluateProjectInput,
    cancel_flag: Option<&AtomicBool>,
    status_reporter: Option<&StatusReporter>,
) -> Result<CallToolResult, EvalCancelled> {
    let started = Instant::now();
    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }
    let (priority, priority_source) = resolve_priority(params.preferences.as_ref());

    if let Some(Err(exc)) = params.preferences.as_ref().map(|p| p.to_preferences()) {
        return Ok(project_error(&params, priority, priority_source, exc));
    }

    let (resolved_root, source_files) = match validate_and_collect_project(&params, cancel_flag)? {
        Ok(pair) => pair,
        Err(msg) => {
            return Ok(project_error(&params, priority, priority_source, msg));
        }
    };

    let (_requested_path, detected_project) = match resolve_project_path(&params.path) {
        Ok(context) => context,
        Err(err) => {
            return Ok(project_error(&params, priority, priority_source, err));
        }
    };
    let _cache_lease = match crate::cache::lease_managed_path(&resolved_root) {
        Ok(lease) => lease,
        Err(error) => {
            return Ok(project_error(
                &params,
                priority,
                priority_source,
                format!("could not lease the managed PR cache: {error}"),
            ));
        }
    };
    let composable_root = composable_default_root(&detected_project);
    let project_root =
        resolve_mcp_composable_project_root(params.gitnexus_dir.as_deref(), &composable_root);
    // See the matching comment in evaluate_file_sync_with_cancel: must use the
    // resolved override below, not `params.gitnexus_dir`, since a relative
    // override's subdirectory is already baked into `project_root` above.
    let resolved_override =
        resolve_override_for_root(params.gitnexus_dir.as_deref(), &composable_root);
    let gitnexus_outcome = ensure_gitnexus_dir_with_cancel(
        resolved_override.as_deref(),
        &project_root,
        params.no_composable,
        /* capture = */ true,
        cancel_flag,
        status_reporter,
    )?;
    let gitnexus_dir = gitnexus_outcome.gitnexus_dir;

    let mut per_file_results: Vec<ClassificationResult> = Vec::new();
    let mut entries: Vec<ProjectFileEntry> = Vec::new();
    let mut file_has_dep: Vec<bool> = Vec::new();
    let mut parse_failures = 0usize;
    let mut unread_files = 0usize;
    let mut unread_paths: Vec<String> = Vec::new();
    let mut any_dep_graph_loaded = false;
    let mut last_load_error: Option<String> = None;
    let mut per_language_results: HashMap<String, Vec<ClassificationResult>> = HashMap::new();
    let mut per_language_entries: HashMap<String, Vec<ProjectFileEntry>> = HashMap::new();
    let mut per_language_parse_failures: HashMap<String, usize> = HashMap::new();

    let total_files = source_files.len();
    for (idx, path) in source_files.iter().enumerate() {
        if is_cancelled(cancel_flag) {
            return Err(EvalCancelled);
        }
        if let Some(r) = status_reporter {
            r.report(format!("Evaluating file {}/{}", idx + 1, total_files));
        }
        let language = detect_language(path).to_string();
        match evaluate_single_file(
            path,
            priority,
            gitnexus_dir.as_deref(),
            params.include_security_findings,
            params.verbose,
            &params.allow,
        ) {
            Err(_) => {
                parse_failures += 1;
                unread_files += 1;
                unread_paths.push(path.to_string_lossy().to_string());
                *per_language_parse_failures.entry(language).or_default() += 1;
            }
            Ok((result, entry, failed, has_dep, load_error)) => {
                if failed {
                    parse_failures += 1;
                    *per_language_parse_failures.entry(language).or_default() += 1;
                }
                any_dep_graph_loaded |= has_dep;
                if load_error.is_some() {
                    last_load_error = load_error;
                }
                per_file_results.push(result);
                entries.push(entry);
                file_has_dep.push(has_dep);
            }
        }
    }

    if is_cancelled(cancel_flag) {
        return Err(EvalCancelled);
    }

    // The project is one codebase: read advisories against it, then refresh
    // each row from the rescored result before deriving the per-language maps.
    rescore_population(&mut per_file_results);
    for ((result, entry), &has_dep) in per_file_results.iter().zip(&mut entries).zip(&file_has_dep)
    {
        refresh_entry_scores(entry, result, has_dep);
        per_language_results
            .entry(entry.language.clone())
            .or_default()
            .push(result.clone());
        per_language_entries
            .entry(entry.language.clone())
            .or_default()
            .push(entry.clone());
    }

    let coupling_available = gitnexus_dir.is_some() && any_dep_graph_loaded;

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
        scan_ms: started.elapsed().as_millis() as u64,
        unread_files,
        unread_paths,
    });
    Ok(project_response(&params, &model))
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
        scores: overlay.verdict.secure_lifted(&result.scores),
        lattice_element: overlay.verdict.adjusted_element,
        priority: result.priority,
        raw_metrics: result.raw_metrics.clone(),
        interpretation: result.interpretation.clone(),
        is_entrypoint_module: result.is_entrypoint_module,
        is_stable_leaf_module: result.is_stable_leaf_module,
        language: result.language.clone(),
        gate_scores: overlay.verdict.secure_lifted(&result.gate_scores),
        advisories: result.advisories.clone(),
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
    let file_matched_dep = dep_graph
        .as_ref()
        .is_some_and(|g| g.file_node_id().is_some());
    let entry = ProjectFileEntry {
        filepath: path.to_string_lossy().to_string(),
        language: detect_language(path).to_string(),
        lattice_element: lattice_to_str(result_for_rollup.summary()),
        scores: result_for_rollup
            .scores
            .iter()
            .map(|(dim, s)| {
                (
                    dim.clone(),
                    topos_engine::evaluation::policies::desirability::display_percent(*s),
                )
            })
            .collect(),
        pillars: project_pillars(&result_for_rollup, file_matched_dep),
        raw_metrics: if verbose {
            result.raw_metrics.clone()
        } else {
            BTreeMap::new()
        },
        advisories: rounded_advisories(&result),
        warnings: Vec::new(),
        security_findings: if include_security_findings {
            findings
        } else {
            Vec::new()
        },
        security_advisory: overlay.as_ref().and_then(|o| o.advisory.clone()),
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
        file_matched_dep,
        load_error,
    ))
}

/// Re-derive a row's score-dependent fields after the population pass
/// changed `result.scores` / `result.advisories` (verdict fields never move).
fn refresh_entry_scores(
    entry: &mut ProjectFileEntry,
    result: &ClassificationResult,
    has_dep: bool,
) {
    entry.scores = result
        .scores
        .iter()
        .map(|(dim, s)| {
            (
                dim.clone(),
                topos_engine::evaluation::policies::desirability::display_percent(*s),
            )
        })
        .collect();
    entry.pillars = project_pillars(result, has_dep);
    entry.advisories = rounded_advisories(result);
}

/// Per-pillar `achieved`; the score is `scores[pillar]`.
fn project_pillars(
    result: &ClassificationResult,
    has_dep: bool,
) -> BTreeMap<String, ProjectPillar> {
    build_pillars(result, has_dep)
        .into_iter()
        .map(|(dim, pillar)| {
            (
                dim,
                ProjectPillar {
                    achieved: pillar.achieved,
                },
            )
        })
        .collect()
}

/// Advisory readings with floats to three significant figures: 16-digit
/// floats were most of a row's size.
fn rounded_advisories(result: &ClassificationResult) -> BTreeMap<String, serde_json::Value> {
    advisories_json(&result.advisories)
        .into_iter()
        .map(|(key, mut value)| {
            shaping::round_floats(&mut value);
            (key, value)
        })
        .collect()
}

fn validate_and_collect_project(
    params: &EvaluateProjectInput,
    cancel_flag: Option<&AtomicBool>,
) -> Result<Result<(PathBuf, Vec<PathBuf>), String>, EvalCancelled> {
    let (resolved_root, _) = match resolve_project_path(&params.path) {
        Ok(pair) => pair,
        Err(err) => return Ok(Err(err)),
    };
    if !resolved_root.is_dir() {
        return Ok(Err(format!(
            "Path is not a directory: {}",
            resolved_root.display()
        )));
    }
    let suffixes = all_source_suffixes();
    let source_files = topos_engine::adapters::discovery::collect_source_files_with_cancel(
        &[&resolved_root],
        &suffixes,
        true,
        cancel_flag,
    )
    .ok_or(EvalCancelled)?;
    if source_files.is_empty() {
        return Ok(Err("No supported source files found.".to_string()));
    }
    Ok(Ok((resolved_root, source_files)))
}

fn min_scores_by_dim(results: &[ClassificationResult]) -> BTreeMap<String, f64> {
    let mut min_scores: BTreeMap<String, f64> = BTreeMap::new();
    for r in results {
        for (dim, &s) in &r.scores {
            let entry = min_scores.entry(dim.clone()).or_insert(f64::INFINITY);
            *entry = entry.min(s);
        }
    }
    min_scores
        .into_iter()
        .map(|(dim, s)| {
            (
                dim,
                topos_engine::evaluation::policies::desirability::display_percent(s),
            )
        })
        .collect()
}

fn aggregate_floor_verdict(rolled: &BTreeMap<String, EvaluationValue>) -> LatticeElement {
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

// Counts `failing_gates` calls on the current test thread, so the
// decorate-sort-undecorate refactor can assert one gate evaluation per row.
// Thread-local because libtest runs each test on its own thread.
#[cfg(test)]
thread_local! {
    static GATE_EVAL_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Sort key of a file: tier (failures first), then the tier's two ranking
/// numbers and a tie-break, all ascending.
type FileKey = (u8, f64, f64, f64);

/// Every gate-derived sort input for one row, evaluated exactly once.
///
/// The ranking lists decorate rows with this, sort the decorated vector and
/// project back to rows, so gate classification stays O(n) instead of being
/// recomputed inside every comparator call.
#[derive(Clone, Copy)]
struct RowKeys {
    hard_fail: bool,
    /// Failed gates, plus one for a file that did not parse.
    failures: usize,
    /// Only read for hard fails -- `0.0` otherwise, as the old comparator
    /// never looked at it for the other buckets.
    weakest: f64,
    giant: bool,
    cyclomatic: f64,
    file_key: FileKey,
}

impl RowKeys {
    fn new(row: &ScoredProjectRow) -> Self {
        let result = &row.result;
        let gate_failures = failing_gates(row).len();
        let failures = gate_failures + usize::from(!result.is_parseable);
        let hard_fail = failures > 0;
        let cyclomatic = result.raw_metrics.get("cfg.cyclomatic").copied();
        let giant = !hard_fail && cyclomatic.is_some_and(|v| v > SIMPLE.max_cyclomatic);
        let weakest = if hard_fail {
            weakest_score_from_result(result)
        } else {
            0.0
        };
        let cyclomatic = cyclomatic.unwrap_or(0.0);
        // Failures first (most failed gates, then lowest score, then fewest
        // pillars held), then every other file by lowest score, then fewest
        // pillars held. Giants are a watch list in their own right and do
        // not outrank other files.
        let held = row.entry.pillars.values().filter(|p| p.achieved).count() as f64;
        let file_key = if hard_fail {
            (0, -(failures as f64), weakest, held)
        } else {
            (1, worst_key(&row.entry), held, 0.0)
        };
        assert_total_order(file_key, &row.entry.filepath);
        RowKeys {
            hard_fail,
            failures,
            weakest,
            giant,
            cyclomatic,
            file_key,
        }
    }
}

type KeyedRow<'a> = (RowKeys, &'a ScoredProjectRow);

/// Decorate: one gate evaluation per row, input order preserved.
fn decorate_rows(rows: &[ScoredProjectRow]) -> Vec<KeyedRow<'_>> {
    rows.iter().map(|row| (RowKeys::new(row), row)).collect()
}

/// Stable sort on the precomputed file key -- same comparator, same ties.
fn sort_keyed(keyed: &mut [KeyedRow<'_>]) {
    keyed.sort_by(|(a, _), (b, _)| {
        a.file_key
            .partial_cmp(&b.file_key)
            .unwrap_or(Ordering::Equal)
    });
}

fn weakest_score_from_result(result: &ClassificationResult) -> f64 {
    let scores_pct: BTreeMap<String, f64> = result
        .scores
        .iter()
        .map(|(dim, s)| (dim.clone(), s * 100.0))
        .collect();
    weakest_score(&scores_pct)
}

fn to_worst_entry(entry: &ProjectFileEntry) -> WorstFileEntry {
    WorstFileEntry {
        parse_failure: !entry.is_parseable,
        ..WorstFileEntry::plain(entry.filepath.clone(), entry.lattice_element)
    }
}

fn classify_keyed_rows<'a>(
    keyed: &[KeyedRow<'a>],
) -> (
    Vec<WorstFileEntry>,
    Vec<WorstFileEntry>,
    Vec<WorstFileEntry>,
) {
    let mut hard: Vec<KeyedRow<'a>> = keyed.iter().filter(|(k, _)| k.hard_fail).copied().collect();
    hard.sort_by(|(a, _), (b, _)| {
        b.failures
            .cmp(&a.failures)
            .then_with(|| a.weakest.partial_cmp(&b.weakest).unwrap_or(Ordering::Equal))
    });

    // `leaf_composable_zeros` is deprecated and always empty -- see the
    // schema field. The structural-leaf carve-out existed to keep
    // `mdg.instability` failures out of `hard_fails`; that metric is now
    // advisory, so it cannot produce a hard fail to suppress.
    let leaves: Vec<WorstFileEntry> = Vec::new();

    let mut giants: Vec<KeyedRow<'a>> = keyed.iter().filter(|(k, _)| k.giant).copied().collect();
    giants.sort_by(|(a, _), (b, _)| {
        b.cyclomatic
            .partial_cmp(&a.cyclomatic)
            .unwrap_or(Ordering::Equal)
    });

    (
        hard.iter()
            .map(|(_, row)| to_worst_entry(&row.entry))
            .collect(),
        leaves,
        giants
            .iter()
            .map(|(_, row)| to_worst_entry(&row.entry))
            .collect(),
    )
}

/// Undecorated entry point kept for the ranking-list tests.
#[cfg(test)]
fn classify_project_rows(
    rows: &[ScoredProjectRow],
) -> (
    Vec<WorstFileEntry>,
    Vec<WorstFileEntry>,
    Vec<WorstFileEntry>,
) {
    classify_keyed_rows(&decorate_rows(rows))
}

/// Ranking key for one file. **Total order: never `NaN`.**
///
/// The ranking comparators sort with `partial_cmp(..).unwrap_or(Equal)`,
/// which would silently degrade a `NaN` key to "equal" and make the result
/// depend on comparison order rather than on the data. That cannot happen,
/// on three independent grounds:
///
/// 1. **No scored metric can be `NaN` at the source.** Every gated or
///    advisory metric is either an integer count widened to `f64`
///    (`cfg.cyclomatic` = `usize as f64` in `graphs/cfg/object.rs`,
///    `ast.max_function_complexity`, `mdg.fan_in`/`fan_out`,
///    `cpg.dangerous_calls`/`taint_flows`) or a division whose zero
///    denominator is guarded before the divide (`ast.entropy` returns `0.0`
///    for empty source, `mdg.instability` returns `0.5` below
///    `MIN_RESOLVABLE_COUPLING`, `mdg.abstractness` returns `0.0` at
///    `total == 0`). `nav.max_function_divergence` is
///    `Σ depth·ln(1 + fanout)` with `fanout >= 0`, so every term is a
///    finite `ln` of `>= 1`.
/// 2. **The desirability curves absorb `NaN` anyway.** Every curve in
///    `evaluation::policies::desirability` maps `NaN` to `0.0`, and
///    `band_score` clamps its inputs, so a hypothetical `NaN` metric yields
///    a finite score and `ScoredDecision::score` stays in `[0, 1]`. `weakest_score`'s `reduce(f64::min)` inherits the same
///    property.
/// 3. **The cyclomatic branch is unreachable with `NaN`.** It runs only
///    behind `is_maintainability_giant`, whose `*v > SIMPLE.max_cyclomatic`
///    test is `false` for `NaN`.
///
/// Note this invariant does *not* rest on the v0.4.0 "gates fail closed on
/// `NaN`" guard (`policies::gates::classify`): that one hardens `achieved`
/// at the *comparison*, and deliberately leaves `raw_metrics` unsanitized.
fn assert_total_order(key: FileKey, filepath: &str) {
    debug_assert!(
        !key.1.is_nan() && !key.2.is_nan() && !key.3.is_nan(),
        "ranking sort key must be totally ordered, got {key:?} for {}",
        filepath
    );
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
            let rows: Vec<ScoredProjectRow> = match (lang_entries, lang_results) {
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
            let keyed = decorate_rows(&rows);
            // Only the single worst row is used; the cached file key gives
            // the same first-wins tie behavior as the previous stable sort.
            let worst = keyed
                .iter()
                .min_by(|(a, _), (b, _)| {
                    a.file_key
                        .partial_cmp(&b.file_key)
                        .unwrap_or(Ordering::Equal)
                })
                .map(|(_, row)| &row.entry);
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
    scan_ms: u64,
    /// Files whose evaluation failed outright (unreadable); not in `entries`.
    unread_files: usize,
    /// Their absolute paths.
    unread_paths: Vec<String>,
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
    let project_warnings = project_warnings(&args, &args.entries);
    let rows: Vec<ScoredProjectRow> = if args.entries.len() == args.per_file_results.len() {
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

    // One definition of "worst" for every field that names a file: the
    // order of `files` (`RowKeys::file_key`). Sort first, then read the
    // head of the same order everywhere.
    let mut keyed = decorate_rows(&rows);
    sort_keyed(&mut keyed);
    let (hard_fails, leaf_composable_zeros, maintainability_giants) = classify_keyed_rows(&keyed);
    let binding_inputs = BindingInputs {
        params: args.params,
        priority: args.priority,
        gitnexus_dir: args.gitnexus_dir,
    };
    let worst_files: Vec<WorstFileEntry> = keyed
        .iter()
        .take(summary::WORST_LEN)
        .map(|(_, row)| worst_entry(row, &binding_inputs))
        .collect();
    let entries: Vec<ProjectFileEntry> = keyed.iter().map(|(_, row)| row.entry.clone()).collect();
    let hard_fail_head = entries
        .first()
        .filter(|_| keyed.first().is_some_and(|(keys, _)| keys.hard_fail));

    let aggregate_explanation = aggregate_explanation(
        overall,
        &rolled,
        &rolled_scores,
        &entries,
        args.unread_files,
    );
    let worst_file_verdict = worst_files.first().map(|w| w.lattice_element);
    let guidance = project_guidance(&entries);

    let page: Vec<ProjectFileEntry> = entries
        .iter()
        .skip(args.params.offset)
        .take(args.params.limit.clamp(1, 500))
        .cloned()
        .collect();
    let has_more = args.params.offset + page.len() < entries.len();
    let next_offset = has_more.then_some(args.params.offset + page.len());

    let contract = project_contract(
        overall,
        hard_fail_head,
        &entries,
        args.coupling_available,
        &project_warnings,
        args.parse_failures,
    );

    let mut model = ProjectEvaluationResult {
        root: args.resolved_root.to_string_lossy().to_string(),
        file_count: args.source_file_count,
        parse_failures: args.parse_failures,
        rolled_up_dimensions: rolled
            .iter()
            .map(|(dim, &val)| (dim.clone(), lattice_to_str(val)))
            .collect(),
        rolled_up_scores: rolled_scores,
        aggregate_floor_verdict: Some(overall),
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
        scan_ms: Some(args.scan_ms),
        unread_files: args.unread_files,
        unread_paths: args
            .unread_paths
            .iter()
            .take(summary::UNREAD_LISTED)
            .cloned()
            .collect(),
        response_tokens_estimate: 0,
        files: page,
        verbose: args.params.verbose,
        error: None,
    };
    // The estimate is of the whole full response: the structured content
    // and the text block.
    model.response_tokens_estimate =
        shaping::estimate_tokens(&model) + shaping::estimate_tokens(&render_project_md(&model));
    model
}

/// The warnings of a project result: the large-tree note first (so a
/// text-only client reads it before the rest), then the generation note and
/// the dependency-graph warnings. All of them are known before the agent
/// contract is built from them.
fn project_warnings(args: &BuildProjectArgs<'_>, entries: &[ProjectFileEntry]) -> Vec<String> {
    let mut warnings = if args.params.no_composable {
        Vec::new()
    } else {
        gitnexus_warnings(
            args.resolved_gitnexus_override,
            args.project_root,
            args.gitnexus_dir,
            args.any_dep_graph_loaded,
            args.last_load_error.as_deref(),
        )
    };
    if let Some(note) = &args.generation_note {
        warnings.insert(0, note.clone());
    }
    warnings.splice(
        0..0,
        large_tree::warning(
            args.source_file_count,
            args.resolved_root,
            entries,
            args.unread_files,
        ),
    );
    warnings
}

fn empty_project_result(
    params: &EvaluateProjectInput,
    priority: Priority,
    priority_source: PrioritySource,
    error: Option<String>,
) -> ProjectEvaluationResult {
    ProjectEvaluationResult {
        root: error_root(&params.path),
        file_count: 0,
        parse_failures: 0,
        rolled_up_dimensions: BTreeMap::new(),
        rolled_up_scores: BTreeMap::new(),
        aggregate_floor_verdict: None,
        language_rollups: Vec::new(),
        aggregate_explanation: String::new(),
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
        scan_ms: None,
        unread_files: 0,
        unread_paths: Vec::new(),
        response_tokens_estimate: 0,
        files: Vec::new(),
        verbose: params.verbose,
        error,
    }
}

fn aggregate_explanation(
    overall: LatticeElement,
    rolled: &BTreeMap<String, EvaluationValue>,
    rolled_scores: &BTreeMap<String, f64>,
    entries: &[ProjectFileEntry],
    unread: usize,
) -> String {
    let Some(worst) = entries.first() else {
        return if unread > 0 {
            format!(
                "No file could be scored ({unread} unreadable), so the aggregate floor is SLOP."
            )
        } else {
            "No files were evaluated, so the aggregate floor is SLOP.".to_string()
        };
    };
    let target = format!(
        "worst file is {} ({})",
        worst.filepath,
        worst.lattice_element.as_str()
    );
    if rolled.is_empty() {
        return format!(
            "Aggregate floor is {}: no generator was measured on any file (none parsed); {target}.",
            overall.as_str()
        );
    }
    let mut failed: Vec<&String> = rolled
        .iter()
        .filter(|(_, &val)| lattice_to_str(val) == LatticeElement::SLOP)
        .map(|(dim, _)| dim)
        .collect();
    failed.sort();
    let Some(lowest) = failed.iter().min_by(|a, b| {
        let sa = rolled_scores.get(**a).copied().unwrap_or(100.0);
        let sb = rolled_scores.get(**b).copied().unwrap_or(100.0);
        sa.partial_cmp(&sb).unwrap_or(Ordering::Equal)
    }) else {
        let unread_note = if unread > 0 {
            format!(" ({unread} unreadable not counted)")
        } else {
            String::new()
        };
        return format!(
            "Aggregate floor is {}: every scored file satisfies every measured \
             generator{unread_note}; {target}.",
            overall.as_str()
        );
    };
    let score_text = rolled_scores
        .get(*lowest)
        .map(|s| format!(" (lowest file score {s:.1}%)"))
        .unwrap_or_default();
    let names: Vec<&str> = failed.iter().map(|dim| dim.as_str()).collect();
    format!(
        "Aggregate floor is {}: at least one file does not satisfy {}{score_text}; {target}.",
        overall.as_str(),
        names.join(", ")
    )
}

fn project_guidance(entries: &[ProjectFileEntry]) -> String {
    let Some(worst) = entries.first() else {
        return "No files were evaluated.".to_string();
    };
    let path = &worst.filepath;
    if !worst.scores.is_empty() {
        let dim = worst
            .scores
            .iter()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(Ordering::Equal))
            .map(|(dim, _)| dim.clone())
            .unwrap_or_default();
        return format!("Start with `{path}`; weakest measured generator is {dim}.");
    }
    format!("Start with `{path}`; inspect parseability and raw metrics.")
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

/// The floor verdict, and what it is.
fn overall_line(r: &ProjectEvaluationResult) -> Option<String> {
    let floor = r.aggregate_floor_verdict?;
    let unread = if r.unread_files > 0 {
        format!("; {} unreadable not counted", r.unread_files)
    } else {
        String::new()
    };
    Some(format!(
        "**Overall:** {} (the pillars every scored file holds{unread})",
        floor.as_str()
    ))
}

/// The header: the scanned directory, when there is one.
fn project_title(r: &ProjectEvaluationResult) -> String {
    if r.root.is_empty() {
        "# Project Evaluation".to_string()
    } else {
        format!("# Project Evaluation — {}", r.root)
    }
}

pub(crate) fn render_project_md(r: &ProjectEvaluationResult) -> String {
    let mut lines = vec![project_title(r), String::new()];
    if let Some(error) = &r.error {
        lines.push(format!("> ⚠️ **Error:** {error}"));
        lines.push(String::new());
    }
    lines.extend(overall_line(r));
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
    lines.extend(summary::offset_past_end(r).map(|note| format!("\n_{note}._")));
    if let Some(error) = &r.error {
        lines.push(format!("\n> error: {error}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scanned directory of the fixtures below.
    const ROOT: &str = "/tmp/topos-project-test";

    /// The absolute path of fixture file `name`.
    fn abs(name: &str) -> String {
        format!("{ROOT}/{name}")
    }

    fn json_of(summary: &summary::Summary) -> serde_json::Value {
        serde_json::to_value(summary).expect("serialize summary")
    }

    fn entry(filepath: &str, simple_score: f64) -> ProjectFileEntry {
        ProjectFileEntry {
            filepath: abs(filepath),
            language: "python".to_string(),
            lattice_element: LatticeElement::SIMPLE,
            scores: BTreeMap::from([("simple".to_string(), simple_score)]),
            pillars: BTreeMap::from([("simple".to_string(), ProjectPillar { achieved: true })]),
            raw_metrics: BTreeMap::from([("cfg.cyclomatic".to_string(), 3.0)]),
            advisories: BTreeMap::new(),
            warnings: Vec::new(),
            security_findings: Vec::new(),
            security_advisory: None,
            acknowledged_risks: Vec::new(),
            raw_lattice_element: None,
            adjusted_lattice_element: None,
            secure_raw: None,
            secure_adjusted: None,
            grade_capped: false,
            is_parseable: true,
        }
    }

    #[test]
    fn refreshed_row_carries_population_advisories() {
        let mut results: Vec<ClassificationResult> = [2.0, 3.0, 30.0]
            .into_iter()
            .map(|cc| ClassificationResult {
                is_parseable: true,
                language: "python".to_string(),
                raw_metrics: BTreeMap::from([("cfg.cyclomatic".to_string(), cc)]),
                gate_scores: BTreeMap::from([("simple".to_string(), 0.8)]),
                ..Default::default()
            })
            .collect();
        rescore_population(&mut results);
        let mut row = entry("worst.py", 0.0);
        refresh_entry_scores(&mut row, &results[2], false);
        let reading = &row.advisories["cfg.cyclomatic"];
        assert!(reading["local_weight"].as_f64().unwrap() > 0.0);
        assert_eq!(
            row.scores["simple"],
            (results[2].scores["simple"] * 1000.0).round() / 10.0
        );
        assert!(
            row.scores["simple"] >= 50.0,
            "advisories never cross the gate"
        );
    }

    fn project_params(offset: usize) -> EvaluateProjectInput {
        EvaluateProjectInput {
            path: ".".to_string(),
            preferences: None,
            gitnexus_dir: None,
            no_composable: true,
            detail: Detail::Summary,
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
            scan_ms: 7,
            unread_files: 0,
            unread_paths: Vec::new(),
        })
    }

    /// `build_language_rollups` picks the worst row with `min_by` instead of
    /// sorting. Pins both halves of that equivalence: the minimum by
    /// `RowKeys::file_key`, and first-wins on a tie (what `first()`
    /// after a stable `sort_by` gave).
    #[test]
    fn language_rollup_worst_file_is_the_minimum_and_ties_keep_the_first() {
        let rollup = |entries: Vec<ProjectFileEntry>| {
            build_language_rollups(
                &HashMap::from([("python".to_string(), Vec::new())]),
                &HashMap::from([("python".to_string(), entries)]),
                &HashMap::new(),
            )
            .remove(0)
        };

        let ranked = rollup(vec![
            entry("mid.py", 50.0),
            entry("worst.py", 10.0),
            entry("best.py", 90.0),
        ]);
        assert_eq!(ranked.file_count, 3);
        assert_eq!(
            ranked.worst_file_path.as_deref(),
            Some(abs("worst.py").as_str())
        );

        let tied = rollup(vec![
            entry("first.py", 10.0),
            entry("second.py", 10.0),
            entry("best.py", 90.0),
        ]);
        assert_eq!(
            tied.worst_file_path.as_deref(),
            Some(abs("first.py").as_str())
        );
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
        assert_eq!(
            &paths[..3],
            [abs("worst.py"), abs("second.py"), abs("third.py")]
        );
        assert_eq!(paths.len(), 5, "all five rows of the fixture");
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
        assert!(!second_page
            .files
            .iter()
            .any(|f| f.filepath == abs("worst.py")));

        // Slim by construction: identity plus verdict, no row payload.
        for list in [
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

    /// A structural leaf (`I = 0.0`, no symbol fan) used to need an explicit
    /// carve-out to stay out of `hard_fails`. `mdg.instability` is advisory
    /// now, so it produces no hard fail to suppress and the deprecated
    /// `leaf_composable_zeros` bucket stays empty.
    #[test]
    fn structural_leaf_does_not_hard_fail_without_a_carve_out() {
        let entry = entry("leaf.py", 95.0);
        let rows = vec![classified_row(composable_leaf_result(), entry)];
        let (hard, leaves, giants) = classify_project_rows(&rows);
        assert!(hard.is_empty(), "structural leaf must not hard-fail");
        assert!(leaves.is_empty(), "leaf_composable_zeros is deprecated");
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
        assert_eq!(hard[0].filepath, abs("fail.py"));
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

    /// Ranking lists decorate once per row: gate classification must run
    /// exactly `n` times, not once per comparison (which a sort of 8 rows
    /// would balloon well past 8).
    #[test]
    fn ranking_lists_evaluate_gates_once_per_row() {
        let rows: Vec<ScoredProjectRow> = (0..8)
            .map(|i| {
                let mut result = ClassificationResult {
                    is_parseable: true,
                    ..Default::default()
                };
                result.raw_metrics.insert(
                    "cfg.cyclomatic".to_string(),
                    SIMPLE.max_cyclomatic + f64::from(i) + 1.0,
                );
                classified_row(result, entry(&format!("f{i}.py"), 90.0 - f64::from(i)))
            })
            .collect();

        GATE_EVAL_COUNT.with(|c| c.set(0));
        let (_, _, giants) = classify_project_rows(&rows);
        assert_eq!(
            GATE_EVAL_COUNT.with(std::cell::Cell::get),
            rows.len(),
            "gate classification must run once per row, not once per comparison"
        );
        assert_eq!(giants.len(), rows.len());
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
        assert_eq!(giants[0].filepath, abs("big.py"));
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
        let (_, entry, ..) = evaluate_single_file(
            &path,
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

    /// #232 and #471: a project row's `pillars` carry only `achieved`
    /// because the pillar score is exactly `scores[pillar]`. Pins that
    /// equality for the generic builder, and the overlay's SECURE lift
    /// (scores are gate-anchored, so a pass is at least 50).
    #[test]
    fn project_row_pillar_scores_equal_row_scores_under_security_overlay() {
        let path = write_temp_source("overlay_secure", "def f(expr):\n    return eval(expr)\n");
        let (result, entry, ..) = evaluate_single_file(
            &path,
            Priority::Simple,
            None,
            /* include_security_findings = */ false,
            /* verbose = */ false,
            &["eval".to_string()],
        )
        .expect("classify temp file");

        let built = build_pillars(&result, false);
        assert!(!built.is_empty());
        for (dim, pillar) in &built {
            if let Some(score) = entry.scores.get(dim) {
                assert_eq!(
                    *score, pillar.score,
                    "{dim}: scores and pillar score differ"
                );
            }
        }
        let secure = entry.scores.get("secure").copied().expect("secure score");
        assert!(
            secure >= 50.0,
            "a passing pillar never scores below the gate: {secure}"
        );
        assert!(
            entry.pillars["secure"].achieved,
            "the allowlisted eval call must still achieve SECURE via the overlay verdict"
        );
        let json = serde_json::to_value(&entry).expect("serialize");
        assert!(json["pillars"]["secure"].get("score").is_none());
    }

    // ---- #471 summary-first response, #474 one definition of "worst" ----

    /// A scored row: its entry scores, raw gate metrics, and the per-pillar
    /// verdicts `combine_dimensions` reads (SIMPLE is lost when
    /// `ast.max_function_complexity` is over its gate).
    fn gated_row(
        name: &str,
        scores: &[(&str, f64)],
        metrics: &[(&str, f64)],
    ) -> (ClassificationResult, ProjectFileEntry) {
        let mut row = entry(name, 0.0);
        row.scores = scores.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let raw_metrics: BTreeMap<String, f64> =
            metrics.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        let simple_ok = raw_metrics
            .get("ast.max_function_complexity")
            .is_none_or(|v| *v <= SIMPLE.max_function_complexity);
        let mut dimensions = BTreeMap::from([
            ("secure".to_string(), EvaluationValue::Secure),
            ("navigable".to_string(), EvaluationValue::Navigable),
        ]);
        dimensions.insert(
            "simple".to_string(),
            if simple_ok {
                EvaluationValue::Simple
            } else {
                EvaluationValue::Slop
            },
        );
        let result = ClassificationResult {
            is_parseable: true,
            raw_metrics,
            dimensions,
            ..Default::default()
        };
        (result, row)
    }

    fn project_from_with(
        rows: Vec<(ClassificationResult, ProjectFileEntry)>,
        params: &EvaluateProjectInput,
        source_file_count: usize,
        unread_files: usize,
    ) -> ProjectEvaluationResult {
        let (per_file_results, entries): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
        let root = Path::new("/tmp/topos-project-test");
        build_project_result(BuildProjectArgs {
            resolved_root: root,
            source_file_count,
            parse_failures: 0,
            per_file_results,
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
            scan_ms: 12,
            unread_files,
            unread_paths: Vec::new(),
        })
    }

    fn project_from(
        rows: Vec<(ClassificationResult, ProjectFileEntry)>,
        params: &EvaluateProjectInput,
    ) -> ProjectEvaluationResult {
        let count = rows.len();
        project_from_with(rows, params, count, 0)
    }

    /// Fixture where the old fields named three different files: in walk
    /// order `a` fails one gate (the old `guidance` and `agent_contract`
    /// head, first hard fail in walk order), `b` fails two (the old
    /// `hard_fails[0]`) and `c` passes with the lowest score (the old
    /// `worst_files[0]`).
    fn disagreeing_rows() -> Vec<(ClassificationResult, ProjectFileEntry)> {
        vec![
            gated_row(
                "a.py",
                &[("simple", 40.0), ("secure", 90.0)],
                &[("ast.max_function_complexity", 30.0)],
            ),
            gated_row(
                "b.py",
                &[("simple", 30.0), ("secure", 20.0)],
                &[
                    ("ast.max_function_complexity", 30.0),
                    ("cpg.dangerous_calls", 2.0),
                ],
            ),
            gated_row("c.py", &[("simple", 5.0), ("secure", 95.0)], &[]),
        ]
    }

    #[test]
    fn every_field_names_the_same_worst_file() {
        let model = project_from(disagreeing_rows(), &project_params(0));
        let order: Vec<&str> = model.files.iter().map(|f| f.filepath.as_str()).collect();
        assert_eq!(
            order,
            vec![abs("b.py"), abs("a.py")],
            "two failed gates rank first"
        );
        assert_eq!(model.worst_files[0].filepath, abs("b.py"));
        assert_eq!(model.hard_fails[0].filepath, abs("b.py"));
        assert!(
            model
                .aggregate_explanation
                .contains("worst file is /tmp/topos-project-test/b.py"),
            "{}",
            model.aggregate_explanation
        );
        assert!(
            model.guidance.contains("`/tmp/topos-project-test/b.py`"),
            "{}",
            model.guidance
        );
        let contract = model.agent_contract.expect("contract");
        assert!(
            contract
                .next_actions
                .iter()
                .any(|a| a.contains("`/tmp/topos-project-test/b.py`")),
            "{:?}",
            contract.next_actions
        );
        assert_eq!(
            model.worst_file_verdict,
            Some(model.worst_files[0].lattice_element)
        );
    }

    #[test]
    fn worst_files_name_the_weakest_pillar_and_count_failed_gates() {
        let model = project_from(disagreeing_rows(), &project_params(0));
        let b = &model.worst_files[0];
        assert_eq!(b.gates_failed, 2);
        // secure (20%) is b's lowest-scoring pillar.
        assert_eq!(b.pillar.as_deref(), Some("secure"));
        assert_eq!(b.score, Some(20.0));
        // The fixture's paths do not exist, so there is nothing to bind:
        // the field is present and `null`, as `topos_evaluate_file` has it.
        assert_eq!(b.binding_constraint, Some(None));
        let c = &model.worst_files[2];
        assert_eq!(c.pillar.as_deref(), Some("simple"));
        assert_eq!(c.score, Some(5.0));
        assert_eq!(c.gates_failed, 0);
    }

    /// #474: `simple: SLOP` beside a `SECURE_NAVIGABLE` floor is the lattice
    /// element of the dimensions every file satisfies, and the explanation
    /// says so instead of "Aggregate floor is SLOP".
    #[test]
    fn floor_verdict_and_explanation_agree_with_rolled_up_dimensions() {
        let model = project_from(disagreeing_rows(), &project_params(0));
        assert_eq!(model.rolled_up_dimensions["simple"], LatticeElement::SLOP);
        assert_eq!(
            model.aggregate_floor_verdict,
            Some(LatticeElement::SECURE_NAVIGABLE)
        );
        assert!(
            model.aggregate_explanation.starts_with(
                "Aggregate floor is SECURE_NAVIGABLE: at least one file does not satisfy simple"
            ),
            "{}",
            model.aggregate_explanation
        );
    }

    fn many_rows(n: usize) -> Vec<(ClassificationResult, ProjectFileEntry)> {
        (0..n)
            .map(|i| {
                let (result, mut row) = gated_row(
                    &format!("pkg{}/module_{i}.py", i % 7),
                    &[("simple", 20.0 + (i % 50) as f64), ("secure", 90.0)],
                    &[("ast.max_function_complexity", 11.0 + (i % 5) as f64)],
                );
                row.advisories.insert(
                    "cfg.cyclomatic".to_string(),
                    serde_json::json!({
                        "local_weight": 0.123_456_789_012_345_6,
                        "population_percentile": 87.654_321_098_765_43,
                        "z": -1.234_567_890_123_456_7,
                    }),
                );
                (result, row)
            })
            .collect()
    }

    #[test]
    fn summary_and_full_agree_on_rollup_verdict_and_worst() {
        let mut params = project_params(0);
        params.limit = 500;
        let model = project_from(many_rows(60), &params);
        let summary = summary::build(&model, 500);
        let json = json_of(&summary);
        assert_eq!(json["schema"], summary::SUMMARY_SCHEMA);
        let full = serde_json::to_value(&model).expect("serialize");
        for key in [
            "rolled_up_dimensions",
            "rolled_up_scores",
            "aggregate_floor_verdict",
            "file_count",
            "parse_failures",
            "scan_ms",
            "priority",
            "coupling_available",
            "warnings",
        ] {
            assert_eq!(json[key], full[key], "{key} must be the same in both modes");
        }
        let worst = json["worst"].as_array().expect("worst");
        assert_eq!(worst.len(), summary::WORST_LEN);
        for (index, (shown, full_entry)) in worst.iter().zip(&model.worst_files).enumerate() {
            assert_eq!(
                shown["path"], full_entry.filepath,
                "summary paths are absolute"
            );
            assert_eq!(
                shown["verdict"], full["worst_files"][index]["lattice_element"],
                "same file, same verdict"
            );
            assert_eq!(shown["binding_constraint"], serde_json::Value::Null);
        }
        assert_eq!(json["failing_files"], model.hard_fails.len());
        assert_eq!(json["giants"], model.maintainability_giants.len());
        assert_eq!(json["omitted"]["files"], 60 - summary::WORST_LEN);
        for forbidden in [
            "files",
            "hard_fails",
            "maintainability_giants",
            "advisories",
        ] {
            assert!(json.get(forbidden).is_none(), "{forbidden} is full-only");
        }
        let more = json["more"].as_str().expect("more");
        assert!(
            more.contains("detail: \"full\"") && more.contains("limit 500"),
            "{more}"
        );
        assert!(
            more.contains("path") && more.contains("topos_evaluate_file"),
            "{more}"
        );
    }

    /// Summary JSON for a 400-file tree with every file failing a gate
    /// stays under this; the full page of 25 rows of the same tree is far
    /// larger.
    const SUMMARY_CEILING_CHARS: usize = 4_000;
    const TEXT_CEILING_CHARS: usize = 2_000;

    #[test]
    fn summary_size_is_independent_of_file_count_and_under_ceiling() {
        let mut params = project_params(0);
        params.limit = 25;
        let small = summary::build(&project_from(many_rows(30), &params), 25);
        let model = project_from(many_rows(400), &params);
        let large = summary::build(&model, 25);
        let chars = serde_json::to_string(&json_of(&large)).unwrap().len();
        assert!(chars <= SUMMARY_CEILING_CHARS, "summary is {chars} chars");
        let small_chars = serde_json::to_string(&json_of(&small)).unwrap().len();
        assert!(
            chars.abs_diff(small_chars) < 200,
            "summary grew with file count: {small_chars} -> {chars}"
        );
        let full_page = serde_json::to_string(&model.files).unwrap().len();
        assert!(
            full_page > 3 * chars,
            "full page {full_page} vs summary {chars}"
        );
        let whole =
            shaping::estimate_tokens(&model) + shaping::estimate_tokens(&render_project_md(&model));
        assert!(
            model.response_tokens_estimate.abs_diff(whole) <= 20,
            "{} vs {whole}",
            model.response_tokens_estimate
        );
        assert!(model.response_tokens_estimate > full_page / 4);
    }

    #[test]
    fn summary_text_block_is_compact_and_names_worst_files() {
        let mut params = project_params(0);
        params.limit = 25;
        let mut model = project_from(many_rows(400), &params);
        model.root = "/Users/someone/Repos/a-long-repository-name/with/nested/crates/path".into();
        model
            .warnings
            .insert(0, "large tree: ".to_string() + &"y".repeat(200));
        model.warnings.push("x".repeat(900));
        let summary = summary::build(&model, 25);
        let text = summary.to_markdown();
        assert!(
            text.len() <= TEXT_CEILING_CHARS,
            "{} chars:\n{text}",
            text.len()
        );
        assert!(text.contains("Worst, ranked by"));
        assert!(text.contains("module_"));
        assert!(
            text.contains("2 gates failed") || text.contains("simple"),
            "{text}"
        );
        assert!(text.contains("detail: \"full\""), "{text}");
        assert!(
            text.contains("400 file(s) found, 400 scored; scan 12ms"),
            "{text}"
        );
    }

    #[test]
    fn advisory_floats_round_to_three_significant_figures() {
        let mut value = serde_json::json!({"z": -1.234_567_890_123_456_7, "n": 12});
        crate::shaping::round_floats(&mut value);
        assert_eq!(value["z"], -1.23);
        assert_eq!(value["n"], 12);
    }

    #[test]
    fn large_tree_warning_lists_top_level_directories_by_count() {
        let rows: Vec<ProjectFileEntry> = (0..1_200)
            .map(|i| {
                let dir = match i % 10 {
                    0..=5 => "src",
                    6..=8 => "tests",
                    _ => "docs",
                };
                entry(&format!("{dir}/f{i}.py"), 50.0)
            })
            .chain([entry("setup.py", 50.0)])
            .collect();
        let warning = large_tree::warning(1_201, Path::new(ROOT), &rows, 0).expect("warning");
        assert!(warning.starts_with("1201 files found"), "{warning}");
        let src = warning.find("src 720").expect("src");
        let tests = warning.find("tests 360").expect("tests");
        let docs = warning.find("docs 120").expect("docs");
        assert!(src < tests && tests < docs, "{warning}");
        assert!(
            warning.contains(". 1"),
            "root files group as `.`: {warning}"
        );
        assert!(large_tree::warning(1_000, Path::new(ROOT), &rows, 0).is_none());
    }

    #[test]
    fn large_tree_warning_reaches_both_modes() {
        let model = project_from_with(many_rows(4), &project_params(0), 5_000, 0);
        assert!(model
            .warnings
            .iter()
            .any(|w| w.starts_with("5000 files found")));
        let summary = summary::build(&model, 2);
        assert_eq!(summary.warnings, model.warnings);
    }

    #[test]
    fn detail_defaults_to_summary() {
        let parsed: EvaluateProjectInput =
            serde_json::from_value(serde_json::json!({"path": "."})).expect("parse");
        assert_eq!(parsed.detail, Detail::Summary);
        let full: EvaluateProjectInput =
            serde_json::from_value(serde_json::json!({"path": ".", "detail": "full"}))
                .expect("parse");
        assert_eq!(full.detail, Detail::Full);
    }

    /// The `detail` dispatch itself: default is the summary, `full` is the
    /// per-file document, both from one real scan.
    #[test]
    fn evaluate_project_returns_summary_by_default_and_rows_on_full() {
        let path = write_temp_source(
            "detail_dispatch",
            "def f(x):\n    if x:\n        return 1\n    return 0\n",
        );
        let parent = path.parent().expect("parent");
        std::fs::write(parent.join("pyproject.toml"), "[project]\nname = \"t\"\n")
            .expect("write project marker");
        let dir = parent.to_string_lossy().to_string();
        let params = |detail: Detail| EvaluateProjectInput {
            path: dir.clone(),
            detail,
            limit: 25,
            ..project_params(0)
        };
        let summary = evaluate_project_sync_with_cancel(params(Detail::default()), None, None)
            .expect("not cancelled");
        let structured = summary.structured_content.expect("structured");
        assert_eq!(
            structured["schema"],
            summary::SUMMARY_SCHEMA,
            "{structured}"
        );
        assert!(structured.get("files").is_none());
        assert_eq!(structured["file_count"], 1);
        assert!(structured["worst"][0]["path"]
            .as_str()
            .is_some_and(|p| p.ends_with("detail_dispatch.py") && Path::new(p).is_absolute()));

        let full = evaluate_project_sync_with_cancel(params(Detail::Full), None, None)
            .expect("not cancelled");
        let structured = full.structured_content.expect("structured");
        assert_eq!(structured["files"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            structured["root"]
                .as_str()
                .map(Path::new)
                .map(Path::is_absolute),
            Some(true)
        );
    }

    /// A scanned project of `files` (name, source) and its absolute root.
    fn scanned_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_mcp_evaluate_project_{}_{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create project dir");
        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = \"t\"\n")
            .expect("write project marker");
        for (file, source) in files {
            std::fs::write(dir.join(file), source).expect("write source");
        }
        dir.canonicalize().expect("canonicalize")
    }

    fn structured_of(result: CallToolResult) -> serde_json::Value {
        result.structured_content.expect("structured content")
    }

    /// `topos_evaluate_file`'s `binding_constraint` for `file`, `null` when
    /// it reports none.
    fn file_binding(file: &Path, allow: &[&str]) -> serde_json::Value {
        let params: EvaluateFileInput = serde_json::from_value(serde_json::json!({
            "filepath": file.to_string_lossy(),
            "no_composable": true,
            "allow": allow,
        }))
        .expect("file params");
        let result = evaluate_file_sync_with_cancel(params, None).expect("not cancelled");
        structured_of(result)
            .get("binding_constraint")
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    }

    fn project_full(dir: &Path, allow: &[&str]) -> serde_json::Value {
        let params: EvaluateProjectInput = serde_json::from_value(serde_json::json!({
            "path": dir.to_string_lossy(),
            "no_composable": true,
            "detail": "full",
            "allow": allow,
        }))
        .expect("project params");
        structured_of(evaluate_project_sync_with_cancel(params, None, None).expect("not cancelled"))
    }

    const NESTED: &str = "def f(a, b, c, d, e, g):\n    if a:\n        if b:\n            if c:\n                if d:\n                    if e:\n                        if g:\n                            return 1\n    return 0\n";

    fn branchy(branches: usize) -> String {
        let mut source = String::from("def g(x):\n");
        for i in 0..branches {
            source.push_str(&format!("    if x == {i}:\n        return {i}\n"));
        }
        source.push_str("    return -1\n");
        source
    }

    /// For each worst file, `binding_constraint` is exactly what
    /// `topos_evaluate_file` reports for that file: nested versus complex
    /// code, a dangerous call beside complexity, and an allowlisted call.
    #[test]
    fn worst_files_bind_the_same_metric_as_evaluate_file() {
        let complex = branchy(14);
        let both = format!("{}\ndef h(x):\n    return eval(x)\n", branchy(14));
        let files = [
            ("nested.py", NESTED),
            ("complex.py", complex.as_str()),
            ("both.py", both.as_str()),
            ("allowed.py", "def h(x):\n    return eval(x)\n"),
        ];
        for allow in [&[][..], &["eval"][..]] {
            let name = format!("binding_{}", allow.len());
            let dir = scanned_project(&name, &files);
            let worst = project_full(&dir, allow)["worst_files"].clone();
            let worst = worst.as_array().expect("worst_files");
            assert_eq!(worst.len(), files.len());
            let mut bound = 0;
            for entry in worst {
                let path = Path::new(entry["filepath"].as_str().expect("path"));
                assert!(path.is_absolute());
                let from_project = entry
                    .get("binding_constraint")
                    .expect("present, null when none");
                let from_file = file_binding(path, allow);
                assert_eq!(*from_project, from_file, "{path:?} with allow {allow:?}");
                bound += usize::from(!from_file.is_null());
            }
            assert!(bound >= 2, "the fixtures must bind something: {worst:?}");
        }
    }

    /// With `eval` allowlisted the verdict holds SECURE, and so must every
    /// count: no failed gate, no failing file, no security binding.
    #[test]
    fn an_allowlisted_call_is_not_counted_as_a_failure() {
        let files = [("allowed.py", "def h(x):\n    return eval(x)\n")];
        let flagged = project_full(&scanned_project("allow_off", &files), &[]);
        assert_eq!(flagged["hard_fails"].as_array().map(Vec::len), Some(1));
        assert_eq!(flagged["worst_files"][0]["gates_failed"], 1);

        let allowed = project_full(&scanned_project("allow_on", &files), &["eval"]);
        assert!(allowed["rolled_up_dimensions"]["secure"] != "SLOP");
        assert!(allowed.get("hard_fails").is_none(), "{allowed}");
        assert!(allowed["worst_files"][0].get("gates_failed").is_none());
        assert_eq!(
            allowed["worst_files"][0]["binding_constraint"],
            serde_json::Value::Null
        );
    }

    /// Equal failures and equal score: the file that holds fewer pillars
    /// ranks first (the ingest.py / test_mcp_stdio.py tie).
    #[test]
    fn equal_failures_and_score_rank_by_fewest_pillars_held() {
        let (result_a, mut a) = gated_row(
            "a_navigable.py",
            &[("simple", 0.0)],
            &[("ast.max_function_complexity", 36.0)],
        );
        a.pillars = BTreeMap::from([
            ("simple".to_string(), ProjectPillar { achieved: false }),
            ("navigable".to_string(), ProjectPillar { achieved: true }),
        ]);
        let (result_b, mut b) = gated_row(
            "b_slop.py",
            &[("simple", 0.0)],
            &[("ast.max_function_complexity", 59.0)],
        );
        b.pillars = BTreeMap::from([
            ("simple".to_string(), ProjectPillar { achieved: false }),
            ("navigable".to_string(), ProjectPillar { achieved: false }),
        ]);
        let model = project_from(vec![(result_a, a), (result_b, b)], &project_params(0));
        assert_eq!(model.worst_files[0].filepath, abs("b_slop.py"));
    }

    /// A value 0.1996 under a 0.2 floor must not read as `0.2`, `0.2`, `min`.
    #[test]
    fn summary_values_stay_apart_from_their_limit() {
        let mut model = project_from(many_rows(1), &project_params(0));
        model.worst_files[0].binding_constraint = Some(Some(crate::schemas::BindingConstraint {
            pillar: "simple".to_string(),
            metric: "ast.entropy".to_string(),
            value: Some(0.1996),
            threshold: Some(0.2),
            symbol: None,
            line_start: None,
            line_end: None,
        }));
        let summary = summary::build(&model, 25);
        let binding = summary.worst[0].binding_constraint.as_ref().unwrap();
        assert_eq!(binding.value, Some(0.1996));
        assert_eq!(binding.threshold, Some(0.2));
        assert_eq!(binding.bound, Some("min"));
        let text = summary.to_markdown();
        assert!(
            text.contains("binding: ast.entropy 0.1996 (min 0.2)"),
            "{text}"
        );
        assert!(summary.metrics.contains_key("ast.entropy"));
    }

    /// The estimate and the text name the limit that was asked for.
    #[test]
    fn token_estimate_follows_the_requested_limit() {
        let estimate = |limit: usize| {
            let mut params = project_params(0);
            params.limit = limit;
            let model = project_from(many_rows(60), &params);
            let more = json_of(&summary::build(&model, limit))["more"]
                .as_str()
                .unwrap()
                .to_string();
            (model.response_tokens_estimate, more)
        };
        let (small, small_more) = estimate(5);
        let (large, large_more) = estimate(50);
        assert!(large > 2 * small, "{small} vs {large}");
        assert!(small_more.contains("at limit 5") && large_more.contains("at limit 50"));
    }

    /// Errors carry no verdict or rollup for a path that was never scanned.
    #[test]
    fn errors_report_no_verdict_or_rollup() {
        let response = project_error(
            &project_params(0),
            Priority::Simple,
            PrioritySource::Default,
            "boom".into(),
        );
        let structured = response.structured_content.unwrap();
        for absent in [
            "aggregate_floor_verdict",
            "rolled_up_dimensions",
            "held_by_every_file",
        ] {
            assert!(structured.get(absent).is_none(), "{absent}");
        }
        let mut full = project_params(0);
        full.detail = Detail::Full;
        let structured =
            project_error(&full, Priority::Simple, PrioritySource::Default, "x".into())
                .structured_content
                .unwrap();
        for absent in [
            "aggregate_floor_verdict",
            "rolled_up_dimensions",
            "aggregate_explanation",
        ] {
            assert!(structured.get(absent).is_none(), "{absent}");
        }
    }

    /// The headline says what the floor verdict is, and giants are defined.
    #[test]
    fn text_defines_the_floor_the_ranking_and_giants() {
        let model = project_from(disagreeing_rows(), &project_params(0));
        let text = summary::build(&model, 25).to_markdown();
        assert!(
            text.contains(
                "floor `SECURE_NAVIGABLE` (the pillars every scored file holds: navigable, secure)"
            ),
            "{text}"
        );
        assert!(text.contains("Worst, ranked by failures"), "{text}");
        assert!(
            text.contains("giants 0 (advisory cyclomatic above the SIMPLE band"),
            "{text}"
        );
        let md = render_project_md(&model);
        assert!(md.contains("(the pillars every scored file holds)"), "{md}");
    }

    #[test]
    fn the_large_tree_warning_alone_sets_the_warnings_risk_flag() {
        let flags = |count: usize| {
            project_from_with(many_rows(3), &project_params(0), count, 0)
                .agent_contract
                .expect("contract")
                .risk_flags
        };
        assert!(flags(5_000).contains(&"warnings".to_string()));
        assert!(!flags(3).contains(&"warnings".to_string()));
    }

    #[test]
    fn a_large_tree_with_nothing_scored_reads_as_a_sentence() {
        let warning = large_tree::warning(1_005, Path::new(ROOT), &[], 1_005).expect("warning");
        assert_eq!(
            warning,
            "1005 files found; none could be scored (1005 unreadable). `path` can name a \
             narrower directory."
        );
    }

    /// Warnings are never cut or merged, only clipped per warning and joined
    /// onto one line each.
    #[test]
    fn every_warning_keeps_its_own_clipped_line_in_the_text_block() {
        let mut model = project_from(many_rows(40), &project_params(0));
        model.warnings = (0..3)
            .map(|i| format!("warning {i}\nsecond line {}", "z".repeat(400)))
            .collect();
        let text = summary::build(&model, 25).to_markdown();
        for i in 0..3 {
            assert!(
                text.contains(&format!("Warning: warning {i} second line z")),
                "{text}"
            );
        }
        assert!(
            !text.contains("\nsecond line"),
            "continuation lines are joined"
        );
        assert!(text.contains('…'), "each is clipped");
    }

    #[test]
    fn the_past_the_end_note_counts_scored_files_when_some_were_unreadable() {
        let mut params = project_params(80);
        params.limit = 25;
        let model = project_from_with(many_rows(60), &params, 62, 2);
        let more = json_of(&summary::build(&model, 25))["more"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            more.contains("offset 80 is past the last of 60 scored files"),
            "{more}"
        );
        assert!(render_project_md(&model).contains("past the last of 60 scored files"));
        let text = summary::build(&model, 25).to_markdown();
        assert!(!text.contains("Omitted: files 0"), "{text}");
    }

    #[test]
    fn nothing_held_serializes_an_empty_list_and_an_error_omits_it() {
        let (mut result, row) = gated_row("a.py", &[("simple", 1.0)], &[]);
        for dim in ["simple", "secure", "navigable"] {
            result
                .dimensions
                .insert(dim.to_string(), EvaluationValue::Slop);
        }
        let model = project_from(vec![(result, row)], &project_params(0));
        let json = json_of(&summary::build(&model, 25));
        assert_eq!(json["held_by_every_file"], serde_json::json!([]));
        assert!(summary::build(&model, 25)
            .to_markdown()
            .contains("the pillars every scored file holds: no pillar"));

        let error = project_error(
            &project_params(0),
            Priority::Simple,
            PrioritySource::Default,
            "boom".into(),
        );
        assert!(error
            .structured_content
            .unwrap()
            .get("held_by_every_file")
            .is_none());
    }

    #[test]
    fn unreadable_files_are_named_wherever_every_file_is_claimed() {
        let rows = vec![gated_row(
            "ok.py",
            &[("simple", 90.0), ("secure", 90.0)],
            &[],
        )];
        let model = project_from_with(rows, &project_params(0), 3, 2);
        assert!(
            model.aggregate_explanation.contains(
                "every scored file satisfies every measured generator (2 unreadable not counted)"
            ),
            "{}",
            model.aggregate_explanation
        );
        let text = summary::build(&model, 25).to_markdown();
        assert!(text.contains("2 unreadable not counted"), "{text}");
        assert!(render_project_md(&model).contains("2 unreadable not counted"));
    }

    #[test]
    fn hard_fails_flag_parse_failures_like_worst_files() {
        let (mut unparsed, mut row) = gated_row("broken.py", &[], &[]);
        unparsed.is_parseable = false;
        row.is_parseable = false;
        let model = project_from(vec![(unparsed, row)], &project_params(0));
        assert!(model.hard_fails[0].parse_failure);
        assert!(model.worst_files[0].parse_failure);
    }

    /// The summary reads the same keys as `topos_evaluate_file`'s
    /// `binding_constraint`, with `bound` added.
    #[test]
    fn summary_binding_has_the_evaluate_file_keys_plus_bound() {
        let binding = crate::schemas::BindingConstraint {
            pillar: "simple".to_string(),
            metric: "ast.entropy".to_string(),
            value: Some(0.1),
            threshold: Some(0.2),
            symbol: Some("f".to_string()),
            line_start: Some(3),
            line_end: Some(9),
        };
        let keys = |value: serde_json::Value| -> Vec<String> {
            let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        let mut model = project_from(many_rows(1), &project_params(0));
        model.worst_files[0].binding_constraint = Some(Some(binding.clone()));
        let summary = json_of(&summary::build(&model, 25));
        let mut expected = keys(serde_json::to_value(&binding).unwrap());
        expected.push("bound".to_string());
        expected.sort();
        assert_eq!(
            keys(summary["worst"][0]["binding_constraint"].clone()),
            expected
        );
    }

    #[test]
    fn a_security_binding_gets_a_description_in_metrics() {
        let mut model = project_from(many_rows(1), &project_params(0));
        model.worst_files[0].binding_constraint = Some(Some(crate::schemas::BindingConstraint {
            pillar: "secure".to_string(),
            metric: "eval".to_string(),
            value: Some(1.0),
            threshold: Some(0.0),
            symbol: None,
            line_start: None,
            line_end: None,
        }));
        let summary = summary::build(&model, 25);
        assert!(summary.metrics["eval"].contains("dangerous API"));
    }

    /// `verbose` and `include_security_findings` change nothing in a
    /// summary; the summary says so and still points at `detail: "full"`.
    #[test]
    fn parameters_only_the_full_response_uses_are_stated() {
        let mut params = project_params(0);
        params.verbose = true;
        params.include_security_findings = true;
        let model = project_from(many_rows(2), &params);
        let summary = summary::build(&model, &params);
        assert_eq!(
            json_of(&summary)["apply_to_full_only"],
            serde_json::json!(["verbose", "include_security_findings"])
        );
        assert!(summary.more.is_some(), "two files, yet `more` is present");
        assert!(summary
            .to_markdown()
            .contains("verbose and include_security_findings apply to detail: \"full\" only."));
        let plain = json_of(&summary::build(&model, 25));
        assert!(plain.get("apply_to_full_only").is_none());
    }

    #[test]
    fn unreadable_files_are_listed_up_to_a_cap_and_the_rest_counted() {
        let mut model = project_from(many_rows(2), &project_params(0));
        model.unread_files = 15;
        model.unread_paths = (0..summary::UNREAD_LISTED)
            .map(|i| abs(&format!("u{i}.py")))
            .collect();
        let summary = summary::build(&model, 25);
        assert_eq!(summary.unread_paths.len(), 10);
        assert_eq!(summary.omitted.unread_files, 5);
        assert_eq!(summary.failing_files, model.hard_fails.len() + 15);
        let text = summary.to_markdown();
        assert!(
            text.contains("Unreadable: ") && text.contains("u0.py"),
            "{text}"
        );
        assert!(text.contains("(+5 not listed)"), "{text}");
        assert!(text.contains("Omitted: unread files 5."), "{text}");
    }

    #[test]
    fn full_text_says_when_the_offset_is_past_the_end() {
        let mut params = project_params(80);
        params.limit = 25;
        let model = project_from(many_rows(60), &params);
        assert!(render_project_md(&model).contains("offset 80 is past the last of 60 files"));
    }

    fn rolled_explanation(rows: Vec<(ClassificationResult, ProjectFileEntry)>) -> String {
        project_from(rows, &project_params(0)).aggregate_explanation
    }

    #[test]
    fn aggregate_explanation_covers_every_case() {
        // No files at all.
        assert_eq!(
            rolled_explanation(Vec::new()),
            "No files were evaluated, so the aggregate floor is SLOP."
        );
        // Everything passes.
        let passing = rolled_explanation(vec![gated_row(
            "ok.py",
            &[("simple", 90.0), ("secure", 90.0)],
            &[],
        )]);
        assert!(
            passing.contains("every scored file satisfies every measured generator"),
            "{passing}"
        );
        // Nothing parsed: no generator was measured, which is not "all pass".
        let (mut unparsed, mut row) = gated_row("bad.py", &[], &[]);
        unparsed.is_parseable = false;
        unparsed.dimensions.clear();
        row.is_parseable = false;
        let none = rolled_explanation(vec![(unparsed, row)]);
        assert!(
            none.contains("no generator was measured on any file") && !none.contains("satisfies"),
            "{none}"
        );
        // One file failing every pillar.
        let (mut everything, row) = gated_row(
            "all.py",
            &[("simple", 1.0), ("secure", 2.0), ("navigable", 3.0)],
            &[],
        );
        for dim in ["simple", "secure", "navigable"] {
            everything
                .dimensions
                .insert(dim.to_string(), EvaluationValue::Slop);
        }
        let all = rolled_explanation(vec![(everything, row)]);
        assert!(
            all.starts_with("Aggregate floor is SLOP: at least one file does not satisfy navigable, secure, simple"),
            "{all}"
        );
        // Files found but none readable.
        let model = {
            let params = project_params(0);
            let root = Path::new("/tmp/topos-project-test");
            build_project_result(BuildProjectArgs {
                resolved_root: root,
                source_file_count: 2,
                parse_failures: 2,
                per_file_results: Vec::new(),
                entries: Vec::new(),
                any_dep_graph_loaded: false,
                last_load_error: None,
                per_language_results: HashMap::new(),
                per_language_entries: HashMap::new(),
                per_language_parse_failures: HashMap::new(),
                params: &params,
                priority: Priority::Simple,
                priority_source: PrioritySource::Default,
                coupling_available: false,
                project_root: root,
                resolved_gitnexus_override: None,
                gitnexus_dir: None,
                generation_note: None,
                scan_ms: 1,
                unread_files: 2,
                unread_paths: vec![abs("gone.py"), abs("locked.py")],
            })
        };
        assert!(model.aggregate_explanation.contains("2 unreadable"));
        let summary = summary::build(&model, 25);
        assert_eq!(summary.failing_files, 2, "unread files count as failing");
        let text = summary.to_markdown();
        assert!(
            text.contains("2 file(s) found, 0 scored, 2 unreadable"),
            "{text}"
        );
    }

    #[test]
    fn error_paths_return_the_requested_shape_with_an_absolute_root() {
        let mut params = project_params(0);
        params.path = "relative/missing".to_string();
        let response = project_error(
            &params,
            Priority::Simple,
            PrioritySource::Default,
            "boom".into(),
        );
        let structured = response.structured_content.expect("structured");
        assert_eq!(structured["schema"], summary::SUMMARY_SCHEMA);
        assert_eq!(structured["error"], "boom");
        assert_eq!(structured["file_count"], 0);
        assert!(structured.get("scan_ms").is_none(), "no misleading 0");
        assert!(structured.get("files").is_none());
        assert!(Path::new(structured["root"].as_str().unwrap()).is_absolute());
        let text = response.content[0].as_text().unwrap().text.clone();
        assert!(text.starts_with("**Error:** boom"), "{text}");

        params.detail = Detail::Full;
        let full = project_error(
            &params,
            Priority::Simple,
            PrioritySource::Default,
            "boom".into(),
        );
        let structured = full.structured_content.expect("structured");
        assert_eq!(structured["error"], "boom");
        assert!(structured["files"].is_array());
        assert!(Path::new(structured["root"].as_str().unwrap()).is_absolute());
        assert!(structured.get("scan_ms").is_none());
    }

    /// A giant is a watch list, not a failure: it must not outrank a
    /// lower-scoring file that passes every gate, and the lowest-scoring
    /// file is what guidance and the contract pick when nothing fails.
    #[test]
    fn a_giant_ranks_by_score_like_any_other_file() {
        let giant = gated_row(
            "giant.py",
            &[("simple", 95.0)],
            &[("cfg.cyclomatic", SIMPLE.max_cyclomatic + 50.0)],
        );
        let low = gated_row("low.py", &[("simple", 10.0), ("secure", 90.0)], &[]);
        let model = project_from(vec![giant, low], &project_params(0));
        assert_eq!(model.maintainability_giants.len(), 1);
        assert_eq!(model.maintainability_giants[0].filepath, abs("giant.py"));
        assert!(model.hard_fails.is_empty());
        assert_eq!(model.worst_files[0].filepath, abs("low.py"));
        assert_eq!(model.worst_files[1].filepath, abs("giant.py"));
        assert!(model.guidance.contains("low.py"), "{}", model.guidance);
        let contract = model.agent_contract.expect("contract");
        assert!(
            contract.next_actions.iter().any(|a| a.contains("/low.py")),
            "{:?}",
            contract.next_actions
        );
        // Every entry carries a reason: the weakest pillar for passing files.
        for worst in &model.worst_files {
            assert!(worst.pillar.is_some() && worst.score.is_some(), "{worst:?}");
        }
    }

    #[test]
    fn parse_failures_rank_first_and_say_so() {
        let (mut unparsed, mut row) = gated_row("broken.py", &[], &[]);
        unparsed.is_parseable = false;
        row.is_parseable = false;
        let ok = gated_row("ok.py", &[("simple", 1.0)], &[]);
        let model = project_from(vec![ok, (unparsed, row)], &project_params(0));
        assert_eq!(model.worst_files[0].filepath, abs("broken.py"));
        assert!(model.worst_files[0].parse_failure);
        assert_eq!(model.hard_fails[0].filepath, abs("broken.py"));
        let summary = json_of(&summary::build(&model, 25));
        assert_eq!(summary["worst"][0]["parse_failure"], true);
    }

    #[test]
    fn unmeasured_pillar_has_no_score_and_is_not_achieved() {
        let result = ClassificationResult {
            is_parseable: true,
            ..Default::default()
        };
        let pillars = project_pillars(&result, false);
        assert!(!pillars["composable"].achieved);
        let mut row = entry("a.py", 50.0);
        row.pillars = pillars;
        let json = serde_json::to_value(&row).expect("serialize");
        assert!(json["scores"].get("composable").is_none());
        assert_eq!(json["pillars"]["composable"]["achieved"], false);
        assert!(json["pillars"]["composable"].get("score").is_none());
    }

    #[test]
    fn serialized_project_rows_round_advisories() {
        let mut results: Vec<ClassificationResult> = [2.0, 3.0, 30.0]
            .into_iter()
            .map(|cc| ClassificationResult {
                is_parseable: true,
                language: "python".to_string(),
                raw_metrics: BTreeMap::from([("cfg.cyclomatic".to_string(), cc)]),
                gate_scores: BTreeMap::from([("simple".to_string(), 0.8)]),
                ..Default::default()
            })
            .collect();
        rescore_population(&mut results);
        let mut row = entry("worst.py", 0.0);
        refresh_entry_scores(&mut row, &results[2], false);
        let json = serde_json::to_value(&row).expect("serialize");
        let mut floats = 0;
        let mut stack = vec![&json["advisories"]];
        while let Some(value) = stack.pop() {
            match value {
                serde_json::Value::Number(n) if n.is_f64() => {
                    floats += 1;
                    let x = n.as_f64().unwrap();
                    assert_eq!(shaping::sig3(x), x, "{x} is not rounded");
                }
                serde_json::Value::Object(map) => stack.extend(map.values()),
                serde_json::Value::Array(items) => stack.extend(items),
                _ => {}
            }
        }
        assert!(floats > 0, "advisories had no floats to check");
    }

    #[test]
    fn large_tree_warning_says_when_it_lists_only_the_top_five() {
        let rows: Vec<ProjectFileEntry> = (0..7)
            .flat_map(|d| (0..=d).map(move |i| entry(&format!("dir{d}/f{i}.py"), 50.0)))
            .collect();
        let warning = large_tree::warning(1_500, Path::new(ROOT), &rows, 3).expect("warning");
        assert!(
            warning.contains("top 5 of 7 top-level directories"),
            "{warning}"
        );
        assert!(
            warning.contains("dir6 7") && !warning.contains("dir0 1"),
            "{warning}"
        );
        assert!(warning.contains("3 unreadable file(s) are not in these counts"));
    }

    #[test]
    fn token_estimate_covers_the_whole_response_and_names_a_past_the_end_offset() {
        let mut params = project_params(80);
        params.limit = 25;
        let model = project_from(many_rows(60), &params);
        assert!(model.files.is_empty());
        assert!(
            model.response_tokens_estimate > 200,
            "hard_fails and worst_files still cost tokens: {}",
            model.response_tokens_estimate
        );
        let more = json_of(&summary::build(&model, 25))["more"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            more.contains("offset 80 is past the last of 60 files"),
            "{more}"
        );
    }

    #[test]
    fn text_block_renders_every_warning_with_the_large_tree_one_first() {
        let mut model = project_from_with(many_rows(4), &project_params(0), 5_000, 0);
        model.warnings.push("second warning".to_string());
        model.warnings.push("third warning".to_string());
        let text = summary::build(&model, 25).to_markdown();
        let large = text.find("5000 files found").expect("large-tree warning");
        let second = text.find("second warning").expect("second");
        let third = text.find("third warning").expect("third");
        assert!(large < second && second < third, "{text}");
        assert!(text.len() <= TEXT_CEILING_CHARS, "{} chars", text.len());
    }
}
