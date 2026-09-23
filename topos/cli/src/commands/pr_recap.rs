//! `topos pr-recap` — the data builder for schema `topos.pr_recap.v3`.
//!
//! Scores added and modified source files at `--base` and `--head`, groups
//! the ones that look like a split into clusters, applies the configured
//! `[pr_recap]` gates, and hands a single [`model::PrRecap`] document to
//! whichever renderer the caller asked for. Every verdict on a card is
//! decided here, from the lattice, the UAST ledger, the two coupling graphs
//! and the gate policy — a formatter can never invent one.

mod clusters;
mod coupling;
#[cfg(test)]
mod fixtures;
mod gates;
mod git;
mod github;
mod hotspots;
mod model;
mod render;
mod score;
mod verdict;
mod view;

use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use topos_engine::config::{
    find_config_file, load_topos_config, FailOn, GateId, PrGateConfig, PrGatePreset, ToposConfig,
};
use topos_engine::core::omega::Omega;
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;
use topos_engine::graphs::mdg::split::detect_splits;

use self::clusters::{build_clusters, changed_list};
use self::coupling::{plan_coupling, prepare, settle};
use self::git::{cap_by_churn, changed_files, churn, skip_reason, worktree_files};
use self::hotspots::top_hotspots;
use self::model::*;
use self::score::Scoring;
use self::verdict::{added_rollup, direction_for, finish_statuses, project_rollup};
use super::config::{parse_priority_input, priority_for_generator, priority_name, PriorityInput};
use crate::commands::depgraph::{gitnexus_available, PrStores};
use crate::commands::gh::{ensure_commit, git_root, merge_base, pull_request, resolve_commit};
use crate::commands::interaction::{self, PromptEnv, Streams, TermAsker};
use crate::commands::render::{RenderOptions, Working};

const DEFAULT_FILE_CAP: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RecapFormat {
    /// Full terminal review card.
    Card,
    /// The review card; kept so existing scripts keep working.
    #[value(hide = true)]
    Compact,
    /// Markdown for a sticky pull request comment.
    Github,
}

/// A built-in `[pr_recap]` preset, for `--preset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GatePreset {
    Relaxed,
    Recommended,
    Strict,
}

impl GatePreset {
    fn preset(self) -> PrGatePreset {
        match self {
            GatePreset::Relaxed => PrGatePreset::Relaxed,
            GatePreset::Recommended => PrGatePreset::Recommended,
            GatePreset::Strict => PrGatePreset::Strict,
        }
    }
}

/// The long help for `topos pr-recap`, printed by `--help` under the
/// flag list. `-h` stays short: it shows only the flag one-liners.
pub const LONG_HELP: &str = r#"What it does:
  Scores the files your change touched at the base commit and at the head commit,
  then reports the structural difference between the two. It is deterministic and
  reads only your code: no LLM, no network call, no model judgment. The medal is
  Topos's lattice verdict over the pillars below. Structure moving in the right
  direction is not proof that behavior is unchanged or that the tests still pass;
  read it as a review aid, not as a green check.

Pillars:
  SIMPLE      per-function complexity and the control-flow gates.
  COMPOSABLE  module coupling from the GitNexus dependency graph: fan-out and
              instability.
  SECURE      dangerous calls and taint flows.
  NAVIGABLE   nesting divergence.
  COMPOSABLE is not measured with no graph, and no pillar is measured on a file
  that did not parse.

Medals:
  PLATINUM  all four pillars pass.
  GOLD      three pass.
  SILVER    two pass.
  BRONZE    one passes.
  SLOP      none pass.
  `BRONZE → SILVER` means the medal itself changed over this range.

Findings:
  The card lists what blocks (X) and what needs attention (!), most important
  first, at most max_hotspots of them; findings at the same function merge into
  one item. A dim line counts the rest: `N more`, smaller dips, and notes (info
  findings). Score dips under one point are not shown.

Changed files (--verbose):
  One row per file whose medal, pillars or scores moved. The mark is the file's
  worst finding: X blocks, ! needs attention, none is info or nothing.
  CHANGE says what happened, pillar by pillar:
    X SIMPLE lost       a pillar the base passed now fails. Gaining another
                        pillar does not cancel it: `X SIMPLE lost · ✓ NAVIGABLE
                        gained` is a trade, and still a loss.
    ✓ NAVIGABLE gained  a pillar the base failed now passes.
    ↓ SIMPLE 69 → 64    a score moved at least one point without crossing a gate.
    new                 an added file; `X SECURE fails` when it arrives failing.
                        New code is gated like changed code; a split child is
                        judged through its split instead, since moved code
                        brings its findings with it.
    cosmetic            scores moved while the syntax tree barely changed (an
                        agent-slop signal: the shape of the code is the same, the
                        numbers are not).

Splits (--verbose):
  SPLIT       a file whose code moved out into new files. It passes (✓) when the
              worst function got simpler and total decisions grew by no more than
              10%, warns (!) when decisions grew by more than 10% or a child
              landed SLOP, and fails (X) when a moved function came out more
              complex than it went in, or the split as a whole has more SECURE
              findings than the parent had. What each costs the verdict is the
              split_* gates' call; a pillar the parent lost is its own finding.
  Children (├─) are the new files a split produced. `N in` counts the symbols or
  functions that moved into that child; `shared ×N` means N files besides the
  parent import it.

