//! Shared `pr-recap` engine for schema `topos.pr_recap.v3`.
//!
//! Scores added and modified source files at `base` and `head`, groups the
//! ones that look like a split into clusters, applies the configured
//! `[pr_recap]` gates, and returns a single [`model::PrRecap`] document.

pub mod clusters;
pub mod gates;
pub mod gh;
pub mod git;
pub mod hotspots;
mod mcp;
pub mod model;
pub mod moves;
pub mod progress;
pub mod score;
pub mod store;
pub mod summary;
pub mod verdict;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResponse;
use rmcp::service::RequestContext;
use rmcp::{tool, tool_router, ErrorData as McpError, RoleServer};
use topos_engine::config::{FailOn, GateId, PrGateConfig, PrGatePreset, Severity, ToposConfig};
use topos_engine::core::omega::Omega;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::preferences::{Generator, UserPreferences, RANKING_LEN};
use topos_engine::graphs::mdg::file_graph::FileGraph;
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;
use topos_engine::graphs::mdg::split::{detect_splits, SplitReport};

use crate::schemas::PrRecapInput;
use crate::server::ToposServer;

use self::clusters::{build_clusters, changed_list};
use self::gh::{ensure_commit, git_root, merge_base, pull_request, resolve_commit};
use self::git::{
    cap_by_churn, changed_files, churn, file_change, skip_reason, worktree_files, DiffEntry,
};
use self::hotspots::top_hotspots;
pub use self::model::*;
use self::moves::RangeMoves;
use self::progress::Progress;
use self::score::Scoring;
use self::store::{
    build_estimate_ms, last_build_ms, pr_store_state, prepare_pr_stores_with_cancel, PrStores,
    StoreState,
};
use self::verdict::{added_rollup, direction_for, finish_statuses, project_rollup};
use crate::cache::{ensure_marker_for_build, PrLease};

/// Above this many files to score, the CLI warns the run may take a while;
/// `0` (`--max-files 0`) still scores everything.
pub const LARGE_RECAP_WARN_THRESHOLD: usize = 100;

/// Everything a recap is judged by: the pillar emphasis, the project
/// config (for its allowlist), the resolved gate policy with the file it
/// came from, and the date its waivers are checked against.
pub struct Judging {
    pub priority: Priority,
    pub topos: ToposConfig,
    pub gate: PrGateConfig,
    pub source: Option<PathBuf>,
    /// Today, `YYYY-MM-DD` in UTC: a waiver past its `expires` waives
    /// nothing.
    pub today: String,
    /// What the surface calls the scoring cap in hints that name it:
    /// [`gates::CLI_CAP_NAME`] on the CLI, `max_files` on the MCP tool.
    pub cap_name: &'static str,
}

/// The gate policy for one run: `--preset` replaces the project's
/// `[pr_recap]` with a built-in preset, and `--strict` then fails on warn
/// as well.
pub fn resolve_gate_policy(
    strict: bool,
    preset: Option<PrGatePreset>,
    file: &PrGateConfig,
) -> PrGateConfig {
    resolve_gate_policy_with_fail_on(strict.then_some(FailOn::Warn), preset, file)
}

/// The gate policy for one run with an optional `fail_on` override
/// (`--fail-on`, `--strict`, or `--advisory`).
pub fn resolve_gate_policy_with_fail_on(
    fail_on: Option<FailOn>,
    preset: Option<PrGatePreset>,
    file: &PrGateConfig,
) -> PrGateConfig {
    let mut policy = match preset {
        Some(preset) => PrGateConfig::for_preset(preset),
        None => file.clone(),
    };
    if let Some(fail_on) = fail_on {
        policy.fail_on = fail_on;
    }
    policy
}

pub fn parse_priority(value: &str) -> Result<Priority, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "simple" => Ok(Priority::Simple),
        "composable" => Ok(Priority::Composable),
        "secure" => Ok(Priority::Secure),
        "navigable" => Ok(Priority::Navigable),
        _ => Err(format!(
            "invalid priority '{value}' (expected simple, composable, secure, or navigable)"
        )),
    }
}

