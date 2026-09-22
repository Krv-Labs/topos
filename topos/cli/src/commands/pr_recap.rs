//! `topos pr-recap` — the data builder for schema `topos.pr_recap.v2`.
//!
//! Scores added and modified source files at `--base` and `--head`, groups
//! the ones that look like a split into clusters, and hands a single
//! [`model::PrRecap`] document to whichever renderer the caller asked for.
//! Every verdict on a card is decided here, from the lattice, the UAST
//! ledger and the two coupling graphs — a formatter can never invent one.

mod compact;
mod github;
mod model;
mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Args, ValueEnum};
use console::{Style, Term};
use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::core::omega::{verdict_from_generators, EvaluationValue, Generator, Omega};
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::policies::calibration::{COMPOSABLE, NAVIGABLE, SIMPLE};
use topos_engine::evaluation::policies::gates::pillar_for_metric;
use topos_engine::evaluation::security_guidance::remediation_for;
use topos_engine::functors::probes::ast::complexity::calculate_function_complexity_entries;
use topos_engine::functors::probes::ast::divergence::calculate_function_divergence_entries;
use topos_engine::functors::profunctors::ast::compare::calculate_ast_distance;
use topos_engine::functors::profunctors::uast::ledger::{
    match_functions, FunctionSnapshot, Ledger, MatchKind,
};
use topos_engine::graphs::ast::languages::all_source_suffixes;
use topos_engine::graphs::mdg::object::ModuleDependencyGraph;
use topos_engine::graphs::mdg::split::{
    detect_splits, fan_out_excluding, ChangedFile, FileChange as SplitChange, Reach, SplitCluster,
};

use self::model::*;
use super::classify::classify_with_representations;
use super::lang::detect_language;
use crate::commands::depgraph::{git_root, gitnexus_available, prepare_pr_stores, PrStores};
use crate::commands::render::{paint, RenderOptions, Working};

/// Below this, a score dip is noise (0.1 on the displayed 0–100 scale).
const SCORE_REGRESSION_FLOOR: f64 = 0.001;
const DEFAULT_FILE_CAP: usize = 40;
const HOTSPOT_CAP: usize = 2;

const SKIP_PREFIXES: &[&str] = &[
    "openwiki/",
    "target/",
    "node_modules/",
    "dist/",
    "vendor/",
    ".git/",
];

/// Lines that could be an import of a sibling module, for the no-graph
/// split fallback.
const IMPORT_PREFIXES: &[&str] = &["import", "from", "use", "#include", "require("];

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RecapFormat {
    /// Full terminal review card.
    Card,
    /// One-screen card for a CI log.
    Compact,
    /// Markdown for a sticky pull request comment.
    Github,
}

/// The long help for `topos pr-recap`, printed by `--help` under the
/// flag list. `-h` stays short: it shows only the flag one-liners.
pub const LONG_HELP: &str = r#"What it does:
  Scores the files your change touched at the base commit and at the head commit,
  then reports the structural difference between the two. It is deterministic and
  reads only your code: no LLM, no network call, no model judgement. The medal is
  Topos's lattice verdict over the pillars below. Structure moving in the right
  direction is not proof that behaviour is unchanged or that the tests still pass;
  read it as a review aid, not as a green check.

Pillars (S C E N):
  S  SIMPLE      per-function complexity and the control-flow gates.
  C  COMPOSABLE  module coupling from the GitNexus dependency graph: fan-out and
                 instability.
  E  SECURE      dangerous calls and taint flows.
  N  NAVIGABLE   nesting divergence.
  In the matrix a pillar is `●` when it passes at head, `○` when it fails, and `·`
  when it was not measured (COMPOSABLE with no graph, or a file that did not
  parse). On a modified file, `↑` or `↓` next to the mark means that pillar's score
  moved by at least one point.

Medals:
  PLATINUM  all four pillars pass.
  GOLD      three pass.
  SILVER    two pass.
  BRONZE    one passes.
  SLOP      none pass.
  `BRONZE → SILVER` means the medal itself changed over this range.

Rows:
  ✓ UP        a pillar was cleared, or a score rose.
  ! DOWN      a score fell, but the medal held.
  X LOST      a pillar was lost.
  ! COSMETIC  scores moved while the syntax tree barely changed (an agent-slop
              signal: the shape of the code is the same, the numbers are not).
  ✓ NEW       an added file that is not part of a split.
  SPLIT       a file whose code moved out into new files. It passes (✓) when the
              worst function got simpler and total decisions grew by no more than
              10%, warns (!) when decisions grew by more than 10% or a child
              landed SLOP, and fails (X) when the parent lost a pillar or a moved
              function came out more complex than it went in.
  Children (├─) are the new files a split produced. `N in` counts the symbols or
  functions that moved into that child; `shared ×N` means N files besides the
  parent import it; `N more` folds away the quiet children.

Splits table columns:
  WORST FN   the highest single-function complexity, before and after.
  DECISIONS  total decision points (cyclomatic) in the parent before, then in the
             parent and all of its children after. A `+P%` marks growth over 10%.

Project table:
  One row per pillar over every scored file: whether it passes at head, the mean
  score before and after, how many files fail it out of how many were measured,
  and a rail showing where the head score sits.

Headline / exit codes:
  IMPROVEMENT  structure got better.            exit 0
  SCORE UP     scores rose, medals held.        exit 0
  LATERAL      mixed or flat.                   exit 0
  SCORE DOWN   scores fell, medals held.        exit 1
  REGRESSION   a pillar or a medal was lost.    exit 1
  SUSPICIOUS   the change looks cosmetic.       exit 1
  An error exits 2. The worst file decides the headline for the whole range.

Coupling:
  Given a PR number and an installed GitNexus, both commits are indexed under
  `.git/topos-pr-<N>/` (a few seconds each) so that COMPOSABLE and split tracing
  use real import and call edges. `--no-coupling` skips that work and reports
  COMPOSABLE as not measured. With `--base/--head` there is no PR store, so
  splits are detected from import lines and the moved-function ledger instead.

Outputs:
  --format card     the full review card; the default on a terminal.
  --format compact  at most 12 lines; the default when output is piped.
  --format github   markdown for a sticky PR comment, with a hidden marker so a
                    later run replaces it instead of adding another comment.
  --json            schema topos.pr_recap.v2: every number behind the card.
  --verbose         every split child, each moved function, every score change.

Examples:
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
    /// Commit the change ends at; `:worktree` includes uncommitted edits.
    #[arg(long)]
    pub head: Option<String>,
    /// Repository to read. Defaults to the current directory.
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Print the machine-readable document instead of the review card.
    #[arg(long)]
    pub json: bool,
    /// Score at most this many added or modified files.
    #[arg(long, default_value_t = DEFAULT_FILE_CAP)]
    pub max_files: usize,
    /// Unfold every split and print the per-function ledger.
    #[arg(long)]
    pub verbose: bool,
    /// Print the short CI card. Same as `--format compact`.
    #[arg(long)]
    pub compact: bool,
    /// Which card to print: card, compact or github.
    #[arg(long, value_enum)]
    pub format: Option<RecapFormat>,
    /// Skip coupling preparation; COMPOSABLE is reported as not measured.
    #[arg(long)]
    pub no_coupling: bool,
}

/// Which card to print, once `--compact`, `--format` and the terminal have
/// all had their say. `--json` is decided by the caller and wins over this.
fn resolve_format(compact: bool, format: Option<RecapFormat>, is_term: bool) -> RecapFormat {
    if compact {
        return RecapFormat::Compact;
    }
    if let Some(format) = format {
        return format;
    }
    if is_term {
        RecapFormat::Card
    } else {
        RecapFormat::Compact
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
        (None, None, None) => Ok(("HEAD".to_string(), "HEAD".to_string(), None)),
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

struct PullRequestRefs {
    number: u64,
    head_ref: String,
    base_ref: String,
    head_sha: String,
    base_sha: String,
}

fn pull_request(repo: &Path, number: u64) -> Result<PullRequestRefs, String> {
    let output = Command::new("gh")
        .current_dir(repo)
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--json",
            "number,baseRefName,headRefName,baseRefOid,headRefOid,isCrossRepository",
        ])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                missing_gh(number)
            } else {
                format!("running gh: {e}")
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh pr view {number}: {}", stderr.trim()));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("reading pull request {number}: {e}"))?;
    if value
        .get("isCrossRepository")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return Err("cross-repository pull requests are not fetched by this command".to_string());
    }
    let field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("pull request {number} has no {key}"))
    };
    Ok(PullRequestRefs {
        number,
        head_ref: field("headRefName")?,
        base_ref: field("baseRefName")?,
        head_sha: field("headRefOid")?,
        base_sha: field("baseRefOid")?,
    })
}