Splits table columns:
  WORST FN   the highest single-function complexity, before and after.
  DECISIONS  total decision points (cyclomatic) in the parent before, then in the
             parent and all of its children after. A `+P%` marks growth over 10%.

Project table:
  One row per pillar over the existing files the change touched: whether it
  passes at head, the mean score before and after, how many files fail it out of
  how many were measured, and a rail showing where the head score sits. Both
  columns cover the same files; added files are rolled up on their own.

Readiness / exit codes:
  Each gate in `[pr_recap]` (`topos config`) turns what happened into a finding
  with a severity: off, info, warn or block. The worst finding decides.
  X BLOCKED          a block finding, such as a lost pillar.   exit 1
  ! NEEDS ATTENTION  a warn finding, such as a large drop.     exit 0, or 1
                     under fail_on = "warn"
  ✓ READY            info findings only, or none.              exit 0
  A score drop counts only once it is large enough, in a file changed enough
  ([pr_recap.score_drop]); a smaller one is info. The policy is the flags, else
  the nearest .topos.toml, else the recommended preset. --preset picks a
  built-in preset and ignores the file's [pr_recap]; --strict fails on warn
  too. An error (a bad range, git or gh failing) exits 2. The direction
  (IMPROVEMENT, SCORE DOWN, LATERAL, ...) says which way the structure moved and
  never changes the exit code.

Range:
  The base side is the merge-base of the base and the head, so commits that
  landed on the base branch after the fork are not charged to the change. With no
  arguments, uncommitted edits (tracked and untracked) are reviewed against HEAD.
  Past --max-files, the files with the most changed lines are scored first and
  the recap is marked incomplete; the rest are listed as skipped.

Priority:
  Files are classified with the project's configured priority (`topos config`),
  or SECURE when none is set. `--priority` overrides it for one run.

Coupling:
  Given a PR number and an installed GitNexus, both commits are indexed under
  `.git/topos-pr-<N>/` so that COMPOSABLE and split tracing use real import and
  call edges. A first build takes about 20 s per commit, the two in parallel:
  about 25 s in all. Later runs on the same commits reuse the graphs without
  GitNexus, and a new head rebuilds only the head graph. `--no-coupling` skips
  that work and reports COMPOSABLE as not measured. With `--base/--head` there
  is no PR store, so splits are detected from import lines and the
  moved-function ledger instead.

Prompts:
  On a terminal, pr-recap asks before a slow step: building coupling graphs
  that are not built yet. Graphs already built are reused without asking.
    --yes, -y      answer yes to every question.
    --no-input     never ask; take each question's default.
    --no-coupling  skip the graphs without asking, even with --yes.
  Without a terminal on stdin, stdout and stderr, or with CI or
  GH_PROMPT_DISABLED set, nothing is asked and each default is taken; the
  default builds the graphs, so a required check never depends on a terminal
  or a cache. Esc at the prompt skips the build; Ctrl-C stops with exit 2.

Outputs:
  --format card     the review card, the default. The same layout on a terminal
                    and in a pipe; piped output drops the color.
  --format github   markdown for a sticky PR comment, with a hidden marker so a
                    later run replaces it instead of adding another comment.
  --json            schema topos.pr_recap.v3: every number and finding behind
                    the card.
  --verbose         adds the changed-files table, every split child and each
                    moved function.
  --info            appends the recommended change for each finding.

Examples:
  topos pr-recap
  topos pr-recap 306
  topos pr-recap --base main --head HEAD
  topos pr-recap --head :worktree
  topos pr-recap 306 --format github > comment.md
"#;

#[derive(Args)]
pub struct PrRecapArgs {
    /// Pull request number to review against the branch it merges into.
    #[arg(value_name = "PR")]
    pub pr: Option<u64>,
    /// Commit the change starts from (the pull request base).
    #[arg(long)]
    pub base: Option<String>,
    /// Commit the change ends at; `:worktree` includes uncommitted and
    /// untracked edits.
    #[arg(long)]
    pub head: Option<String>,
    /// Repository to read. Defaults to the current directory.
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Print the machine-readable document instead of the review card.
    #[arg(long)]
    pub json: bool,
    /// Score at most this many added or modified files, most changed first.
    #[arg(long, default_value_t = DEFAULT_FILE_CAP)]
    pub max_files: usize,
    /// Add the changed-files table, every split child and the per-function
    /// ledger.
    #[arg(long)]
    pub verbose: bool,
    /// Append the recommended change for each finding after the card.
    #[arg(long)]
    pub info: bool,
    /// The review card; kept so existing scripts keep working.
    #[arg(long, hide = true)]
    pub compact: bool,
    /// Which card to print: card or github.
    #[arg(long, value_enum)]
    pub format: Option<RecapFormat>,
    /// Skip coupling preparation; COMPOSABLE is reported as not measured.
    #[arg(long)]
    pub no_coupling: bool,
    /// Answer yes to every question: build coupling graphs, run optional checks.
    #[arg(long, short = 'y', conflicts_with = "no_input")]
    pub yes: bool,
    /// Never ask; take each question's default.
    #[arg(long)]
    pub no_input: bool,
    /// Pillar to prioritize (simple, composable, secure, navigable), or a
    /// full comma-separated ranking, most important first.
    #[arg(long, value_name = "PILLAR|SIMPLE,COMPOSABLE,SECURE,NAVIGABLE")]
    pub priority: Option<String>,
    /// Fail the check on NEEDS ATTENTION too (fail_on = "warn").
    #[arg(long)]
    pub strict: bool,
    /// Gate with a built-in preset, ignoring the project's [pr_recap].
    #[arg(long, value_enum)]
    pub preset: Option<GatePreset>,
}