pub fn parse_ranking(value: &str) -> Result<[Generator; RANKING_LEN], String> {
    let parsed: Vec<Generator> = value
        .split(',')
        .map(|part| {
            let name = part.trim().to_ascii_lowercase();
            Generator::ALL
                .into_iter()
                .find(|g| g.as_str() == name)
                .ok_or_else(|| format!("invalid preference '{name}'"))
        })
        .collect::<Result<_, _>>()?;
    let ranking: [Generator; RANKING_LEN] = parsed.try_into().map_err(|values: Vec<_>| {
        format!(
            "preferences require all {RANKING_LEN} pillars exactly once (got {})",
            values.len()
        )
    })?;
    UserPreferences::new(ranking)
        .map(|prefs| prefs.ranking())
        .map_err(|e| e.to_string())
}

/// One `--priority` value: either a single pillar, or a full ranking
/// (comma-separated, most important first).
pub enum PriorityInput {
    Single(Priority),
    Ranking([Generator; RANKING_LEN]),
}

pub fn parse_priority_input(value: &str) -> Result<PriorityInput, String> {
    if value.contains(',') {
        parse_ranking(value).map(PriorityInput::Ranking)
    } else {
        parse_priority(value).map(PriorityInput::Single)
    }
}

pub fn priority_for_generator(generator: Generator) -> Priority {
    match generator {
        Generator::Simple => Priority::Simple,
        Generator::Composable => Priority::Composable,
        Generator::Secure => Priority::Secure,
        Generator::Navigable => Priority::Navigable,
    }
}

pub fn priority_name(priority: Priority) -> &'static str {
    priority.top_generator().as_str()
}

/// The scorer emphasis: `--priority`, else the project's configured one,
/// else SECURE.
pub fn resolve_priority(raw: Option<&str>, config: &ToposConfig) -> Result<Priority, String> {
    let Some(raw) = raw else {
        if config.priority.is_none() && config.preferences.is_none() {
            return Ok(Priority::Secure);
        }
        return Ok(config.effective_priority());
    };
    Ok(match parse_priority_input(raw)? {
        PriorityInput::Single(priority) => priority,
        PriorityInput::Ranking(ranking) => priority_for_generator(ranking[0]),
    })
}

pub fn resolve_range(
    repo: &Path,
    pr: Option<u64>,
    base: Option<String>,
    head: Option<String>,
) -> Result<(String, String, Option<PullRequest>), String> {
    match (pr, base, head) {
        (None, None, None) => Ok(("HEAD".to_string(), ":worktree".to_string(), None)),
        (None, None, Some(head)) => Ok(("HEAD".to_string(), head, None)),
        (None, Some(base), head) => Ok((base, head.unwrap_or_else(|| "HEAD".to_string()), None)),
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => {
            Err("pass a pull request number or --base/--head, not both".to_string())
        }
        (Some(number), None, None) => {
            let review = pull_request(repo, number)?;
            ensure_commit(repo, &review.base_sha)?;
            ensure_commit(repo, &review.head_sha)?;
            Ok((
                review.base_sha,
                review.head_sha,
                Some(PullRequest {
                    number: review.number,
                    head_ref: review.head_ref,
                    base_ref: review.base_ref,
                }),
            ))
        }
    }
}

/// The commit `head` stands on: `:worktree` edits sit on top of HEAD, so
/// the change forks from there.
pub fn head_commit(head: &str) -> &str {
    if head == ":worktree" {
        "HEAD"
    } else {
        head
    }
}

/// What this run can do about coupling graphs, before anything is built.
pub enum CouplingPlan {
    /// No graphs this run, and why.
    Unavailable(CouplingStatus),
    /// Both graphs appear built at these commits. The lease is acquired and
    /// the state re-checked before this plan is trusted.
    Ready {
        stores: PrStores,
        base_sha: String,
        head_sha: String,
    },
    /// The graphs need building.
    NeedsBuild {
        pr: u64,
        base_sha: String,
        head_sha: String,
        state: StoreState,
        estimate_ms: Option<u64>,
    },
}

