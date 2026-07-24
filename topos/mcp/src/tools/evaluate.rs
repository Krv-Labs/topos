//! Evaluation tools: code string, single file, and whole project.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::omega::{verdict_from_generators, EvaluationValue};
use topos_engine::evaluation::policies::base::Priority;

use crate::diagnostics::{overlay_for_file, overlay_for_source, SecurityOverlay};
use crate::evaluation::{
    all_source_suffixes, classify_code_string, classify_file, detect_language, ensure_gitnexus_dir,
    gitnexus_warnings,
};
use crate::formatting::{
    build_pillars, composable_contract_signals, error_md, render_evaluation_md,
    to_evaluation_result, to_tool_result, EvalResultOptions,
};
use crate::metric_locations::build_metric_locations;
use crate::refactor_targets::build_refactor_targets;
use crate::schemas::{
    lattice_to_str, priority_str, resolve_priority, AgentContract, EvaluateCodeInput,
    EvaluateFileInput, EvaluateProjectInput, EvaluationResult, LatticeElement, PrioritySource,
    ProjectEvaluationResult, ProjectFileEntry, ProjectLanguageRollup, RefactorTarget,
    SecurityFinding,
};
use crate::security::{read_resolved_utf8, resolve_file_root, resolve_within_root};
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
    /// Score a raw code string on the SIMPLE / COMPOSABLE / SECURE quality
    /// lattice (read-only; never writes or runs the code).
    ///
    /// Use for a snippet not yet on disk. Only SIMPLE and SECURE are
    /// reachable here (scored from the source's CFG/CPG); COMPOSABLE needs
    /// a module dependency graph, so for it use `topos_evaluate_file` with
    /// `gitnexus_dir`, or `topos_evaluate_project` for a whole tree.
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

    /// Score a file on disk on the SIMPLE / COMPOSABLE / SECURE lattice —
    /// the only evaluate tool that can reach COMPOSABLE (side-effecting).
    ///
    /// Unless `no_composable` is set, this generates/refreshes `.gitnexus`
    /// (given by `gitnexus_dir` or auto-detected at `<root>/.gitnexus`) when
    /// it's missing or stale, then attaches the resulting
    /// ModuleDependencyGraph — the same default behavior as the CLI's
    /// `topos evaluate`. SIMPLE/SECURE always run. When GitNexus isn't
    /// installed or generation fails, `coupling_available` is false and
    /// `warnings` explains why; the rest of the evaluation still succeeds.
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
    /// SIMPLE / COMPOSABLE / SECURE lattice, with a project rollup
    /// (side-effecting).
    ///
    /// Autodetects all supported languages (Python, Rust, JavaScript,
    /// TypeScript, C++, Go) in one walk — no language argument — and skips
    /// unsupported files. The rollup takes the project-wide minimum per
    /// dimension (weakest file floors it). Returns a paginated per-file
    /// table (worst first) plus per-language rollups; page with `limit` /
    /// `offset`.
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
    let resolved = match resolve_within_root(&params.filepath) {
        Ok(path) => path,
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

    let project_root = match resolve_file_root() {
        Ok(root) => root,
        Err(err) => {
            return err_eval(
                "Access denied / path error",
                Priority::Simple,
                priority_source,
                err,
            )
        }
    };
    let gitnexus_outcome = ensure_gitnexus_dir(
        params.gitnexus_dir.as_deref(),
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
        params.gitnexus_dir.as_deref(),
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

    // Targets are computed before the result model so the agent
    // contract can route them natively.
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

    let project_root = match resolve_file_root() {
        Ok(root) => root,
        Err(err) => {
            let model = empty_project_result(&params, priority, priority_source, Some(err));
            let md = render_project_md(&model);
            return to_tool_result(&model, md);
        }
    };
    let gitnexus_outcome = ensure_gitnexus_dir(
        params.gitnexus_dir.as_deref(),
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
    let mut scores = result.scores.clone();
    let pass = overlay.verdict.adjusted_secure_pass;
    dimensions.insert(
        "secure".to_string(),
        if pass {
            EvaluationValue::Secure
        } else {
            EvaluationValue::Slop
        },
    );
    scores.insert("secure".to_string(), if pass { 1.0 } else { 0.0 });
    ClassificationResult {
        is_parseable: result.is_parseable,
        dimensions,
        scores,
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

fn evaluate_single_file(
    path: &Path,
    resolved_root: &Path,
    priority: Priority,
    gitnexus_dir: Option<&Path>,
    include_security_findings: bool,
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
        raw_metrics: result.raw_metrics.clone(),
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
    let resolved_root = resolve_within_root(&params.path)?;
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
    lattice_to_str(verdict_from_generators(
        rolled.get("simple") == Some(&EvaluationValue::Simple),
        rolled.get("composable") == Some(&EvaluationValue::Composable),
        rolled.get("secure") == Some(&EvaluationValue::Secure),
    ))
}

fn worst_key(entry: &ProjectFileEntry) -> f64 {
    entry
        .scores
        .values()
        .fold(f64::INFINITY, |acc, &v| acc.min(v))
        .min(if entry.scores.is_empty() {
            0.0
        } else {
            f64::INFINITY
        })
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
            let mut entries: Vec<&ProjectFileEntry> = per_language_entries
                .get(language)
                .map(|v| v.iter().collect())
                .unwrap_or_default();
            entries.sort_by(|a, b| {
                worst_key(a)
                    .partial_cmp(&worst_key(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let worst = entries.first();
            ProjectLanguageRollup {
                language: language.clone(),
                file_count: entries.len(),
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
    let mut entries = args.entries;
    entries.sort_by(|a, b| {
        worst_key(a)
            .partial_cmp(&worst_key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let aggregate_explanation = aggregate_explanation(&rolled, &rolled_scores, &entries);
    let worst_files: Vec<ProjectFileEntry> = entries.iter().take(3).cloned().collect();
    let worst_file_verdict = worst_files.first().map(|w| w.lattice_element);
    let guidance = project_guidance(&worst_files);

    let page: Vec<ProjectFileEntry> = entries
        .iter()
        .skip(args.params.offset)
        .take(args.params.limit.clamp(1, 500))
        .cloned()
        .collect();
    let has_more = args.params.offset + page.len() < entries.len();
    let next_offset = has_more.then_some(args.params.offset + page.len());

    let mut project_warnings = gitnexus_warnings(
        args.params.gitnexus_dir.as_deref(),
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
        &worst_files,
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
    let worst = entries
        .iter()
        .min_by(|a, b| {
            worst_key(a)
                .partial_cmp(&worst_key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
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

fn project_guidance(worst_files: &[ProjectFileEntry]) -> String {
    let Some(worst) = worst_files.first() else {
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
    worst_files: &[ProjectFileEntry],
    coupling_available: bool,
    warnings: &[String],
    parse_failures: usize,
) -> AgentContract {
    let mut blocked_by: Vec<String> = Vec::new();
    let mut risk_flags: Vec<String> = Vec::new();
    let mut next_actions: Vec<String> = Vec::new();

    let composable = composable_contract_signals(coupling_available, warnings, true);
    blocked_by.extend(composable.blocked_by.clone());
    risk_flags.extend(composable.risk_flags.clone());
    if parse_failures > 0 {
        blocked_by.push("parse_failures".into());
        risk_flags.push("parse_failures".into());
    }
    if !warnings.is_empty() {
        risk_flags.push("warnings".into());
    }
    if worst_files.iter().any(|f| f.grade_capped) {
        risk_flags.push("grade_capped".into());
    }
    // Verdict-anchored, not payload-anchored: secure_adjusted is false
    // exactly when active findings survive the allowlist, and it is
    // unaffected by the include_security_findings payload gate.
    if worst_files.iter().any(|f| f.secure_adjusted == Some(false)) {
        risk_flags.push("active_security_findings".into());
    }

    let verification_gates = vec![
        "topos_assess_worktree_change validates each accepted in-place refactor".to_string(),
        "project rollup does not regress after non-trivial changes".to_string(),
        "behavior tests or type/lint checks pass when available".to_string(),
    ];
    if let Some(action) = composable.next_action {
        return AgentContract {
            next_tool: composable.next_tool,
            next_actions: vec![action],
            blocked_by,
            verification_gates,
            risk_flags,
        };
    }
    let Some(worst) = worst_files.first() else {
        return AgentContract {
            next_tool: None,
            next_actions,
            blocked_by,
            verification_gates: Vec::new(),
            risk_flags,
        };
    };
    let next_tool = if overall == LatticeElement::IDEAL {
        next_actions.push("preserve behavior checks before accepting".into());
        None
    } else {
        next_actions.push(format!(
            "start with worst file {} using language {}",
            worst.filepath, worst.language
        ));
        Some("topos_inspect_code".to_string())
    };

    AgentContract {
        next_tool,
        next_actions,
        blocked_by,
        verification_gates,
        risk_flags,
    }
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