/// The gate policy for one run: `--preset` replaces the project's
/// `[pr_recap]` with a built-in preset, and `--strict` then fails on warn
/// as well. With neither, the project's table (itself recommended when
/// absent) applies as is.
fn resolve_gate_policy(
    strict: bool,
    preset: Option<GatePreset>,
    file: &PrGateConfig,
) -> PrGateConfig {
    let mut policy = match preset {
        Some(preset) => PrGateConfig::for_preset(preset.preset()),
        None => file.clone(),
    };
    if strict {
        policy.fail_on = FailOn::Warn;
    }
    policy
}

/// Everything a recap is judged by: the pillar emphasis, the project
/// config (for its allowlist), and the resolved gate policy with the file
/// it came from.
struct Judging {
    priority: Priority,
    topos: ToposConfig,
    gate: PrGateConfig,
    source: Option<PathBuf>,
}

/// Which card to print. The card is the same on a terminal and in a
/// pipe, so `--compact` and `--format compact` are aliases for it. `--json`
/// is decided by the caller and wins over this.
fn resolve_format(compact: bool, format: Option<RecapFormat>) -> RecapFormat {
    match format {
        Some(RecapFormat::Github) if !compact => RecapFormat::Github,
        _ => RecapFormat::Card,
    }
}

// --- Range resolution --------------------------------------------------

fn resolve_range(
    repo: &Path,
    pr: Option<u64>,
    base: Option<String>,
    head: Option<String>,
) -> Result<(String, String, Option<PullRequest>), String> {
    match (pr, base, head) {
        // HEAD against HEAD is always empty. With nothing to go on, review
        // what `git diff` would show: the uncommitted edits.
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
fn head_commit(head: &str) -> &str {
    if head == ":worktree" {
        "HEAD"
    } else {
        head
    }
}

// --- Entry point -------------------------------------------------------

/// Exit 0 on a pass, 1 when the readiness fails the check under
/// `fail_on`, 2 on an error.
///
/// `main` exits 1 for every command error, which would make a broken run
/// indistinguishable from a failed check in CI, so errors stop here.
pub fn run(args: PrRecapArgs) -> Result<(), String> {
    match run_recap(args) {
        Ok(0) => Ok(()),
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("Error: {message}");
            std::process::exit(2);
        }
    }
}

/// The scorer emphasis: `--priority`, else the project's configured one,
/// else SECURE, which is what this command always used before it read the
/// project config.
fn resolve_priority(raw: Option<&str>, config: &ToposConfig) -> Result<Priority, String> {
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

/// Print the recap and hand back its exit code.
fn run_recap(args: PrRecapArgs) -> Result<i32, String> {
    let repo = args
        .repo
        .clone()
        .unwrap_or(std::env::current_dir().map_err(|e| format!("current directory: {e}"))?);
    let root = git_root(&repo)?;
    let topos = load_topos_config(&root);
    // A mistyped key is dropped, never silent.
    for warning in &topos.pr_recap.warnings {
        eprintln!("warning: {warning}");
    }
    let judging = Judging {
        priority: resolve_priority(args.priority.as_deref(), &topos)?,
        gate: resolve_gate_policy(args.strict, args.preset, &topos.pr_recap),
        source: args
            .preset
            .is_none()
            .then(|| find_config_file(&root))
            .flatten(),
        topos,
    };
    let (base, head, review) = resolve_range(&root, args.pr, args.base.clone(), args.head.clone())?;
    let format = resolve_format(args.compact, args.format);
    // The stores must be built at the commit the sources are read from.
    let base = merge_base(&root, &base, head_commit(&head))?;

    // Any question is asked here, before the spinner takes stderr.
    let interaction = interaction::resolve(
        args.yes,
        args.no_input,
        &PromptEnv::from_env(),
        &Streams::detect(),
    );
    let plan = plan_coupling(&root, &base, &head, &args, gitnexus_available());
    let plan = settle(plan, interaction, &mut TermAsker)?;

    // The spinner covers store generation too: that is the slow part.
    let working = (!args.json).then(Working::start);
    let (stores, coupling) = prepare(&root, plan);
    let recap = build_recap(
        &root,
        &base,
        &head,
        args.max_files,
        stores.as_ref(),
        coupling,
        &judging,
    );
    if let Some(working) = working {
        working.clear();
    }
    let mut recap = recap?;
    recap.review = review;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&recap).map_err(|e| format!("serializing recap: {e}"))?
        );
    } else {
        match format {
            RecapFormat::Card | RecapFormat::Compact => {
                let detail = render::Detail {
                    verbose: args.verbose,
                    info: args.info,
                };
                for line in render::render_card(&recap, detail, RenderOptions::stdout()) {
                    println!("{line}");
                }
            }
            RecapFormat::Github => println!("{}", github::render_github(&recap)),
        }
    }
    Ok(recap.exit_code)
}