fn missing_gh(number: u64) -> String {
    format!(
        "gh is not installed, so pull request {number} cannot be resolved.\n\
         Install it with `brew install gh`, or pass the commits directly:\n\
         topos pr-recap --base <base-sha> --head <head-sha>"
    )
}

fn ensure_commit(repo: &Path, sha: &str) -> Result<(), String> {
    if resolve_commit(repo, sha).is_ok() {
        return Ok(());
    }
    git(repo, &["fetch", "--no-tags", "origin", sha])
        .map(|_| ())
        .map_err(|_| format!("could not fetch {sha}. The commit may be from a fork."))
}

// --- Entry point -------------------------------------------------------

/// Why COMPOSABLE is not measured on this run.
fn unmeasured_coupling(pr: Option<u64>, no_coupling: bool) -> CouplingStatus {
    let note = if no_coupling {
        "skipped (--no-coupling)".to_string()
    } else if pr.is_none() {
        "pass a pull request number to measure COMPOSABLE".to_string()
    } else {
        "gitnexus not installed (npm install -g gitnexus)".to_string()
    };
    CouplingStatus {
        measured: false,
        note,
    }
}

pub fn run(args: PrRecapArgs) -> Result<(), String> {
    let repo = args
        .repo
        .clone()
        .unwrap_or(std::env::current_dir().map_err(|e| format!("current directory: {e}"))?);
    let root = git_root(&repo)?;
    let (base, head, review) = resolve_range(&root, args.pr, args.base.clone(), args.head.clone())?;
    let format = resolve_format(args.compact, args.format, Term::stdout().is_term());

    // The spinner covers store generation too: that is the slow part.
    let working = (!args.json).then(Working::start);
    let (stores, coupling) = coupling_stores(&root, &base, &head, &args);
    let recap = build_recap(
        &root,
        &base,
        &head,
        args.max_files,
        stores.as_ref(),
        coupling,
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
            RecapFormat::Card => {
                let options = RenderOptions::stdout();
                println!();
                println!("...");
                for line in render::render_card(&recap, args.verbose, options) {
                    println!("{line}");
                }
                // The card's own tips sit outside it, like `evaluate`'s.
                println!();
                for tip in render::tips(&recap, args.verbose) {
                    println!("{}", paint(tip, Style::new().dim(), options));
                }
                println!();
            }
            RecapFormat::Compact => {
                for line in compact::render_compact(&recap, RenderOptions::stdout()) {
                    println!("{line}");
                }
            }
            RecapFormat::Github => println!("{}", github::render_github(&recap)),
        }
    }
    if recap.headline.fails_check() && recap.error.is_none() {
        std::process::exit(1);
    }
    if recap.error.is_some() {
        std::process::exit(2);
    }
    Ok(())
}

/// Build (or reuse) the two coupling stores, and say why not when we can't.
fn coupling_stores(
    root: &Path,
    base: &str,
    head: &str,
    args: &PrRecapArgs,
) -> (Option<PrStores>, CouplingStatus) {
    let Some(pr) = args.pr.filter(|_| !args.no_coupling) else {
        return (None, unmeasured_coupling(args.pr, args.no_coupling));
    };
    if !gitnexus_available() {
        return (None, unmeasured_coupling(args.pr, args.no_coupling));
    }
    let (Ok(base_sha), Ok(head_sha)) = (resolve_commit(root, base), resolve_commit(root, head))
    else {
        return (
            None,
            CouplingStatus {
                measured: false,
                note: format!("could not resolve {base}...{head}"),
            },
        );
    };
    match prepare_pr_stores(root, pr, &base_sha, &head_sha) {
        Ok(stores) => {
            let note = format!("built from {}", stores.parent.display());
            (
                Some(stores),
                CouplingStatus {
                    measured: true,
                    note,
                },
            )
        }
        Err(error) => (
            None,
            CouplingStatus {
                measured: false,
                note: error,
            },
        ),
    }
}

// --- Document assembly -------------------------------------------------

/// One scored file with the intermediate state the later passes need.
struct Scored {
    recap: FileRecap,
    before: ClassificationResult,
    after: ClassificationResult,
    after_src: String,
    before_snapshots: Option<Vec<FunctionSnapshot>>,
    after_snapshots: Option<Vec<FunctionSnapshot>>,
    distance: Option<f64>,
    is_new: bool,
}

fn build_recap(
    repo: &Path,
    base: &str,
    head: &str,
    max_files: usize,
    stores: Option<&PrStores>,
    coupling: CouplingStatus,
) -> Result<PrRecap, String> {
    let repo = git_root(repo)?;
    let worktree = head == ":worktree";
    let base_sha = resolve_commit(&repo, base)?;
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
        for entry in scoreable.drain(max_files..) {
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
    let classifier = CharacteristicMorphism;
    let mut scored = Vec::new();
    for entry in &scoreable {
        scored.push(score_file(
            &repo,
            &base_sha,
            &head_sha,
            entry,
            &classifier,
            base_graph.as_ref().filter(|_| measured),
            head_graph.as_ref().filter(|_| measured),
        )?);
    }

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

    let files: Vec<FileRecap> = scored.into_iter().map(|file| file.recap).collect();
    let (headline, reason) = headline_for(&files, &base_sha, &head_sha);
    let hotspots = top_hotspots(&files);
    let project = project_rollup(&files);
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
    Ok(PrRecap {
        schema: SCHEMA,
        base: base_sha,
        head: head_sha,
        review: None,
        headline,
        check: if headline.fails_check() {
            "fail"
        } else {
            "pass"
        },
        reason,
        error: None,
        scope,
        project,
        clusters,
        files,
        skipped,
        deleted: diff.deleted,
        hotspots,
        non_claim: "Structural direction is not proof that tests or behavior still pass.",
    })
}

fn load_graph(store: &Path) -> Option<ModuleDependencyGraph> {
    ModuleDependencyGraph::from_lbug_path(&store.join(".gitnexus").join("lbug"), "").ok()
}

fn changed_list(entries: &[DiffEntry], deleted: &[String]) -> Vec<ChangedFile> {
    let mut changed: Vec<ChangedFile> = entries
        .iter()
        .map(|entry| ChangedFile {
            path: entry.path.clone(),
            change: match file_change(&entry.status) {
                FileChange::Added => SplitChange::Added,
                FileChange::Renamed => SplitChange::Renamed,
                FileChange::Modified => SplitChange::Modified,
            },
        })
        .collect();
    changed.extend(deleted.iter().map(|path| ChangedFile {
        path: path.clone(),
        change: SplitChange::Deleted,
    }));
    changed
}

// --- Git plumbing ------------------------------------------------------

struct DiffEntry {
    status: String,
    path: String,
    /// Pre-rename path, set only for `R*` entries. `git diff --name-status
    /// --find-renames` emits `R100\told/path\tnew/path`; the base revision
    /// only has `old/path`, so callers reading the base side must use this
    /// instead of `path`.
    old_path: Option<String>,
}

struct Diff {
    entries: Vec<DiffEntry>,
    deleted: Vec<String>,
}

fn resolve_commit(repo: &Path, rev: &str) -> Result<String, String> {
    if !is_safe_rev(rev) {
        return Err(format!("refusing revision '{rev}'"));
    }
    let output = git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{rev}^{{commit}}"),
        ],
    )
    .map_err(|_| format!("could not resolve '{rev}' to a commit"))?;
    Ok(output.trim().to_string())
}

fn worktree_files(repo: &Path, base: &str) -> Result<Diff, String> {
    let output = git(
        repo,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "--end-of-options",
            base,
        ],
    )?;
    parse_name_status(&output)
}

fn changed_files(repo: &Path, base: &str, head: &str) -> Result<Diff, String> {
    let output = git(
        repo,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "--end-of-options",
            &format!("{base}...{head}"),
        ],
    )?;
    parse_name_status(&output)
}