impl CouplingPlan {
    /// Whether [`prepare`] will build anything, the one slow case.
    pub fn needs_build(&self) -> bool {
        matches!(self, CouplingPlan::NeedsBuild { .. })
    }
}

/// A status with no graphs behind it.
pub fn unmeasured(reason: CouplingReason, note: impl Into<String>) -> CouplingStatus {
    CouplingStatus {
        measured: false,
        note: note.into(),
        reason,
        estimate_ms: None,
    }
}

/// Read what is already built for `base`/`head`. Fast: plain file reads and
/// two `git rev-parse` calls, never gitnexus.
pub fn plan_coupling(
    root: &Path,
    base: &str,
    head: &str,
    pr: Option<u64>,
    no_coupling: bool,
    gitnexus_available: bool,
) -> CouplingPlan {
    if no_coupling {
        return CouplingPlan::Unavailable(unmeasured(CouplingReason::Flag, "--no-coupling"));
    }
    let Some(pr) = pr else {
        return CouplingPlan::Unavailable(unmeasured(
            CouplingReason::NoPr,
            "pass a pull request number to measure COMPOSABLE",
        ));
    };
    let (Ok(base_sha), Ok(head_sha)) = (resolve_commit(root, base), resolve_commit(root, head))
    else {
        return CouplingPlan::Unavailable(unmeasured(
            CouplingReason::Error,
            format!("could not resolve {base}...{head}"),
        ));
    };
    let stores = match PrStores::locate(root, pr) {
        Ok(stores) => stores,
        Err(error) => return CouplingPlan::Unavailable(unmeasured(CouplingReason::Error, error)),
    };
    let state = pr_store_state(&stores, &base_sha, &head_sha);
    if state == StoreState::Ready {
        return CouplingPlan::Ready {
            stores,
            base_sha,
            head_sha,
        };
    }
    if !gitnexus_available {
        return CouplingPlan::Unavailable(unmeasured(
            CouplingReason::GitnexusMissing,
            "gitnexus not installed (npm install -g gitnexus)",
        ));
    }
    let history = stores.parent.parent().and_then(last_build_ms);
    CouplingPlan::NeedsBuild {
        pr,
        base_sha,
        head_sha,
        state,
        estimate_ms: build_estimate_ms(state, history),
    }
}

/// Carry out a settled plan: build the graphs a [`CouplingPlan::NeedsBuild`]
/// still wants, and say where the graphs came from or why there are none.
pub fn prepare(
    root: &Path,
    plan: CouplingPlan,
    on_side_done: &(dyn Fn() + Sync),
) -> (Option<PrStores>, CouplingStatus) {
    prepare_with_cancel(root, plan, on_side_done, None)
}

/// Lease-aware [`prepare`]. A cached store is leased and re-checked before it
/// is trusted; if its state changed while waiting, the lease is dropped and
/// the store rebuilt. The returned [`PrStores`] keeps the lease through recap
/// scoring and releases it only when dropped.
pub fn prepare_with_cancel(
    root: &Path,
    plan: CouplingPlan,
    on_side_done: &(dyn Fn() + Sync),
    cancel: Option<&AtomicBool>,
) -> (Option<PrStores>, CouplingStatus) {
    match plan {
        CouplingPlan::Unavailable(status) => (None, status),
        CouplingPlan::Ready {
            stores,
            base_sha,
            head_sha,
        } => {
            let Some(pr) = stores.pr() else {
                return (
                    None,
                    unmeasured(
                        CouplingReason::Error,
                        "PR store is not named topos-pr-<N>".to_string(),
                    ),
                );
            };
            let Some(common) = stores.common_dir() else {
                return (
                    None,
                    unmeasured(
                        CouplingReason::Error,
                        "PR store is not inside a Git common directory".to_string(),
                    ),
                );
            };
            match PrLease::acquire_with_cancel(&common, pr, cancel) {
                Ok(lease) => {
                    let leased = stores.with_lease(lease);
                    if let Err(error) = ensure_marker_for_build(&common, root, pr, &leased.parent) {
                        return (None, unmeasured(CouplingReason::Error, error));
                    }
                    if pr_store_state(&leased, &base_sha, &head_sha) == StoreState::Ready {
                        crate::cache::pr_used(root, pr, &leased.parent);
                        let status = CouplingStatus {
                            measured: true,
                            note: format!("reused from {}", leased.parent.display()),
                            reason: CouplingReason::Cached,
                            estimate_ms: None,
                        };
                        return (Some(leased), status);
                    }
                    drop(leased);
                }
                Err(error) => {
                    return (
                        None,
                        unmeasured(
                            CouplingReason::Error,
                            format!("could not lease PR {pr}: {error}"),
                        ),
                    );
                }
            }
            finish_build(root, pr, &base_sha, &head_sha, on_side_done, cancel)
        }
        CouplingPlan::NeedsBuild {
            pr,
            base_sha,
            head_sha,
            ..
        } => finish_build(root, pr, &base_sha, &head_sha, on_side_done, cancel),
    }
}