// --- Document assembly -------------------------------------------------

fn build_recap(
    repo: &Path,
    base: &str,
    head: &str,
    max_files: usize,
    stores: Option<&PrStores>,
    coupling: CouplingStatus,
    judging: &Judging,
) -> Result<PrRecap, String> {
    let repo = git_root(repo)?;
    let worktree = head == ":worktree";
    // The file list, the before-sources and the base store all come from
    // this one commit. Idempotent when the caller already passed it.
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

    let mut skipped = Vec::new();
    let mut scoreable = Vec::new();
    for entry in diff.entries {
        match skip_reason(&entry) {
            Some(reason) => skipped.push(SkippedFile {
                path: entry.path,
                reason,
            }),
            None => scoreable.push(entry),
        }
    }
    let capped = scoreable.len().saturating_sub(max_files);
    if capped > 0 {
        // Git lists paths alphabetically; dropping that tail would skip
        // whatever sorts last, however much of the change it carries.
        let churn = churn(&repo, &base_sha, (!worktree).then_some(head_sha.as_str()))?;
        let (kept, dropped) = cap_by_churn(scoreable, max_files, |path| {
            churn
                .get(path)
                .copied()
                // Untracked files are not in `git diff`: all their lines are new.
                .or_else(|| {
                    worktree
                        .then(|| std::fs::read_to_string(repo.join(path)).ok())
                        .flatten()
                        .map(|source| source.lines().count())
                })
                .unwrap_or(0)
        });
        scoreable = kept;
        for entry in dropped {
            skipped.push(SkippedFile {
                path: entry.path,
                reason: format!("over the {max_files}-file scoring cap"),
            });
        }
    }

    // One load per store, not one per file: the graph is the whole repo.
    let base_graph = stores.and_then(|stores| load_graph(&stores.base));
    let head_graph = stores.and_then(|stores| load_graph(&stores.head));
    let measured = coupling.measured && base_graph.is_some() && head_graph.is_some();

    // Pass A — score every file on its own.
    let scoring = Scoring {
        repo: &repo,
        base: &base_sha,
        head: &head_sha,
        base_graph: base_graph.as_ref().filter(|_| measured),
        head_graph: head_graph.as_ref().filter(|_| measured),
        priority: judging.priority,
        config: &judging.topos,
    };
    let mut scored = scoreable
        .iter()
        .map(|entry| scoring.score_file(entry))
        .collect::<Result<Vec<_>, _>>()?;

    // Pass B — group the split parents with their children.
    let changed = changed_list(&scoreable, &diff.deleted);
    let report = (measured)
        .then(|| {
            detect_splits(
                base_graph.as_ref().expect("measured implies a base graph"),
                head_graph.as_ref().expect("measured implies a head graph"),
                &changed,
            )
        })
        .map(|report| report.clusters)
        .unwrap_or_default();
    let clusters = build_clusters(
        &mut scored,
        &report,
        measured,
        head_graph.as_ref().filter(|_| measured),
    );

    // Pass C — per-file verdicts, now that cluster fan-out is known.
    let lattice = Omega::default();
    finish_statuses(&mut scored, &clusters, &lattice);

    let mut files: Vec<FileRecap> = scored.into_iter().map(|file| file.recap).collect();
    // Pass D — the configured gates decide readiness.
    let (direction, described) = direction_for(&files, &base_sha, &head_sha);
    let (readiness, findings) = gates::evaluate(&files, &clusters, capped, &judging.gate);
    for file in &mut files {
        file.severity = gates::worst_at(&findings, &file.path);
    }
    let mut reason = findings
        .first()
        .map_or(described, |finding| finding.text.clone());
    let incomplete_leads = findings
        .first()
        .is_some_and(|finding| finding.gate == GateId::Incomplete);
    if capped > 0 && !incomplete_leads {
        reason.push_str(&format!(
            " Incomplete: {capped} lower-churn file{} over the {max_files}-file cap went \
             unscored (raise --max-files).",
            if capped == 1 { "" } else { "s" }
        ));
    }
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
        findings,
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
        non_claim: "Structural direction is not proof that tests or behavior still pass.",
    })
}

fn load_graph(store: &Path) -> Option<ModuleDependencyGraph> {
    ModuleDependencyGraph::from_lbug_path(&store.join(".gitnexus").join("lbug"), "").ok()
}

#[cfg(test)]
mod tests {
    use topos_engine::config::Severity;

    use super::*;
    use crate::commands::gh::git;

    pub(super) fn no_coupling() -> CouplingStatus {
        coupling::unmeasured(
            CouplingReason::NoPr,
            "pass a pull request number to measure COMPOSABLE",
        )
    }