fn parse_name_status(output: &str) -> Result<Diff, String> {
    let mut entries = Vec::new();
    let mut deleted = Vec::new();
    for line in output.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let status = parts.next().unwrap_or("").to_string();
        let paths: Vec<&str> = parts.collect();
        let path = paths.last().unwrap_or(&"").to_string();
        if path.is_empty() {
            continue;
        }
        if status.starts_with('D') {
            deleted.push(path);
        } else if status.starts_with('A') || status.starts_with('M') || status.starts_with('R') {
            let old_path =
                (status.starts_with('R') && paths.len() >= 2).then(|| paths[0].to_string());
            entries.push(DiffEntry {
                status,
                path,
                old_path,
            });
        }
    }
    Ok(Diff { entries, deleted })
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "git is not available".to_string()
            } else {
                format!("running git: {e}")
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {}: {}", args.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn is_safe_rev(rev: &str) -> bool {
    !rev.is_empty() && !rev.starts_with('-') && !rev.contains('\0')
}

fn skip_reason(entry: &DiffEntry) -> Option<String> {
    let path = entry.path.replace('\\', "/");
    if SKIP_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return Some("generated or vendor path".to_string());
    }
    let suffixes = all_source_suffixes();
    let supported = suffixes.iter().any(|suffix| path.ends_with(suffix));
    if !supported {
        return Some("not a supported source file".to_string());
    }
    None
}

fn show_file(repo: &Path, rev: &str, path: &str) -> Result<String, String> {
    git(
        repo,
        &["show", "--end-of-options", &format!("{rev}:{path}")],
    )
    .map_err(|_| format!("could not read {path} at {rev}"))
}

fn file_change(status: &str) -> FileChange {
    match status.chars().next() {
        Some('A') => FileChange::Added,
        Some('R') => FileChange::Renamed,
        _ => FileChange::Modified,
    }
}

// --- Pass A: one file at a time ---------------------------------------

/// Everything one parse of one revision of one file yields.
struct ParsedSide {
    worst: Option<FunctionRef>,
    snapshots: Option<Vec<FunctionSnapshot>>,
}

fn parse_side(source: &str, language: &str, path: &str) -> ParsedSide {
    let morphism = ProgramMorphism::with_path(source, language, path);
    let Some(ast) = morphism.ast.as_ref().filter(|_| morphism.is_valid()) else {
        return ParsedSide {
            worst: None,
            snapshots: None,
        };
    };
    let worst = calculate_function_complexity_entries(&ast.uast_root, source)
        .into_iter()
        .max_by_key(|entry| entry.complexity)
        .map(|entry| FunctionRef {
            name: entry.qualified_name,
            line: entry.start_line,
            complexity: entry.complexity,
        });
    let snapshots = Some(
        topos_engine::functors::profunctors::uast::ledger::snapshot_functions(
            &ast.uast_root,
            source,
            path,
        ),
    );
    ParsedSide { worst, snapshots }
}

#[allow(clippy::too_many_arguments)]
fn score_file(
    repo: &Path,
    base: &str,
    head: &str,
    entry: &DiffEntry,
    classifier: &CharacteristicMorphism,
    base_graph: Option<&ModuleDependencyGraph>,
    head_graph: Option<&ModuleDependencyGraph>,
) -> Result<Scored, String> {
    let path = entry.path.clone();
    let language = detect_language(Path::new(&path));
    let change = file_change(&entry.status);
    let is_new = change == FileChange::Added;
    let base_path = entry.old_path.as_deref().unwrap_or(&path);
    let before_src = if is_new {
        String::new()
    } else {
        show_file(repo, base, base_path)?
    };
    let after_src = if head == "worktree" {
        std::fs::read_to_string(repo.join(&path)).map_err(|e| format!("reading {path}: {e}"))?
    } else {
        show_file(repo, head, &path)?
    };

    let before = classify_source(
        &before_src,
        &language,
        &path,
        classifier,
        targeted(base_graph, &path).as_ref(),
    );
    let after = classify_source(
        &after_src,
        &language,
        &path,
        classifier,
        targeted(head_graph, &path).as_ref(),
    );
    let distance = structural_distance(&before_src, &after_src, &language, &path);

    let before_side = if is_new {
        ParsedSide {
            worst: None,
            snapshots: None,
        }
    } else {
        parse_side(&before_src, &language, &path)
    };
    let after_side = parse_side(&after_src, &language, &path);

    let before_verdict = measured_verdict(&before);
    let after_verdict = measured_verdict(&after);
    let hotspots = file_hotspots(&path, &before_src, &after_src, &language, &before, &after);
    let (lines_added, lines_removed) = line_delta(&before_src, &after_src);
    let measured = base_graph.is_some() && head_graph.is_some();

    let recap = FileRecap {
        path: path.clone(),
        change,
        // Pass C decides this, once cluster fan-out is known.
        status: Headline::LateralMove,
        lines_before: before_src.lines().count(),
        lines_after: after_src.lines().count(),
        lines_added,
        lines_removed,
        medal_before: (!is_new).then(|| medal(before_verdict)),
        medal_after: after.is_parseable.then(|| medal(after_verdict)),
        pillars: pillar_deltas(&before, &after, is_new),
        structural_distance: distance,
        cosmetic: false,
        complexity_relocated_within_file: complexity_relocated(&before, &after),
        worst_function_before: before_side.worst.clone(),
        worst_function_after: after_side.worst.clone(),
        decisions_before: (!is_new).then(|| decisions(&before)).flatten(),
        decisions_after: decisions(&after),
        fan_in_before: measured.then(|| raw(&before, "mdg.fan_in")).flatten(),
        fan_in_after: measured.then(|| raw(&after, "mdg.fan_in")).flatten(),
        fan_out_before: measured.then(|| raw(&before, "mdg.fan_out")).flatten(),
        fan_out_after: measured.then(|| raw(&after, "mdg.fan_out")).flatten(),
        cluster: None,
        hotspots,
    };
    Ok(Scored {
        recap,
        before,
        after,
        after_src,
        before_snapshots: before_side.snapshots,
        after_snapshots: after_side.snapshots,
        distance,
        is_new,
    })
}

/// A clone of the repository graph aimed at one file. Cloning is far
/// cheaper than re-reading the store for every path.
fn targeted(graph: Option<&ModuleDependencyGraph>, path: &str) -> Option<ModuleDependencyGraph> {
    let mut graph = graph?.clone();
    graph.target_file = path.to_string();
    Some(graph)
}

fn raw(result: &ClassificationResult, key: &str) -> Option<usize> {
    result.raw_metrics.get(key).map(|value| *value as usize)
}

fn decisions(result: &ClassificationResult) -> Option<usize> {
    raw(result, "cfg.cyclomatic")
}

fn medal(value: EvaluationValue) -> Medal {
    Medal {
        symbol: value.symbol().to_string(),
        tier: value.medal_tier().to_string(),
        verdict: value.name().to_string(),
    }
}

fn line_delta(before: &str, after: &str) -> (usize, usize) {
    let before_lines: std::collections::HashMap<&str, usize> = counts(before);
    let after_lines: std::collections::HashMap<&str, usize> = counts(after);
    let removed = before_lines
        .iter()
        .map(|(line, count)| count.saturating_sub(*after_lines.get(line).unwrap_or(&0)))
        .sum();
    let added = after_lines
        .iter()
        .map(|(line, count)| count.saturating_sub(*before_lines.get(line).unwrap_or(&0)))
        .sum();
    (added, removed)
}

fn counts(source: &str) -> std::collections::HashMap<&str, usize> {
    let mut counts = std::collections::HashMap::new();
    for line in source.lines() {
        *counts.entry(line).or_insert(0) += 1;
    }
    counts
}

fn classify_source(
    source: &str,
    language: &str,
    path: &str,
    classifier: &CharacteristicMorphism,
    graph: Option<&ModuleDependencyGraph>,
) -> ClassificationResult {
    let mut morphism = ProgramMorphism::with_path(source, language, path);
    classify_with_representations(classifier, &mut morphism, graph, Priority::Secure)
}