fn finish_build(
    root: &Path,
    pr: u64,
    base_sha: &str,
    head_sha: &str,
    on_side_done: &(dyn Fn() + Sync),
    cancel: Option<&AtomicBool>,
) -> (Option<PrStores>, CouplingStatus) {
    match prepare_pr_stores_with_cancel(root, pr, base_sha, head_sha, on_side_done, cancel) {
        Ok(stores) => {
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                return (
                    None,
                    unmeasured(
                        CouplingReason::Error,
                        "cancelled while preparing the PR cache".to_string(),
                    ),
                );
            }
            crate::cache::pr_built(root, pr, &stores.parent);
            let status = CouplingStatus {
                measured: true,
                note: format!("built from {}", stores.parent.display()),
                reason: CouplingReason::Built,
                estimate_ms: None,
            };
            (Some(stores), status)
        }
        Err(error) => (None, unmeasured(CouplingReason::Error, error)),
    }
}

/// Hears Pass A as it goes, so `run_recap` can draw a bar without
/// `build_recap` knowing about terminals. `()` hears nothing.
pub trait ScoreProgress {
    /// Scoring is about to start on `total` files.
    fn start(&mut self, total: usize);
    /// The file at `path` is about to be scored.
    fn scoring(&mut self, path: &str);
    /// The current file has been scored.
    fn scored(&mut self);
}

impl ScoreProgress for () {
    fn start(&mut self, _: usize) {}
    fn scoring(&mut self, _: &str) {}
    fn scored(&mut self) {}
}