    /// SECURE priority, no project config, the recommended gates.
    fn judging(priority: Priority) -> Judging {
        Judging {
            priority,
            topos: ToposConfig::default(),
            gate: PrGateConfig::default(),
            source: None,
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

    #[test]
    fn empty_diff_is_a_lateral_move() {
        let (_keep, repo) = write_repo(&[("src/a.py", "def ready():\n    return 1\n")]);
        let recap = recap(&repo, "HEAD", "HEAD", 40);
        assert_eq!(recap.direction, Headline::LateralMove);
        assert_eq!(recap.readiness, gates::Readiness::Ready);
        assert!(recap.reason.contains("no commits"), "{}", recap.reason);
        assert!(recap.files.is_empty());
        assert_eq!(recap.check, "pass");
        assert!(recap.project.is_none());
    }

    #[test]
    fn added_source_is_scored_and_markdown_is_skipped() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        write_files(
            &repo,
            &[
                ("src/new.py", "def ready():\n    return 1\n"),
                ("notes.md", "not code\n"),
            ],
        );
        commit_all(&repo, "add");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        assert_eq!(recap.files.len(), 1);
        assert_eq!(recap.files[0].path, "src/new.py");
        assert_eq!(recap.files[0].change, FileChange::Added);
        assert!(recap.files[0].medal_before.is_none());
        assert!(recap.files[0].medal_after.is_some());
        assert_eq!(recap.scope.files_new, 1);
        assert!(recap.skipped.iter().any(|s| s.path == "notes.md"));
        assert!(!recap.scope.coupling.measured);
    }

    #[test]
    fn deleted_file_is_listed_not_scored() {
        let (_keep, repo) = write_repo(&[("src/gone.py", "def ready():\n    return 1\n")]);
        std::fs::remove_file(repo.join("src/gone.py")).unwrap();
        commit_all(&repo, "delete");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        assert!(recap.files.is_empty());
        assert_eq!(recap.deleted, vec!["src/gone.py".to_string()]);
    }

    #[test]
    fn a_dangerous_call_introduced_against_base_is_a_regression() {
        let (_keep, repo) = write_repo(&[("src/run.py", "def ready():\n    return 1\n")]);
        write_files(
            &repo,
            &[(
                "src/run.py",
                "import os\n\ndef ready(cmd):\n    os.system(cmd)\n",
            )],
        );
        commit_all(&repo, "shell");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        assert_eq!(recap.direction, Headline::Regression);
        assert_eq!(recap.readiness, gates::Readiness::Blocked);
        assert_eq!((recap.check, recap.exit_code), ("fail", 1));
        assert!(recap
            .hotspots
            .iter()
            .any(|h| h.metric == "cpg.dangerous_calls"));
        assert!(recap
            .hotspots
            .iter()
            .any(|h| h.detail.contains("os.system") || h.detail.contains("dangerous call")));
        assert_eq!(recap.files[0].pillars["secure"].after_passed, Some(false));
        assert_eq!(recap.files[0].status, Headline::Regression);
        assert_eq!(recap.files[0].severity, Some(Severity::Block));
        let lost = &recap.findings[0];
        assert_eq!(lost.gate, GateId::PillarLost);
        assert_eq!(lost.metric.as_deref(), Some("cpg.dangerous_calls"));
        assert_eq!(lost.line, Some(4), "points at the new call");
        assert!(
            recap.reason.starts_with("src/run.py lost SECURE"),
            "{}",
            recap.reason
        );
        assert_eq!(
            recap.files[0].pillars["secure"].lost_gate.as_deref(),
            Some("cpg.dangerous_calls")
        );
        let project = recap.project.expect("a scored file rolls up");
        assert!(project.regression);
    }

    #[test]
    fn cap_skips_the_overflow_instead_of_hiding_it() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        for i in 0..3 {
            write_files(&repo, &[(&format!("src/f{i}.py"), "x = 1\n")]);
        }
        commit_all(&repo, "three");
        let recap = recap(&repo, "HEAD~1", "HEAD", 1);
        assert_eq!(recap.files.len(), 1);
        assert_eq!(recap.scope.files_capped, 2);
        assert_eq!(recap.skipped.len(), 2);
        assert!(recap.incomplete);
    }

    #[test]
    fn renamed_file_is_scored_against_its_old_path() {
        let (_keep, repo) = write_repo(&[(
            "src/a.py",
            "def ready():\n    if True:\n        return 1\n    return 0\n",
        )]);
        git(&repo, &["mv", "src/a.py", "src/b.py"]).unwrap();
        commit_all(&repo, "rename");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        assert_eq!(recap.files.len(), 1);
        assert_eq!(recap.files[0].path, "src/b.py");
        assert_eq!(recap.files[0].change, FileChange::Renamed);
        assert!(recap.files[0].medal_before.is_some());
    }