fn structural_distance(before: &str, after: &str, language: &str, path: &str) -> Option<f64> {
    if before.is_empty() {
        return None;
    }
    let base = ProgramMorphism::with_path(before, language, path);
    let proposed = ProgramMorphism::with_path(after, language, path);
    match (base.ast.as_ref(), proposed.ast.as_ref()) {
        (Some(base_ast), Some(proposed_ast)) if base.is_valid() && proposed.is_valid() => {
            Some(calculate_ast_distance(base_ast, proposed_ast).normalized_distance)
        }
        _ => None,
    }
}

// --- Pass B: split clusters -------------------------------------------

/// A parent and its children, before the arithmetic is done.
struct Seed<'a> {
    parent: String,
    children: Vec<String>,
    split: Option<&'a SplitCluster>,
}

/// Group split parents with the children actually carved out of them.
///
/// Both seed passes are candidate finders only — an import line or a graph
/// edge says "the parent now uses this file", not "this file came out of
/// the parent". The ledger decides: a child survives only with moved-code
/// evidence (graph `moved_in`, or a `Moved*` match landing in it), so a
/// brand-new module the parent merely started calling renders as a plain
/// NEW row instead of a bogus `! SPLIT`.
fn build_clusters(
    scored: &mut [Scored],
    report: &[SplitCluster],
    measured: bool,
    head_graph: Option<&ModuleDependencyGraph>,
) -> Vec<Cluster> {
    let index: BTreeMap<String, usize> = scored
        .iter()
        .enumerate()
        .map(|(i, file)| (file.recap.path.clone(), i))
        .collect();
    let seeds = if measured {
        graph_seeds(report, &index)
    } else {
        fallback_seeds(scored, &index)
    };

    let mut clusters = Vec::new();
    for seed in seeds {
        if seed.children.is_empty() {
            continue;
        }
        let candidate = materialize(&seed, scored, &index, head_graph);
        // `moved_in` is already max(graph evidence, ledger evidence); zero
        // means nothing travelled into this file, so it is not a child.
        let kept: Vec<String> = candidate
            .children
            .iter()
            .filter(|child| {
                child.moved_in > 0
                    // The cluster ledger is all-or-nothing over the
                    // candidate set: one unparseable sibling must not
                    // erase the evidence for the others.
                    || (candidate.ledger.is_none()
                        && index.get(&child.path).is_some_and(|i| {
                            solo_moved_in(&scored[index[&seed.parent]], &scored[*i], &child.path)
                                > 0
                        }))
            })
            .map(|child| child.path.clone())
            .collect();
        if kept.is_empty() {
            // No child survived: the parent keeps no cluster membership.
            continue;
        }
        // One prune pass only. Re-deriving the cluster re-runs the ledger
        // over the surviving set so no pruned file is counted anywhere.
        let cluster = if kept.len() == seed.children.len() {
            candidate
        } else {
            let pruned = Seed {
                parent: seed.parent.clone(),
                children: kept.clone(),
                split: seed.split,
            };
            materialize(&pruned, scored, &index, head_graph)
        };
        for (path, role) in std::iter::once((seed.parent.clone(), ClusterRole::Parent))
            .chain(kept.iter().map(|child| (child.clone(), ClusterRole::Child)))
        {
            if let Some(i) = index.get(&path) {
                scored[*i].recap.cluster = Some(ClusterMembership {
                    parent: seed.parent.clone(),
                    role,
                });
            }
        }
        clusters.push(cluster);
    }
    clusters
}

fn graph_seeds<'a>(report: &'a [SplitCluster], index: &BTreeMap<String, usize>) -> Vec<Seed<'a>> {
    report
        .iter()
        .filter(|cluster| index.contains_key(&cluster.parent))
        .map(|cluster| Seed {
            parent: cluster.parent.clone(),
            children: cluster
                .children
                .iter()
                .map(|child| child.path.clone())
                .filter(|path| index.contains_key(path))
                .collect(),
            split: Some(cluster),
        })
        .collect()
}

/// No coupling graphs: attribute each added file to the modified file whose
/// head source imports it most often. Ties go to the smallest path.
fn fallback_seeds<'a>(scored: &[Scored], index: &BTreeMap<String, usize>) -> Vec<Seed<'a>> {
    let parents: Vec<&str> = scored
        .iter()
        .filter(|file| file.recap.change == FileChange::Modified)
        .map(|file| file.recap.path.as_str())
        .collect();
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for child in scored.iter().filter(|file| file.is_new) {
        let stem = file_stem(&child.recap.path);
        if stem.is_empty() {
            continue;
        }
        let best = parents
            .iter()
            .filter_map(|parent| {
                let source = &scored[index[*parent]].after_src;
                let hits = import_hits(source, &stem);
                (hits > 0).then_some((hits, *parent))
            })
            // Most import lines wins; on a tie the smallest path does.
            .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(a.1)));
        if let Some((_, parent)) = best {
            grouped
                .entry(parent.to_string())
                .or_default()
                .push(child.recap.path.clone());
        }
    }
    grouped
        .into_iter()
        .map(|(parent, children)| Seed {
            parent,
            children,
            split: None,
        })
        .collect()
}

fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn import_hits(source: &str, stem: &str) -> usize {
    source
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            IMPORT_PREFIXES
                .iter()
                .any(|prefix| line.starts_with(prefix))
                || line.contains("require(")
        })
        .filter(|line| line.contains(stem))
        .count()
}