#[allow(clippy::too_many_arguments)]
pub fn build_recap(
    repo: &Path,
    base: &str,
    head: &str,
    max_files: usize,
    stores: Option<&PrStores>,
    coupling: CouplingStatus,
    judging: &Judging,
    progress: &mut dyn ScoreProgress,
) -> Result<PrRecap, String> {
    let repo = git_root(repo)?;
    let worktree = head == ":worktree";
    let base_sha = merge_base(&repo, base, head_commit(head))?;
    let head_sha = if worktree {
        "worktree".to_string()
    } else {
        resolve_commit(&repo, head)?
    };
    let diff = if worktree {
        worktree_files(&repo, &base_sha)?
    } else {
        changed_files(&repo, &base_sha, &head_sha)?
    };

    let head_to_base: HashMap<String, String> = diff
        .entries
        .iter()
        .filter_map(|entry| Some((entry.path.clone(), entry.old_path.clone()?)))
        .collect();
    let mut changed_sources = BTreeSet::new();
    let mut skipped = Vec::new();
    let mut scoreable = Vec::new();
    for entry in diff.entries {
        match skip_reason(&entry) {
            Some(reason) => skipped.push(SkippedFile {
                path: entry.path,
                reason,
            }),
            None => {
                changed_sources.insert(entry.path.clone());
                scoreable.push(entry);
            }
        }
    }
    let capped = if max_files == 0 {
        0
    } else {
        scoreable.len().saturating_sub(max_files)
    };
    let mut capped_entries = Vec::new();
    if capped > 0 {
        let churn = churn(&repo, &base_sha, (!worktree).then_some(head_sha.as_str()))?;
        let (kept, dropped) = cap_by_churn(scoreable, max_files, |path| {
            churn
                .get(path)
                .copied()
                .or_else(|| {
                    worktree
                        .then(|| std::fs::read_to_string(repo.join(path)).ok())
                        .flatten()
                        .map(|source| source.lines().count())
                })
                .unwrap_or(0)
        });
        scoreable = kept;
        capped_entries = dropped;
        for entry in &capped_entries {
            skipped.push(SkippedFile {
                path: entry.path.clone(),
                reason: format!("over the {max_files}-file scoring cap"),
            });
        }
    }

    let base_graph = stores.and_then(|stores| load_graph(&stores.base));
    let head_graph = stores.and_then(|stores| load_graph(&stores.head));
    let measured = coupling.measured && base_graph.is_some() && head_graph.is_some();

    let scoring = Scoring {
        repo: &repo,
        base: &base_sha,
        head: &head_sha,
        base_graph: base_graph.as_ref().filter(|_| measured),
        head_graph: head_graph.as_ref().filter(|_| measured),
        priority: judging.priority,
        config: &judging.topos,
    };
    progress.start(scoreable.len());
    let mut scored = scoreable
        .iter()
        .map(|entry| {
            progress.scoring(&entry.path);
            let file = scoring.score_file(entry);
            progress.scored();
            file
        })
        .collect::<Result<Vec<_>, _>>()?;

    let changed = changed_list(&scoreable, &diff.deleted);
    let report = if measured {
        detect_splits(
            base_graph.as_ref().expect("measured implies a base graph"),
            head_graph.as_ref().expect("measured implies a head graph"),
            &changed,
        )
    } else {
        SplitReport::default()
    };
    let clusters = build_clusters(
        &mut scored,
        &report.clusters,
        measured,
        head_graph.as_ref().filter(|_| measured),
    );
    let moves = RangeMoves::build(
        &repo,
        &base_sha,
        &scored,
        &diff.deleted,
        &report.moved_between_existing,
    );

    let lattice = Omega::default();
    finish_statuses(&mut scored, &clusters, &lattice);

    let mut files: Vec<FileRecap> = scored.into_iter().map(|file| file.recap).collect();
    let (direction, described, described_path) = direction_for(&files, &base_sha, &head_sha);
    let mut coupling_graphs = base_graph
        .as_ref()
        .zip(head_graph.as_ref())
        .filter(|_| measured)
        .map(|(base, head)| gates::Coupling {
            base: FileGraph::build(base),
            head: FileGraph::build(head),
            head_to_base,
            changed: changed_sources,
            deleted: diff.deleted.iter().cloned().collect(),
            fan_in_files: Vec::new(),
        });
    if let Some(graphs) = &mut coupling_graphs {
        score_capped_for_fan_in(
            graphs,
            &files,
            &capped_entries,
            &clusters,
            &scoring,
            &judging.gate,
        )?;
    }
    let unscored = gates::Unscored {
        files: capped,
        cap_name: judging.cap_name,
    };
    let (_, mut findings) = gates::evaluate(
        &files,
        &clusters,
        unscored,
        &judging.gate,
        &moves,
        coupling_graphs.as_ref(),
    );
    let waivers = gates::waive(
        &mut findings,
        &judging.gate.waivers,
        &judging.today,
        judging.source.as_deref(),
        &repo,
    );
    let readiness = gates::readiness(&findings);
    for file in &mut files {
        file.severity = gates::worst_at(&findings, &file.path);
    }
    let (reason, reason_path) =
        reason_for(&findings, (described, described_path), unscored, max_files);
    let (hotspots, hotspots_total) = top_hotspots(&files, judging.gate.max_hotspots as usize);
    let project = project_rollup(&files);
    let added = added_rollup(&files);
    let scope = Scope {
        files_scored: files.len(),
        files_new: files.iter().filter(|file| file.is_new()).count(),
        lines_added: files.iter().map(|file| file.lines_added).sum(),
        lines_removed: files.iter().map(|file| file.lines_removed).sum(),
        files_skipped: skipped.len(),
        files_deleted: diff.deleted.len(),
        files_capped: capped,
        coupling,
    };
    let exit_code = readiness.exit_code(judging.gate.fail_on);
    Ok(PrRecap {
        schema: SCHEMA,
        base: base_sha,
        head: head_sha,
        priority: priority_name(judging.priority),
        review: None,
        gate: gates::summary(&judging.gate, judging.source.as_deref()),
        readiness,
        exit_code,
        check: if exit_code == 1 { "fail" } else { "pass" },
        reason,
        reason_path,
        findings,
        waivers,
        direction,
        incomplete: capped > 0,
        scope,
        project,
        added,
        clusters,
        files,
        skipped,
        deleted: diff.deleted,
        hotspots,
        hotspots_total,
        non_claim: "Structural direction is not proof that tests or behavior still pass, and PR recap is a structural debt ledger and pre-review coaching aid rather than a semantic correctness or AI-slop classifier.",
    })
}