    /// Without `-z`, git quotes a non-ASCII path and the quoted name reads
    /// no file at either revision.
    #[test]
    fn a_path_git_would_quote_is_still_scored() {
        let (_keep, repo) = write_repo(&[("src/naïve.py", "def ready():\n    return 1\n")]);
        write_files(
            &repo,
            &[
                ("src/naïve.py", "def ready():\n    return 2\n"),
                ("src/with space.py", "def fresh():\n    return 3\n"),
            ],
        );
        commit_all(&repo, "edit");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        let paths: Vec<&str> = recap.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["src/naïve.py", "src/with space.py"]);
        assert!(recap.files[0].medal_before.is_some());
    }

    #[test]
    fn refuses_a_revision_that_looks_like_an_option() {
        let (_keep, repo) = write_repo(&[("src/a.py", "x = 1\n")]);
        let err = build_recap(
            &repo,
            "--output=/tmp/x",
            "HEAD",
            40,
            None,
            no_coupling(),
            &judging(Priority::Secure),
        )
        .unwrap_err();
        assert!(err.contains("refusing"));
    }

    #[test]
    fn no_pr_means_coupling_not_measured() {
        let (_keep, repo) = write_repo(&[("src/a.py", "def ready():\n    return 1\n")]);
        write_files(&repo, &[("src/a.py", "def ready():\n    return 2\n")]);
        commit_all(&repo, "edit");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        assert!(!recap.scope.coupling.measured);
        assert!(recap.scope.coupling.note.contains("pull request"));
    }

    #[test]
    fn compact_is_an_alias_for_the_card() {
        assert_eq!(resolve_format(false, None), RecapFormat::Card);
        assert_eq!(resolve_format(true, None), RecapFormat::Card);
        assert_eq!(
            resolve_format(false, Some(RecapFormat::Compact)),
            RecapFormat::Card
        );
        assert_eq!(
            resolve_format(false, Some(RecapFormat::Github)),
            RecapFormat::Github
        );
    }

    #[test]
    fn tsx_files_are_scored() {
        let base = "export const Widget = ({ a }: { a: number }) => { if (a > 1) { return <b/> } return <i/> }\n";
        let head = "export const Widget = ({ a }: { a: number }) => { if (a > 1) { return <b/> } if (a > 2) { return <u/> } return <i/> }\n";
        let (_keep, repo) = write_repo(&[("Widget.tsx", base)]);
        write_files(&repo, &[("Widget.tsx", head)]);
        commit_all(&repo, "branch");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        let file = &recap.files[0];
        assert_eq!(file.path, "Widget.tsx");
        assert!(file.medal_before.is_some(), "base .tsx must parse");
        assert!(file.medal_after.is_some(), "head .tsx must parse");
        assert!(file.pillars["simple"].measured);
        assert!(file.structural_distance.is_some());
    }

    fn head_sha(repo: &Path) -> String {
        git(repo, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string()
    }

    /// Twelve flat branches: over the SIMPLE function gate, nothing nested.
    pub(super) fn branchy(name: &str) -> String {
        let mut body = format!("def {name}(x):\n");
        for i in 0..12 {
            body.push_str(&format!("    if x == {i}:\n        return {i}\n"));
        }
        body.push_str("    return -1\n");
        body
    }

    const SHELL: &str = "import os\n\ndef run(cmd):\n    os.system(cmd)\n";

    /// The base branch moved on after the fork. Its later edits must not be
    /// read as the before-side: here the base tip added a dangerous call
    /// the PR never had, which would read as the PR clearing SECURE.
    #[test]
    fn the_before_side_is_the_merge_base_not_the_base_tip() {
        let (_keep, repo) = write_repo(&[("src/a.py", "def ready():\n    return 1\n")]);
        let fork = head_sha(&repo);
        git(&repo, &["branch", "feature"]).unwrap();
        write_files(
            &repo,
            &[(
                "src/a.py",
                "import os\n\ndef ready(cmd):\n    os.system(cmd)\n",
            )],
        );
        commit_all(&repo, "base moves on");
        let base_tip = head_sha(&repo);
        git(&repo, &["checkout", "-q", "feature"]).unwrap();
        write_files(&repo, &[("src/a.py", "def ready():\n    return 2\n")]);
        commit_all(&repo, "feature edit");

        let recap = recap(&repo, &base_tip, "HEAD", 40);
        assert_eq!(recap.base, fork, "the recap compares from the fork point");
        let file = &recap.files[0];
        assert_eq!(file.pillars["secure"].before_passed, Some(true));
        assert!(!file.pillars["secure"].cleared());
        assert_ne!(recap.direction, Headline::Improvement);
    }

    #[test]
    fn a_new_file_failing_secure_fails_the_check_beside_existing_edits() {
        let (_keep, repo) = write_repo(&[("src/a.py", "def ready():\n    return 1\n")]);
        write_files(
            &repo,
            &[
                ("src/a.py", "def ready():\n    return 2\n"),
                ("src/run.py", SHELL),
            ],
        );
        commit_all(&repo, "shell in a new file");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);

        let new = recap.files.iter().find(|f| f.path == "src/run.py").unwrap();
        assert_eq!(new.status, Headline::Regression);
        assert_eq!(recap.direction, Headline::Regression);
        assert_eq!(recap.findings[0].gate, GateId::NewFileInsecure);
        assert_eq!(recap.check, "fail");
        assert!(
            recap.reason.contains("src/run.py is new and fails SECURE"),
            "{}",
            recap.reason
        );
        assert!(new
            .hotspots
            .iter()
            .any(|h| h.metric == "cpg.dangerous_calls"));
    }

    /// The stated intent survives: a clean new file cannot turn an existing
    /// file's lateral move into an improvement.
    #[test]
    fn a_passing_new_file_does_not_lift_a_lateral_move() {
        let (_keep, repo) = write_repo(&[("src/a.py", "def ready():\n    return 1\n")]);
        write_files(
            &repo,
            &[
                ("src/a.py", "def ready():\n    return 2\n"),
                ("src/clean.py", "def fine():\n    return 3\n"),
            ],
        );
        commit_all(&repo, "clean new file");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        let new = recap
            .files
            .iter()
            .find(|f| f.path == "src/clean.py")
            .unwrap();
        assert_eq!(new.status, Headline::Improvement);
        assert_eq!(recap.direction, Headline::LateralMove);
    }

    #[test]
    fn an_all_new_change_has_no_before_after_rollup() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        write_files(
            &repo,
            &[
                ("src/a.py", "def a():\n    return 1\n"),
                ("src/b.py", "def b():\n    return 2\n"),
            ],
        );
        commit_all(&repo, "two new files");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        assert!(
            recap.project.is_none(),
            "no existing file, no fake 0% before"
        );
        let added = recap.added.expect("added files roll up on their own");
        assert_eq!(added.files, 2);
        assert!(added.pillars["simple"].passed);
    }

    /// Before and after cover the same existing files; a clean new file
    /// does not join the head side and lift its mean.
    #[test]
    fn the_rollup_compares_one_population() {
        let (_keep, repo) = write_repo(&[("src/a.py", &branchy("pick"))]);
        write_files(
            &repo,
            &[
                ("src/a.py", &format!("{}\n", branchy("pick"))),
                ("src/clean.py", "def fine():\n    return 3\n"),
            ],
        );
        commit_all(&repo, "clean new file");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);
        let project = recap.project.expect("one existing file");
        assert_eq!((project.files_before, project.files_after), (1, 1));
        let simple = &project.pillars["simple"];
        assert_eq!((simple.files_before, simple.files_after), (1, 1));
        assert_eq!(simple.failing_after, 1);
        assert_eq!(simple.before_score, simple.after_score);
        assert_eq!(recap.added.expect("one new file").files, 1);
    }

    #[test]
    fn the_cap_keeps_the_most_changed_files() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        write_files(
            &repo,
            &[
                ("src/a.py", "x = 1\n"),
                ("src/b.py", "x = 1\ny = 2\n"),
                ("src/z.py", &branchy("pick")),
            ],
        );
        commit_all(&repo, "three");
        let recap = recap(&repo, "HEAD~1", "HEAD", 1);
        assert_eq!(recap.files.len(), 1);
        assert_eq!(
            recap.files[0].path, "src/z.py",
            "most churn wins, not alphabet"
        );
        assert!(recap.incomplete);
        assert_eq!(recap.scope.files_capped, 2);
        assert!(recap.reason.contains("Incomplete: 2"), "{}", recap.reason);
        assert_eq!(
            recap.skipped[0].path, "src/b.py",
            "dropped most-churn first"
        );
    }

    #[test]
    fn the_worktree_includes_untracked_files() {
        let (_keep, repo) = write_repo(&[("src/a.py", "def ready():\n    return 1\n")]);
        write_files(&repo, &[("src/fresh.py", "def fresh():\n    return 2\n")]);
        let recap = recap(&repo, "HEAD", ":worktree", 40);
        let fresh = recap
            .files
            .iter()
            .find(|f| f.path == "src/fresh.py")
            .expect("an untracked file is part of the worktree edit");
        assert_eq!(fresh.change, FileChange::Added);
    }

    #[test]
    fn no_arguments_review_the_uncommitted_edits() {
        let (_keep, repo) = write_repo(&[("src/a.py", "x = 1\n")]);
        let (base, head, review) = resolve_range(&repo, None, None, None).unwrap();
        assert_eq!((base.as_str(), head.as_str()), ("HEAD", ":worktree"));
        assert!(review.is_none());
        let clean = recap(&repo, &base, &head, 40);
        assert!(clean.reason.contains("working tree"), "{}", clean.reason);
    }

    #[test]
    fn priority_honors_the_flag_then_the_config_then_secure() {
        let unset = ToposConfig::default();
        assert_eq!(resolve_priority(None, &unset).unwrap(), Priority::Secure);
        let configured = ToposConfig {
            priority: Some(Priority::Composable),
            ..Default::default()
        };
        assert_eq!(
            resolve_priority(None, &configured).unwrap(),
            Priority::Composable
        );
        assert_eq!(
            resolve_priority(Some("navigable"), &configured).unwrap(),
            Priority::Navigable
        );
        assert_eq!(
            resolve_priority(Some("simple,secure,composable,navigable"), &unset).unwrap(),
            Priority::Simple
        );
        assert!(resolve_priority(Some("fast"), &unset).is_err());
    }

    #[test]
    fn the_recap_names_its_priority() {
        let (_keep, repo) = write_repo(&[("src/a.py", "def ready():\n    return 1\n")]);
        write_files(&repo, &[("src/a.py", "def ready():\n    return 2\n")]);
        commit_all(&repo, "edit");
        let recap = build_recap(
            &repo,
            "HEAD~1",
            "HEAD",
            40,
            None,
            no_coupling(),
            &judging(Priority::Navigable),
        )
        .unwrap();
        assert_eq!(recap.priority, "navigable");
    }

    #[test]
    fn the_gate_policy_honors_the_flags_then_the_file_then_recommended() {
        let recommended = PrGateConfig::default();
        let by_default = resolve_gate_policy(false, None, &recommended);
        assert_eq!(by_default, recommended);
        assert_eq!(by_default.fail_on, FailOn::Block);

        let mut file = PrGateConfig::for_preset(PrGatePreset::Relaxed);
        file.gates.set(GateId::Cosmetic, Severity::Off);
        assert_eq!(
            resolve_gate_policy(false, None, &file),
            file,
            "the file as is"
        );

        let strict = resolve_gate_policy(true, None, &file);
        assert_eq!(strict.fail_on, FailOn::Warn, "--strict fails on warn");
        assert_eq!(
            strict.severity(GateId::Cosmetic),
            Severity::Off,
            "the rest is the file's"
        );
        assert_eq!(strict.preset, PrGatePreset::Relaxed);

        let preset = resolve_gate_policy(false, Some(GatePreset::Strict), &file);
        assert_eq!(
            preset,
            PrGateConfig::for_preset(PrGatePreset::Strict),
            "--preset ignores the file"
        );
        let both = resolve_gate_policy(true, Some(GatePreset::Recommended), &file);
        assert_eq!(both.preset, PrGatePreset::Recommended);
        assert_eq!(both.fail_on, FailOn::Warn);
        assert_eq!(
            both.overrides().len(),
            1,
            "--strict is one change from the preset"
        );
    }

    /// An allowlisted dangerous call is acknowledged risk: a new file
    /// making it does not trip `new_file_insecure`, as MCP would not fail it.
    #[test]
    fn an_allowlisted_call_in_a_new_file_is_not_insecure() {
        use topos_engine::config::AllowEntry;
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        write_files(&repo, &[("src/run.py", SHELL)]);
        commit_all(&repo, "shell in a new file");
        let run = |allow: Vec<AllowEntry>| {
            let judging = Judging {
                topos: ToposConfig {
                    allow,
                    root: Some(repo.clone()),
                    ..Default::default()
                },
                ..judging(Priority::Secure)
            };
            build_recap(&repo, "HEAD~1", "HEAD", 40, None, no_coupling(), &judging).unwrap()
        };
        let bare = run(Vec::new());
        assert_eq!(
            bare.findings[0].gate,
            GateId::NewFileInsecure,
            "{:?}",
            bare.findings
        );

        let allowed = run(vec![AllowEntry {
            pattern: "os.system".to_string(),
            reason: "the runner shells out by design".to_string(),
            scope: String::new(),
        }]);
        assert!(
            !allowed
                .findings
                .iter()
                .any(|f| f.gate == GateId::NewFileInsecure),
            "{:?}",
            allowed.findings
        );
        assert_eq!(allowed.files[0].pillars["secure"].after_passed, Some(true));
        assert_eq!(allowed.exit_code, 0);
    }

    #[test]
    fn the_json_document_is_v3() {
        let (_keep, repo) = write_repo(&[("src/run.py", "def ready():\n    return 1\n")]);
        write_files(&repo, &[("src/run.py", SHELL)]);
        commit_all(&repo, "shell");
        let judging = Judging {
            gate: resolve_gate_policy(true, None, &PrGateConfig::default()),
            source: Some(repo.join(".topos.toml")),
            ..judging(Priority::Secure)
        };
        let recap =
            build_recap(&repo, "HEAD~1", "HEAD", 40, None, no_coupling(), &judging).unwrap();
        let json = serde_json::to_value(&recap).unwrap();
        assert_eq!(json["schema"], "topos.pr_recap.v3");
        assert!(json.get("headline").is_none(), "headline is now direction");
        assert_eq!(json["direction"], "REGRESSION");
        assert_eq!(json["readiness"], "BLOCKED");
        assert_eq!(
            (json["exit_code"].as_i64(), json["check"].as_str()),
            (Some(1), Some("fail"))
        );
        assert_eq!(json["gate"]["preset"], "recommended");
        assert_eq!(json["gate"]["fail_on"], "warn");
        assert_eq!(json["gate"]["changes"], 1);
        assert_eq!(
            json["gate"]["source"].as_str(),
            Some(repo.join(".topos.toml").to_string_lossy().as_ref())
        );
        let finding = &json["findings"][0];
        assert_eq!(finding["gate"], "pillar_lost");
        assert_eq!(finding["severity"], "block");
        assert_eq!(finding["pillar"], "secure");
        assert_eq!(finding["material"], true);
        assert_eq!(finding["inherited"], false);
        for key in [
            "path", "line", "function", "metric", "before", "after", "limit", "fix", "text",
        ] {
            assert!(finding.get(key).is_some(), "finding has {key}");
        }
        assert_eq!(json["reason"], finding["text"]);
        assert_eq!(json["files"][0]["severity"], "block");
        assert!(json["files"][0]["pillars"]["secure"]["gate"]["limit"].is_number());
    }

    /// Errors come back as `Err` for `run` to turn into exit 2, never as a
    /// passing document.
    #[test]
    fn a_bad_range_is_an_error_not_a_verdict() {
        let (_keep, repo) = write_repo(&[("src/a.py", "x = 1\n")]);
        let args = PrRecapArgs {
            pr: None,
            base: Some("no-such-ref".to_string()),
            head: None,
            repo: Some(repo),
            json: true,
            max_files: DEFAULT_FILE_CAP,
            verbose: false,
            info: false,
            compact: false,
            format: None,
            no_coupling: true,
            yes: false,
            no_input: false,
            priority: None,
            strict: false,
            preset: None,
        };
        let error = run_recap(args).unwrap_err();
        assert!(error.contains("no-such-ref"), "{error}");
    }
}