fn materialize(
    seed: &Seed<'_>,
    scored: &[Scored],
    index: &BTreeMap<String, usize>,
    head_graph: Option<&ModuleDependencyGraph>,
) -> Cluster {
    let parent = &scored[index[&seed.parent]];
    let kids: Vec<&Scored> = seed
        .children
        .iter()
        .map(|path| &scored[index[path]])
        .collect();

    let decisions_before = parent.recap.decisions_before.unwrap_or(0);
    let decisions_after = parent.recap.decisions_after.unwrap_or(0)
        + kids
            .iter()
            .map(|kid| kid.recap.decisions_after.unwrap_or(0))
            .sum::<usize>();
    let lines_after =
        parent.recap.lines_after + kids.iter().map(|kid| kid.recap.lines_after).sum::<usize>();
    let worst_function_after = std::iter::once(parent.recap.worst_function_after.clone())
        .chain(
            kids.iter()
                .map(|kid| kid.recap.worst_function_after.clone()),
        )
        .flatten()
        .max_by_key(|entry| entry.complexity);

    let kept: Vec<&str> = seed.children.iter().map(String::as_str).collect();
    let ledger = cluster_ledger(parent, &kids);
    let children = cluster_children(seed, &kids, ledger.as_ref());
    let (mark, reasons) = cluster_mark(
        parent,
        &children,
        &kids,
        ledger.as_ref(),
        decisions_before,
        decisions_after,
        &worst_function_after,
    );

    Cluster {
        parent: seed.parent.clone(),
        children,
        mark,
        reasons,
        lines_before: parent.recap.lines_before,
        lines_after,
        decisions_before,
        decisions_after,
        worst_function_before: parent.recap.worst_function_before.clone(),
        worst_function_after,
        parent_fan_out_before: seed.split.map(|split| split.parent_fan_out_before),
        parent_fan_out_after: seed.split.map(|split| split.parent_fan_out_after),
        // Recomputed on the head graph over the surviving children only;
        // the split report's value still counted the pruned ones.
        parent_fan_out_after_excluding_children: match head_graph {
            Some(graph) => Some(fan_out_excluding(graph, &seed.parent, &kept)),
            None => seed
                .split
                .map(|split| split.parent_fan_out_after_excluding_children),
        },
        symbols_moved: seed
            .split
            .map(|split| {
                split
                    .moved
                    .iter()
                    .filter(|entry| kept.contains(&entry.to.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        symbols_new: seed
            .split
            .map(|split| {
                split
                    .new_symbols
                    .iter()
                    .filter(|entry| kept.contains(&entry.file.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        symbols_lost: seed
            .split
            .map(|split| split.lost.clone())
            .unwrap_or_default(),
        ledger,
    }
}

/// The parent at base against the parent plus every child at head.
fn cluster_ledger(parent: &Scored, kids: &[&Scored]) -> Option<Ledger> {
    let before = parent.before_snapshots.clone()?;
    let mut after = parent.after_snapshots.clone()?;
    for kid in kids {
        after.extend(kid.after_snapshots.clone()?);
    }
    Some(match_functions(before, after))
}

fn cluster_children(
    seed: &Seed<'_>,
    kids: &[&Scored],
    ledger: Option<&Ledger>,
) -> Vec<ClusterChild> {
    kids.iter()
        .map(|kid| {
            let path = kid.recap.path.clone();
            let reported = seed
                .split
                .and_then(|split| split.children.iter().find(|child| child.path == path));
            ClusterChild {
                reach: reported.map(|child| child.reach),
                importers: reported
                    .map(|child| child.importers.clone())
                    .unwrap_or_default(),
                // A child whose only evidence is a moved anonymous
                // callback has graph `moved_in == 0`; the ledger sees it.
                moved_in: reported
                    .map_or(0, |child| child.moved_in)
                    .max(moved_into(ledger, &path)),
                path,
            }
        })
        .collect()
}

/// Moves from the parent into one child, ledgered on its own. Used only
/// when the whole-cluster ledger is `None` because some other candidate
/// child failed to parse.
fn solo_moved_in(parent: &Scored, kid: &Scored, path: &str) -> usize {
    let (Some(before), Some(mut after), Some(kid_after)) = (
        parent.before_snapshots.clone(),
        parent.after_snapshots.clone(),
        kid.after_snapshots.clone(),
    ) else {
        return 0;
    };
    after.extend(kid_after);
    moved_into(Some(&match_functions(before, after)), path)
}

/// Ledger moves landing in `child`, counting nested and anonymous
/// callables: JSX extracted into a new component often moves only
/// anonymous callbacks, and that is still moved code.
fn moved_into(ledger: Option<&Ledger>, child: &str) -> usize {
    let Some(ledger) = ledger else { return 0 };
    ledger
        .matches
        .iter()
        .filter(|entry| {
            matches!(
                entry.kind,
                MatchKind::MovedIdentical | MatchKind::MovedModified
            )
        })
        .filter(|entry| {
            entry
                .after
                .as_ref()
                .is_some_and(|after| after.file == child)
        })
        .count()
}

#[allow(clippy::too_many_arguments)]
fn cluster_mark(
    parent: &Scored,
    children: &[ClusterChild],
    kids: &[&Scored],
    ledger: Option<&Ledger>,
    decisions_before: usize,
    decisions_after: usize,
    worst_after: &Option<FunctionRef>,
) -> (ClusterMark, Vec<String>) {
    let mut reasons = Vec::new();
    let lost: Vec<&str> = parent
        .recap
        .pillars
        .iter()
        .filter(|(_, delta)| delta.lost())
        .map(|(pillar, _)| pillar.as_str())
        .collect();
    if !lost.is_empty() {
        reasons.push(format!("{} lost {}", parent.recap.path, lost.join(", ")));
    }
    let grew = ledger.is_some_and(|ledger| {
        ledger.matches.iter().any(|entry| {
            matches!(entry.kind, MatchKind::MovedModified | MatchKind::Renamed)
                && entry.complexity_delta > 0
        })
    });
    let worst_fell = parent
        .recap
        .worst_function_before
        .as_ref()
        .zip(worst_after.as_ref())
        .is_some_and(|(before, after)| after.complexity < before.complexity);
    if let (Some(before), Some(after)) = (
        parent.recap.worst_function_before.as_ref(),
        worst_after.as_ref(),
    ) {
        if before.complexity != after.complexity {
            reasons.push(format!(
                "worst function {}→{}",
                before.complexity, after.complexity
            ));
        }
    }
    if decisions_after != decisions_before {
        reasons.push(decision_reason(decisions_before, decisions_after));
    }
    let private = children
        .iter()
        .filter(|child| child.reach == Some(Reach::Private))
        .count();
    if private > 0 {
        reasons.push(format!("{private} of {} children private", children.len()));
    }

    if !lost.is_empty() || (grew && !worst_fell) {
        if grew {
            reasons.push("a moved function gained complexity on the way".to_string());
        }
        return (ClusterMark::Fail, reasons);
    }
    let bloated = decisions_after as f64 > decisions_before as f64 * (1.0 + CLUSTER_GROWTH_WARN);
    let sloppy = kids.iter().any(|kid| {
        kid.recap
            .medal_after
            .as_ref()
            .map(|medal| medal.tier == "SLOP")
            .unwrap_or(true)
    });
    if sloppy {
        reasons.push("a child is SLOP or did not parse".to_string());
    }
    if bloated || sloppy {
        (ClusterMark::Warn, reasons)
    } else {
        (ClusterMark::Ok, reasons)
    }
}

fn decision_reason(before: usize, after: usize) -> String {
    if after > before {
        let percent = if before == 0 {
            100
        } else {
            (((after as f64 - before as f64) / before as f64) * 100.0).round() as i64
        };
        format!("decisions rose {before}→{after} (+{percent}%)")
    } else {
        format!("decisions fell {before}→{after}")
    }
}

// --- Pass C: per-file verdicts ----------------------------------------

fn finish_statuses(scored: &mut [Scored], clusters: &[Cluster], lattice: &Omega) {
    // A split parent's raw fan-out rises simply because it now imports the
    // files it was carved into. That is not a coupling regression.
    let routed: Vec<&str> = clusters
        .iter()
        .filter(|cluster| {
            cluster
                .parent_fan_out_after_excluding_children
                .zip(cluster.parent_fan_out_before)
                .is_some_and(|(after, before)| after <= before)
        })
        .map(|cluster| cluster.parent.as_str())
        .collect();
    for file in scored.iter_mut() {
        let drop_composable = routed.contains(&file.recap.path.as_str());
        let deltas = score_deltas(&file.before, &file.after, drop_composable);
        let cosmetic = file
            .distance
            .is_some_and(|distance| distance < STRUCTURAL_CHANGE_THRESHOLD)
            && deltas.iter().any(|d| d.abs() >= MEANINGFUL_SCORE_DELTA);
        file.recap.cosmetic = !file.is_new && cosmetic;
        if drop_composable {
            file.recap
                .hotspots
                .retain(|spot| spot.metric != "mdg.fan_out");
        }
        // The verdict has to drop COMPOSABLE too. A parent that trips the
        // fan-out gate loses a pillar outright, and the score deltas are
        // never consulted once the medal itself moved.
        file.recap.status = file_status(
            &file.before,
            &file.after,
            measured_verdict_excluding(&file.before, drop_composable),
            measured_verdict_excluding(&file.after, drop_composable),
            cosmetic,
            &deltas,
            lattice,
            file.is_new,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn file_status(
    before: &ClassificationResult,
    after: &ClassificationResult,
    before_verdict: EvaluationValue,
    after_verdict: EvaluationValue,
    suspicious: bool,
    deltas: &[f64],
    lattice: &Omega,
    is_new: bool,
) -> Headline {
    if is_new {
        return if after_verdict == EvaluationValue::Slop {
            Headline::LateralMove
        } else {
            Headline::Improvement
        };
    }
    if !before.is_parseable || !after.is_parseable {
        return Headline::LateralMove;
    }
    if before_verdict == after_verdict {
        let improved = deltas.iter().any(|d| *d >= SCORE_REGRESSION_FLOOR);
        let regressed = deltas.iter().any(|d| *d <= -SCORE_REGRESSION_FLOOR);
        return if suspicious && improved {
            Headline::SuspiciousNoStructuralChange
        } else if improved && !regressed {
            Headline::ImprovementScore
        } else if regressed && !improved {
            Headline::RegressionScore
        } else {
            Headline::LateralMove
        };
    }
    // `leq(a, b)` means b is at least as good as a. A strict step up is an
    // improvement; a strict step down is a regression; otherwise the two
    // medals cleared different pillars and neither contains the other.
    if lattice.leq(before_verdict, after_verdict) {
        return if suspicious {
            Headline::SuspiciousNoStructuralChange
        } else {
            Headline::Improvement
        };
    }
    if lattice.leq(after_verdict, before_verdict) {
        Headline::Regression
    } else {
        Headline::LateralMove
    }
}

fn score_deltas(
    before: &ClassificationResult,
    after: &ClassificationResult,
    drop_composable: bool,
) -> Vec<f64> {
    Generator::ALL
        .into_iter()
        .filter(|g| !(drop_composable && g.as_str() == "composable"))
        .filter_map(|g| {
            let key = g.as_str();
            Some(after.scores.get(key)? - before.scores.get(key)?)
        })
        .collect()
}

fn pillar_deltas(
    before: &ClassificationResult,
    after: &ClassificationResult,
    is_new: bool,
) -> BTreeMap<String, PillarDelta> {
    Generator::ALL
        .into_iter()
        .map(|generator| {
            let key = generator.as_str();
            let measured = pillar_measured(before, key) || pillar_measured(after, key);
            (
                key.to_string(),
                PillarDelta {
                    measured,
                    before_passed: (!is_new && measured).then(|| pillar_passed(before, generator)),
                    after_passed: measured.then(|| pillar_passed(after, generator)),
                    before_score: (!is_new).then(|| rounded_score(before, key)).flatten(),
                    after_score: rounded_score(after, key),
                    lost_gate: (!is_new
                        && pillar_passed(before, generator)
                        && !pillar_passed(after, generator))
                    .then(|| lost_gate(before, after, key))
                    .flatten(),
                },
            )
        })
        .collect()
}

fn pillar_measured(result: &ClassificationResult, pillar: &str) -> bool {
    // `mdg.abstractness` can exist from the file alone. COMPOSABLE's gate
    // needs the dependency graph, which is only attached for a pull request.
    if pillar == "composable" {
        return result.raw_metrics.contains_key("mdg.fan_out");
    }
    result
        .raw_metrics
        .keys()
        .any(|key| pillar_for_metric(key) == pillar)
}

fn pillar_passed(result: &ClassificationResult, generator: Generator) -> bool {
    if !pillar_measured(result, generator.as_str()) {
        return false;
    }
    result
        .dimensions
        .get(generator.as_str())
        .is_some_and(|value| *value == generator.value())
}

/// Medal from the pillars this run actually measured.
///
/// Without a dependency graph, COMPOSABLE is not a pass and not a fail.
/// Counting it as failed would turn every file into a fake regression.
fn measured_verdict(result: &ClassificationResult) -> EvaluationValue {
    measured_verdict_excluding(result, false)
}

/// The same verdict with COMPOSABLE optionally set aside, for a split
/// parent whose fan-out rose only because it now imports its own children.
/// Only `file_status` uses this; the reported medal stays factual.
fn measured_verdict_excluding(
    result: &ClassificationResult,
    drop_composable: bool,
) -> EvaluationValue {
    let satisfied: Vec<Generator> = Generator::ALL
        .into_iter()
        .filter(|generator| !(drop_composable && generator.as_str() == "composable"))
        .filter(|generator| pillar_passed(result, *generator))
        .collect();
    verdict_from_generators(&satisfied)
}

fn lost_gate(
    before: &ClassificationResult,
    after: &ClassificationResult,
    pillar: &str,
) -> Option<String> {
    before
        .raw_metrics
        .keys()
        .chain(after.raw_metrics.keys())
        .filter(|key| pillar_for_metric(key) == pillar)
        .find(|key| {
            let was = before.raw_metrics.get(*key).copied().unwrap_or(0.0);
            let now = after.raw_metrics.get(*key).copied().unwrap_or(0.0);
            now > was && now > gate_limit(key).unwrap_or(f64::MAX)
        })
        .cloned()
}

fn gate_limit(metric: &str) -> Option<f64> {
    match metric {
        "ast.max_function_complexity" => Some(SIMPLE.max_function_complexity),
        "nav.max_function_divergence" => Some(NAVIGABLE.max_function_divergence),
        "mdg.fan_out" => Some(COMPOSABLE.max_fan_out),
        "cpg.dangerous_calls" | "cpg.taint_flows" => Some(0.0),
        _ => None,
    }
}

fn rounded_score(result: &ClassificationResult, pillar: &str) -> Option<f64> {
    result
        .scores
        .get(pillar)
        .map(|score| (score * 1000.0).round() / 10.0)
}

fn complexity_relocated(before: &ClassificationResult, after: &ClassificationResult) -> bool {
    let func = metric_delta(before, after, "ast.max_function_complexity");
    let file = metric_delta(before, after, "cfg.cyclomatic");
    func < 0.0 && file > 0.0
}

fn metric_delta(before: &ClassificationResult, after: &ClassificationResult, key: &str) -> f64 {
    after.raw_metrics.get(key).copied().unwrap_or(0.0)
        - before.raw_metrics.get(key).copied().unwrap_or(0.0)
}

fn metric_worsened(before: &ClassificationResult, after: &ClassificationResult, key: &str) -> bool {
    metric_delta(before, after, key) > 0.0
}

// --- Pass D: project rollup and headline -------------------------------

/// A pillar is achieved only if every file that measures it passes it.
/// Vacuous truth is excluded: a pillar nobody measured is not achieved.
fn project_rollup(files: &[FileRecap]) -> Option<ProjectRollup> {
    if files.is_empty() {
        return None;
    }
    let mut pillars = BTreeMap::new();
    let mut before_achieved = Vec::new();
    let mut after_achieved = Vec::new();
    for generator in Generator::ALL {
        let key = generator.as_str();
        let before: Vec<&PillarDelta> = files
            .iter()
            .filter(|file| !file.is_new())
            .filter_map(|file| file.pillars.get(key))
            .filter(|delta| delta.before_passed.is_some())
            .collect();
        let after: Vec<&PillarDelta> = files
            .iter()
            .filter_map(|file| file.pillars.get(key))
            .filter(|delta| delta.after_passed.is_some())
            .collect();
        if after.is_empty() {
            continue;
        }
        let before_passed =
            !before.is_empty() && before.iter().all(|d| d.before_passed == Some(true));
        let after_passed = after.iter().all(|d| d.after_passed == Some(true));
        if before_passed {
            before_achieved.push(generator);
        }
        if after_passed {
            after_achieved.push(generator);
        }
        pillars.insert(
            key.to_string(),
            PillarRollup {
                before_passed,
                after_passed,
                before_score: mean(before.iter().filter_map(|d| d.before_score)),
                after_score: mean(after.iter().filter_map(|d| d.after_score)),
                files_before: before.len(),
                files_after: after.len(),
                failing_before: before
                    .iter()
                    .filter(|d| d.before_passed == Some(false))
                    .count(),
                failing_after: after
                    .iter()
                    .filter(|d| d.after_passed == Some(false))
                    .count(),
            },
        );
    }
    let regression = pillars
        .values()
        .any(|pillar| pillar.before_passed && !pillar.after_passed);
    Some(ProjectRollup {
        medal_before: medal(verdict_from_generators(&before_achieved)),
        medal_after: medal(verdict_from_generators(&after_achieved)),
        pillars,
        regression,
        files_before: files.iter().filter(|file| !file.is_new()).count(),
        files_after: files.len(),
    })
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let values: Vec<f64> = values.collect();
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn headline_for(files: &[FileRecap], base: &str, head: &str) -> (Headline, String) {
    if files.is_empty() {
        let reason = if base == head {
            "Those two commits are the same. Uncommitted edits need --head :worktree.".to_string()
        } else {
            "No supported source files changed.".to_string()
        };
        return (Headline::LateralMove, reason);
    }
    // A new file has no before-medal, so it cannot make an existing file's
    // lateral move into an improvement, and it cannot hide one either.
    let existing: Vec<&FileRecap> = files.iter().filter(|file| !file.is_new()).collect();
    let all: Vec<&FileRecap> = files.iter().collect();
    let judged: &[&FileRecap] = if existing.is_empty() { &all } else { &existing };
    // Worst measured file wins. A mixed change is not an improvement.
    let worst = judged
        .iter()
        .min_by_key(|file| file.status.rank())
        .expect("judged is non-empty");
    let best = judged
        .iter()
        .max_by_key(|file| file.status.rank())
        .expect("judged is non-empty");
    let unanimous = best.status.rank() == worst.status.rank();
    let headline = match worst.status {
        Headline::SuspiciousNoStructuralChange => Headline::SuspiciousNoStructuralChange,
        Headline::Regression => Headline::Regression,
        Headline::RegressionScore => Headline::RegressionScore,
        Headline::Improvement if unanimous => Headline::Improvement,
        Headline::ImprovementScore if unanimous => Headline::ImprovementScore,
        _ => Headline::LateralMove,
    };
    let reason = match headline {
        Headline::SuspiciousNoStructuralChange => format!(
            "{} moved its score while the syntax tree barely changed.",
            worst.path
        ),
        Headline::Regression => format!("{} lost a structural pillar.", worst.path),
        Headline::RegressionScore => {
            format!(
                "{} kept its medal, but a pillar score went down.",
                worst.path
            )
        }
        Headline::Improvement => format!(
            "{} cleared a structural pillar it missed before.",
            worst.path
        ),
        Headline::ImprovementScore => {
            format!("{} kept its medal and improved a pillar score.", worst.path)
        }
        Headline::LateralMove => lateral_reason(&existing),
    };
    (headline, reason)
}

/// A lateral move is a mixed or flat change. Say what actually happened to
/// the existing files instead of claiming they all held, which is false as
/// soon as one of them went up.
fn lateral_reason(existing: &[&FileRecap]) -> String {
    if existing.is_empty() {
        return "New files arrived; no existing file was compared.".to_string();
    }
    let up = existing
        .iter()
        .filter(|file| {
            matches!(
                file.status,
                Headline::Improvement | Headline::ImprovementScore
            )
        })
        .count();
    let held = existing.len() - up;
    if up == 0 {
        return "Existing files kept their medals.".to_string();
    }
    format!(
        "{up} existing file{} improved, {held} held; no pillar was lost.",
        if up == 1 { "" } else { "s" }
    )
}

fn top_hotspots(files: &[FileRecap]) -> Vec<Hotspot> {
    let mut ranked: Vec<&Hotspot> = files.iter().flat_map(|f| f.hotspots.iter()).collect();
    ranked.sort_by_key(|spot| hotspot_rank(&spot.metric));
    ranked.into_iter().take(HOTSPOT_CAP).cloned().collect()
}

fn hotspot_rank(metric: &str) -> u8 {
    match metric {
        "cpg.dangerous_calls" => 0,
        "ast.max_function_complexity" => 1,
        "nav.max_function_divergence" => 2,
        "mdg.fan_out" => 3,
        _ => 4,
    }
}

// --- Hotspots ----------------------------------------------------------

fn file_hotspots(
    path: &str,
    before_src: &str,
    source: &str,
    language: &str,
    before: &ClassificationResult,
    after: &ClassificationResult,
) -> Vec<Hotspot> {
    let mut hotspots = Vec::new();
    if before_src.is_empty() {
        return hotspots;
    }
    let morphism = ProgramMorphism::with_path(source, language, path);
    if let Some(ast) = morphism.ast.as_ref().filter(|_| morphism.is_valid()) {
        if metric_worsened(before, after, "ast.max_function_complexity")
            && after
                .raw_metrics
                .get("ast.max_function_complexity")
                .is_some_and(|v| *v > SIMPLE.max_function_complexity)
        {
            if let Some(worst) = calculate_function_complexity_entries(&ast.uast_root, source)
                .into_iter()
                .max_by(|a, b| a.complexity.cmp(&b.complexity))
            {
                hotspots.push(Hotspot {
                    path: path.to_string(),
                    line: worst.start_line,
                    metric: "ast.max_function_complexity".to_string(),
                    detail: format!(
                        "{} complexity is {}, gate is {}",
                        worst.name, worst.complexity, SIMPLE.max_function_complexity as i64
                    ),
                    advice: "Extract a decision or a helper so this function clears the gate."
                        .to_string(),
                });
            }
        }
        if metric_worsened(before, after, "nav.max_function_divergence")
            && after
                .raw_metrics
                .get("nav.max_function_divergence")
                .is_some_and(|v| *v > NAVIGABLE.max_function_divergence)
        {
            if let Some(worst) = calculate_function_divergence_entries(&ast.uast_root, source)
                .into_iter()
                .max_by(|a, b| a.divergence.total_cmp(&b.divergence))
            {
                hotspots.push(Hotspot {
                    path: path.to_string(),
                    line: worst.start_line,
                    metric: "nav.max_function_divergence".to_string(),
                    detail: format!(
                        "{} nesting divergence is {:.1}, gate is {}",
                        worst.name, worst.divergence, NAVIGABLE.max_function_divergence as i64
                    ),
                    advice: "Lift the deepest nested block into a named function.".to_string(),
                });
            }
        }
    }
    if metric_worsened(before, after, "cpg.dangerous_calls")
        && after
            .raw_metrics
            .get("cpg.dangerous_calls")
            .is_some_and(|v| *v > 0.0)
    {
        if let Some(finding) = new_security_finding(before_src, source, language, path, after) {
            let (advice, _) = remediation_for(&finding);
            hotspots.insert(
                0,
                Hotspot {
                    path: path.to_string(),
                    line: finding.line as usize,
                    metric: "cpg.dangerous_calls".to_string(),
                    detail: format!(
                        "dangerous call {}",
                        finding.callee.as_deref().unwrap_or("unknown")
                    ),
                    advice,
                },
            );
        }
    }
    if metric_worsened(before, after, "mdg.fan_out")
        && after
            .raw_metrics
            .get("mdg.fan_out")
            .is_some_and(|v| *v > COMPOSABLE.max_fan_out)
    {
        hotspots.push(Hotspot {
            path: path.to_string(),
            line: 1,
            metric: "mdg.fan_out".to_string(),
            detail: format!(
                "fan-out is {:.0}, gate is {}",
                after.raw_metrics["mdg.fan_out"], COMPOSABLE.max_fan_out as i64
            ),
            advice: "Invert a dependency or split the module.".to_string(),
        });
    }
    hotspots
}

fn dangerous_calls(
    source: &str,
    language: &str,
    path: &str,
) -> Vec<topos_engine::evaluation::security_guidance::SecurityFinding> {
    let mut morphism = ProgramMorphism::with_path(source, language, path);
    let Some(cpg) = morphism.build_cpg() else {
        return Vec::new();
    };
    let mut nodes: Vec<_> = cpg.nodes.values().collect();
    nodes.sort_by_key(|node| (node.uast.span.start_line, node.uast.span.start_byte));
    let mut findings = Vec::new();
    let registry = topos_engine::functors::probes::cpg::danger::effective_registry(
        &cpg.language,
        &std::collections::HashSet::new(),
    );
    for node in nodes {
        if node.kind() != "CallExpr" {
            continue;
        }
        let owned = cpg.node_text(node);
        let text = owned.trim();
        if text.is_empty() {
            continue;
        }
        let callee = topos_engine::functors::probes::cpg::danger::callee_from_text(text);
        if callee.is_empty()
            || !topos_engine::functors::probes::cpg::danger::matches_registry(
                &callee,
                registry.iter().copied(),
            )
        {
            continue;
        }
        findings.push(
            topos_engine::evaluation::security_guidance::SecurityFinding {
                kind: "dangerous_call".to_string(),
                line: node.uast.span.start_line as u32,
                snippet: text.to_string(),
                callee: Some(callee),
                source: None,
                sink: None,
            },
        );
    }
    findings
}

fn new_security_finding(
    before_src: &str,
    after_src: &str,
    language: &str,
    path: &str,
    after: &ClassificationResult,
) -> Option<topos_engine::evaluation::security_guidance::SecurityFinding> {
    let before = classify_source(before_src, language, path, &CharacteristicMorphism, None);
    let calls_before = before
        .raw_metrics
        .get("cpg.dangerous_calls")
        .copied()
        .unwrap_or(0.0);
    let calls_after = after
        .raw_metrics
        .get("cpg.dangerous_calls")
        .copied()
        .unwrap_or(0.0);
    // A line split can make one call look like two snippets. If the scored
    // count did not rise, there is no new finding to point at. A new file
    // has no before-count, so compare its text with the empty baseline.
    if !before_src.is_empty() && calls_after <= calls_before {
        return None;
    }
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for finding in dangerous_calls(before_src, language, path) {
        *seen
            .entry(finding.callee.unwrap_or(finding.snippet))
            .or_insert(0) += 1;
    }
    dangerous_calls(after_src, language, path)
        .into_iter()
        .find(|finding| {
            match seen.get_mut(&finding.callee.clone().unwrap_or(finding.snippet.clone())) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    false
                }
                _ => true,
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALPHA: &str = "def alpha(x):\n    if x:\n        return 1\n    return 0\n";
    const BETA: &str = "def beta(x):\n    if x:\n        return 2\n    return 0\n";
    const GAMMA: &str = "def gamma(x):\n    if x:\n        return 3\n    return 0\n";

    fn no_coupling() -> CouplingStatus {
        unmeasured_coupling(None, false)
    }

    fn recap(repo: &Path, base: &str, head: &str, max_files: usize) -> PrRecap {
        build_recap(repo, base, head, max_files, None, no_coupling()).unwrap()
    }

    fn write_repo(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
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

    fn write_files(repo: &Path, files: &[(&str, &str)]) {
        for (path, body) in files {
            let full = repo.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, body).unwrap();
        }
    }

    fn commit_all(repo: &Path, message: &str) {
        git(repo, &["add", "."]).unwrap();
        git(repo, &["commit", "-qm", message]).unwrap();
    }

    #[test]
    fn empty_diff_is_a_lateral_move() {
        let (_keep, repo) = write_repo(&[("src/a.py", "def ready():\n    return 1\n")]);
        let recap = recap(&repo, "HEAD", "HEAD", 40);
        assert_eq!(recap.headline, Headline::LateralMove);
        assert!(recap.reason.contains("same"));
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
        assert_eq!(recap.headline, Headline::Regression);
        assert_eq!(recap.check, "fail");
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
        assert!(recap.reason.contains("lost a structural pillar"));
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

    #[test]
    fn refuses_a_revision_that_looks_like_an_option() {
        let (_keep, repo) = write_repo(&[("src/a.py", "x = 1\n")]);
        let err =
            build_recap(&repo, "--output=/tmp/x", "HEAD", 40, None, no_coupling()).unwrap_err();
        assert!(err.contains("refusing"));
    }

    #[test]
    fn missing_gh_names_the_workaround() {
        let message = missing_gh(12);
        assert!(message.contains("gh is not installed"));
        assert!(message.contains("brew install gh"));
        assert!(message.contains("--base <base-sha> --head <head-sha>"));
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
    fn format_falls_back_to_compact_off_a_terminal() {
        assert_eq!(resolve_format(false, None, true), RecapFormat::Card);
        assert_eq!(resolve_format(false, None, false), RecapFormat::Compact);
        assert_eq!(resolve_format(true, None, true), RecapFormat::Compact);
        assert_eq!(
            resolve_format(false, Some(RecapFormat::Github), false),
            RecapFormat::Github
        );
    }

    /// A split parent's fan-out rises only because it imports its own
    /// children: COMPOSABLE must be set aside for the verdict too, not
    /// only for the score deltas.
    #[test]
    fn a_routed_fan_out_does_not_read_as_a_regression() {
        fn side(composable: bool) -> ClassificationResult {
            let mut result = ClassificationResult {
                is_parseable: true,
                ..Default::default()
            };
            for generator in Generator::ALL {
                let key = generator.as_str().to_string();
                let passing = generator.as_str() != "composable" || composable;
                result.dimensions.insert(
                    key.clone(),
                    if passing {
                        generator.value()
                    } else {
                        EvaluationValue::Slop
                    },
                );
                result.scores.insert(key, if passing { 1.0 } else { 0.0 });
            }
            result.raw_metrics.insert("mdg.fan_out".to_string(), 1.0);
            result.raw_metrics.insert("cfg.cyclomatic".to_string(), 1.0);
            result
                .raw_metrics
                .insert("cpg.dangerous_calls".to_string(), 0.0);
            result
                .raw_metrics
                .insert("nav.max_function_divergence".to_string(), 0.0);
            result
        }
        let (before, after) = (side(true), side(false));
        let lattice = Omega::default();
        let status = |drop_composable: bool| {
            file_status(
                &before,
                &after,
                measured_verdict_excluding(&before, drop_composable),
                measured_verdict_excluding(&after, drop_composable),
                false,
                &score_deltas(&before, &after, drop_composable),
                &lattice,
                false,
            )
        };
        assert_eq!(status(false), Headline::Regression);
        assert_ne!(status(true), Headline::Regression);
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

    /// A brand-new module the parent merely starts importing is not an
    /// extraction: RefDiff's rule needs moved code, not just a call edge.
    #[test]
    fn a_new_module_the_parent_merely_uses_is_not_a_split() {
        let (_keep, repo) = write_repo(&[(
            "src/app.py",
            "def run(x):\n    if x:\n        return 1\n    return 0\n",
        )]);
        write_files(
            &repo,
            &[
                (
                    "src/app.py",
                    "from util import helper\n\ndef run(x):\n    if x:\n        return helper(x)\n    return 0\n",
                ),
                (
                    "src/util.py",
                    "def helper(x):\n    if x > 1:\n        return 2\n    return 3\n",
                ),
            ],
        );
        commit_all(&repo, "use a new module");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);

        assert!(
            recap.clusters.is_empty(),
            "an import edge alone is not a split: {:?}",
            recap.clusters
        );
        for file in &recap.files {
            assert!(
                file.cluster.is_none(),
                "{} must not be clustered",
                file.path
            );
        }
        let child = recap
            .files
            .iter()
            .find(|f| f.path == "src/util.py")
            .expect("new file scored");
        assert_eq!(child.change, FileChange::Added);
    }

    /// Only a *nested* callable moved into the child — the top-level
    /// wrapper there is new. Nested moves are still moved code, so the
    /// child stays in the cluster and must not render with `moved_in 0`.
    #[test]
    fn a_child_kept_alive_by_moved_callbacks() {
        let (_keep, repo) = write_repo(&[(
            "src/app.py",
            "def run(x):\n    def check(y):\n        if y > 1:\n            return 2\n        return 3\n    return check(x)\n",
        )]);
        write_files(
            &repo,
            &[
                (
                    "src/app.py",
                    "from worker import work\n\ndef run(x):\n    return work(x)\n",
                ),
                (
                    "src/worker.py",
                    "def work(x):\n    def check(y):\n        if y > 1:\n            return 2\n        return 3\n    return check(x)\n",
                ),
            ],
        );
        commit_all(&repo, "extract");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);

        assert_eq!(recap.clusters.len(), 1, "the nested move is evidence");
        let child = &recap.clusters[0].children[0];
        assert_eq!(child.path, "src/worker.py");
        assert_eq!(child.moved_in, 1, "the moved closure counts");
    }

    #[test]
    fn split_is_clustered_without_stores() {
        let big = format!("{ALPHA}\n\n{BETA}\n\n{GAMMA}");
        let (_keep, repo) = write_repo(&[("src/big.py", big.as_str())]);
        write_files(
            &repo,
            &[
                (
                    "src/big.py",
                    &format!("from helpers import beta, gamma\n\n\n{ALPHA}"),
                ),
                ("src/helpers.py", &format!("{BETA}\n\n{GAMMA}")),
            ],
        );
        commit_all(&repo, "split");
        let recap = recap(&repo, "HEAD~1", "HEAD", 40);

        assert_eq!(recap.clusters.len(), 1, "one cluster");
        let cluster = &recap.clusters[0];
        assert_eq!(cluster.parent, "src/big.py");
        assert_eq!(cluster.children.len(), 1);
        assert_eq!(cluster.children[0].path, "src/helpers.py");
        assert!(cluster.children[0].reach.is_none(), "no stores, no reach");
        let ledger = cluster.ledger.as_ref().expect("both sides parse");
        assert_eq!(ledger.totals.moved_identical, 2);
        assert_eq!(cluster.children[0].moved_in, 2);
        assert_eq!(
            cluster.decisions_before, cluster.decisions_after,
            "a pure move adds no decisions"
        );
        assert_eq!(cluster.mark, ClusterMark::Ok);

        let parent = recap
            .files
            .iter()
            .find(|f| f.path == "src/big.py")
            .expect("parent scored");
        assert_eq!(
            parent.cluster.as_ref().map(|c| c.role),
            Some(ClusterRole::Parent)
        );
        let child = recap
            .files
            .iter()
            .find(|f| f.path == "src/helpers.py")
            .expect("child scored");
        assert_eq!(
            child.cluster.as_ref().map(|c| c.role),
            Some(ClusterRole::Child)
        );
        assert_eq!(
            child.cluster.as_ref().map(|c| c.parent.as_str()),
            Some("src/big.py")
        );

        let json = serde_json::to_value(&recap).expect("recap serializes");
        assert_eq!(json["clusters"][0]["parent"], "src/big.py");
        assert_eq!(json["schema"], SCHEMA);
    }
}