/// What the verdict says and the file it names: the first counting
/// finding's text and path, else the direction's `described` text and its
/// file. A run that left files unscored says so, unless the lead finding is
/// that very incompleteness.
fn reason_for(
    findings: &[gates::Finding],
    (described, described_path): (String, Option<String>),
    unscored: gates::Unscored,
    max_files: usize,
) -> (String, Option<String>) {
    let lead = findings.iter().find(|finding| finding.counts());
    let path = match lead {
        Some(finding) => Some(finding.path.clone()).filter(|path| !path.is_empty()),
        None => described_path,
    };
    let mut reason = lead.map_or(described, |finding| finding.text.clone());
    let incomplete_leads = lead.is_some_and(|finding| finding.gate == GateId::Incomplete);
    if unscored.files > 0 && !incomplete_leads {
        let (files, cap_name) = (unscored.files, unscored.cap_name);
        reason.push_str(&format!(
            " Incomplete: {files} lower-churn file{} over the {max_files}-file cap went \
             unscored (raise {cap_name}).",
            if files == 1 { "" } else { "s" },
        ));
    }
    (reason, path)
}

fn score_capped_for_fan_in(
    coupling: &mut gates::Coupling,
    files: &[FileRecap],
    capped: &[DiffEntry],
    clusters: &[Cluster],
    scoring: &Scoring,
    cfg: &PrGateConfig,
) -> Result<(), String> {
    if cfg.severity(GateId::FanInGrowth) == Severity::Off {
        return Ok(());
    }
    let scored: BTreeSet<String> = files.iter().map(|file| file.path.clone()).collect();
    let mut added: BTreeSet<String> = files
        .iter()
        .filter(|file| file.is_new())
        .map(|file| file.path.clone())
        .collect();
    for entry in capped {
        if file_change(&entry.status) == FileChange::Added {
            added.insert(entry.path.clone());
        }
    }
    let want: BTreeSet<String> =
        gates::capped_fan_in_paths(coupling, &scored, &added, clusters, cfg)
            .into_iter()
            .collect();
    for entry in capped {
        if want.contains(&entry.path) {
            coupling.fan_in_files.push(scoring.score_file(entry)?.recap);
        }
    }
    Ok(())
}

pub fn load_graph(store: &Path) -> Option<ModuleDependencyGraph> {
    ModuleDependencyGraph::from_lbug_path(&store.join(".gitnexus").join("lbug"), "").ok()
}

fn parse_preset(raw: &str) -> Result<PrGatePreset, String> {
    match PrGatePreset::parse(raw) {
        Some(
            preset @ (PrGatePreset::Relaxed | PrGatePreset::Recommended | PrGatePreset::Strict),
        ) => Ok(preset),
        Some(PrGatePreset::Custom) | None => Err(format!(
            "invalid preset '{raw}' (expected relaxed, recommended, or strict)"
        )),
    }
}

#[tool_router(router = pr_recap_router, vis = "pub(crate)")]
impl ToposServer {
    /// Structural PR or commit-range recap: gate readiness, blocking findings, files needing attention. Summary by default; `detail: "full"` for `topos.pr_recap.v3`.
    #[tool(
        name = "topos_pr_recap",
        annotations(
            title = "Topos PR Recap",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn topos_pr_recap(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(params): Parameters<PrRecapInput>,
    ) -> Result<CallToolResponse, McpError> {
        let progress = Arc::new(Progress::default());
        let worker = Arc::clone(&progress);
        self.executor
            .run_task_described(
                &context,
                Duration::from_secs(300),
                "Building PR structural recap",
                mcp::timeout_note(&progress),
                move |cancel_flag, _reporter| Ok(mcp::respond(params, Some(cancel_flag), &worker)),
            )
            .await
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::gh::git;
    use super::*;
    use crate::schemas::PrRecapDetail;

    pub(super) fn no_coupling() -> CouplingStatus {
        unmeasured(
            CouplingReason::NoPr,
            "pass a pull request number to measure COMPOSABLE",
        )
    }

    pub(super) fn judging(priority: Priority) -> Judging {
        Judging {
            priority,
            topos: ToposConfig::default(),
            gate: PrGateConfig::default(),
            source: None,
            today: "2026-09-22".to_string(),
            cap_name: gates::CLI_CAP_NAME,
        }
    }

    pub(super) fn recap(repo: &Path, base: &str, head: &str, max_files: usize) -> PrRecap {
        build_recap(
            repo,
            base,
            head,
            max_files,
            None,
            no_coupling(),
            &judging(Priority::Secure),
            &mut (),
        )
        .unwrap()
    }

    pub(super) fn write_repo(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().to_path_buf();
        git(&repo, &["init", "-q"]).expect("init");
        git(&repo, &["config", "user.email", "recap@example.com"]).unwrap();
        git(&repo, &["config", "user.name", "Recap"]).unwrap();
        write_files(&repo, files);
        git(&repo, &["add", "."]).unwrap();
        git(&repo, &["commit", "-qm", "base"]).unwrap();
        (dir, repo)
    }

    pub(super) fn write_files(repo: &Path, files: &[(&str, &str)]) {
        for (path, body) in files {
            let full = repo.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, body).unwrap();
        }
    }

    pub(super) fn commit_all(repo: &Path, message: &str) {
        git(repo, &["add", "."]).unwrap();
        git(repo, &["commit", "-qm", message]).unwrap();
    }

    pub(super) fn branchy(name: &str) -> String {
        let mut body = format!("def {name}(x):\n");
        for i in 0..12 {
            body.push_str(&format!("    if x == {i}:\n        return {i}\n"));
        }
        body.push_str("    return -1\n");
        body
    }

    #[test]
    fn mcp_pr_recap_scores_worktree_and_validates_inputs() {
        let (_temp_dir, repo) = write_repo(&[("a.py", &branchy("solve"))]);
        write_files(&repo, &[("a.py", "def solve(x):\n    return x\n")]);
        let repo_path = repo.to_str().expect("utf-8 repo path").to_string();
        let response = mcp::respond_untimed(
            PrRecapInput {
                directory: Some(repo_path.clone()),
                pr: None,
                base: Some("HEAD".to_string()),
                head: Some(":worktree".to_string()),
                max_files: 0,
                detail: PrRecapDetail::Summary,
                paths: Vec::new(),
                no_coupling: true,
                priority: Some("simple".to_string()),
                strict: false,
                preset: Some("recommended".to_string()),
            },
            None,
        );
        let structured = response.structured_content.expect("structured_content");
        assert_eq!(structured["schema"], summary::SUMMARY_SCHEMA);
        assert_eq!(structured["check"], "pass");
        assert_eq!(structured["scope"]["files_scored"], 1);

        let invalid_response = mcp::respond_untimed(
            PrRecapInput {
                directory: Some(repo_path),
                pr: Some(1),
                base: Some("HEAD".to_string()),
                head: None,
                max_files: 0,
                detail: PrRecapDetail::Summary,
                paths: Vec::new(),
                no_coupling: true,
                priority: None,
                strict: false,
                preset: None,
            },
            None,
        );
        let error = invalid_response.structured_content.expect("error payload");
        assert!(error["error"].as_str().unwrap_or("").contains("not both"));
    }
}
