//! `topos pr-recap` — structural before/after for a git range.
//!
//! Scores added and modified source files at `--base` and `--head`. The
//! headline is computed here, from the lattice, so a later formatter cannot
//! invent a medal. Deleted files are listed, not scored. Module coupling is
//! not generated: when no dependency graph is attached, COMPOSABLE is
//! reported as not measured.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;
use serde::Serialize;
use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::morphism::ProgramMorphism;
use topos_engine::core::omega::{EvaluationValue, Generator, Omega};
use topos_engine::evaluation::policies::base::Priority;
use topos_engine::evaluation::policies::calibration::{COMPOSABLE, NAVIGABLE, SIMPLE};
use topos_engine::evaluation::policies::gates::pillar_for_metric;
use topos_engine::evaluation::security_guidance::remediation_for;
use topos_engine::functors::probes::ast::complexity::calculate_function_complexity_entries;
use topos_engine::functors::probes::ast::divergence::calculate_function_divergence_entries;
use topos_engine::functors::profunctors::ast::compare::calculate_ast_distance;
use topos_engine::graphs::ast::languages::all_source_suffixes;

use super::classify::classify_with_representations;
use super::lang::detect_language;
use crate::commands::render::{guide, guide_line, paint, RenderOptions, Working};
use console::Style;

/// Score movement this large, with almost no syntax-tree change, is cosmetic.
const MEANINGFUL_SCORE_DELTA: f64 = 0.03;
/// Below this, a score dip is noise (0.1 on the displayed 0–100 scale).
const SCORE_REGRESSION_FLOOR: f64 = 0.001;
const STRUCTURAL_CHANGE_THRESHOLD: f64 = 0.02;
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

#[derive(Args)]
pub struct PrRecapArgs {
    /// Review this pull request against the branch it merges into.
    /// Mutually exclusive with `--base` and `--head`.
    #[arg(value_name = "PR")]
    pub pr: Option<u64>,
    /// Git commit the change starts from (the pull request base).
    #[arg(long)]
    pub base: Option<String>,
    /// Git commit the change ends at. Defaults to `HEAD`.
    /// Use `--head :worktree` to include uncommitted edits.
    #[arg(long)]
    pub head: Option<String>,
    /// Repository to read. Defaults to the current directory.
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Emit the machine-readable document instead of the review card.
    #[arg(long)]
    pub json: bool,
    /// Do not score more than this many added or modified files.
    #[arg(long, default_value_t = DEFAULT_FILE_CAP)]
    pub max_files: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Headline {
    SuspiciousNoStructuralChange,
    Regression,
    RegressionScore,
    Improvement,
    ImprovementScore,
    LateralMove,
}

impl Headline {
    fn as_str(self) -> &'static str {
        match self {
            Headline::SuspiciousNoStructuralChange => "SUSPICIOUS_NO_STRUCTURAL_CHANGE",
            Headline::Regression => "REGRESSION",
            Headline::RegressionScore => "REGRESSION_SCORE",
            Headline::Improvement => "IMPROVEMENT",
            Headline::ImprovementScore => "IMPROVEMENT_SCORE",
            Headline::LateralMove => "LATERAL_MOVE",
        }
    }

    fn fails_check(self) -> bool {
        matches!(
            self,
            Headline::SuspiciousNoStructuralChange
                | Headline::Regression
                | Headline::RegressionScore
        )
    }
}

#[derive(Debug, Clone, Serialize)]
struct PillarDelta {
    measured: bool,
    before_passed: Option<bool>,
    after_passed: Option<bool>,
    before_score: Option<f64>,
    after_score: Option<f64>,
    /// The gate that failed, when a previously passing pillar no longer does.
    #[serde(skip_serializing_if = "Option::is_none")]
    lost_gate: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct Hotspot {
    path: String,
    line: usize,
    metric: String,
    detail: String,
    advice: String,
}

#[derive(Debug, Clone, Serialize)]
struct FileRecap {
    path: String,
    status: String,
    lines_added: usize,
    lines_removed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    medal_before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    medal_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verdict_before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verdict_after: Option<String>,
    pillars: BTreeMap<String, PillarDelta>,
    structural_distance: Option<f64>,
    #[serde(skip_serializing_if = "is_false")]
    complexity_relocated_within_file: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    hotspots: Vec<Hotspot>,
}

#[derive(Debug, Clone, Serialize)]
struct SkippedFile {
    path: String,
    reason: String,
}

#[derive(Debug, Clone, Serialize)]
struct Scope {
    files_scored: usize,
    lines_added: usize,
    lines_removed: usize,
    files_skipped: usize,
    files_deleted: usize,
    files_capped: usize,
    coupling_available: bool,
    note: String,
}

#[derive(Debug, Clone, Serialize)]
struct PrRecap {
    schema: &'static str,
    base: String,
    head: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    review: Option<PullRequest>,
    headline: Headline,
    check: &'static str,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    scope: Scope,
    files: Vec<FileRecap>,
    skipped: Vec<SkippedFile>,
    deleted: Vec<String>,
    hotspots: Vec<Hotspot>,
    /// Structural direction is not proof that behavior is unchanged.
    non_claim: &'static str,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Clone, Serialize)]
struct PullRequest {
    number: u64,
    head_ref: String,
    base_ref: String,
}

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

pub fn run(args: PrRecapArgs) -> Result<(), String> {
    let repo = args
        .repo
        .clone()
        .unwrap_or(std::env::current_dir().map_err(|e| format!("current directory: {e}"))?);
    let (base, head, review) = resolve_range(&repo, args.pr, args.base, args.head)?;
    let working = (!args.json).then(Working::start);
    let mut recap = build_recap(&repo, &base, &head, args.max_files)?;
    if let Some(working) = working {
        working.clear();
    }
    recap.review = review;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&recap).map_err(|e| format!("serializing recap: {e}"))?
        );
    } else {
        print_recap(&recap);
    }
    if recap.headline.fails_check() && recap.error.is_none() {
        std::process::exit(1);
    }
    if recap.error.is_some() {
        std::process::exit(2);
    }
    Ok(())
}

fn build_recap(repo: &Path, base: &str, head: &str, max_files: usize) -> Result<PrRecap, String> {
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

    let classifier = CharacteristicMorphism;
    let lattice = Omega::default();
    let mut files = Vec::new();
    for entry in &scoreable {
        files.push(score_file(
            &repo,
            &base_sha,
            &head_sha,
            entry,
            &classifier,
            &lattice,
        )?);
    }

    let (headline, reason) = headline_for(&files, &base_sha, &head_sha);
    let hotspots = top_hotspots(&files);
    let coupling_available = false;
    let scope = Scope {
        files_scored: files.len(),
        lines_added: files.iter().map(|file| file.lines_added).sum(),
        lines_removed: files.iter().map(|file| file.lines_removed).sum(),
        files_skipped: skipped.len(),
        files_deleted: diff.deleted.len(),
        files_capped: capped,
        coupling_available,
        note: format!(
            "scored {} changed file{}; module coupling was not measured",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        ),
    };
    Ok(PrRecap {
        schema: "topos.pr_recap.v1",
        base: base_sha,
        head: head_sha,
        headline,
        check: if headline.fails_check() {
            "fail"
        } else {
            "pass"
        },
        reason,
        review: None,
        error: None,
        scope,
        files,
        skipped,
        deleted: diff.deleted,
        hotspots,
        non_claim: "Structural direction is not proof that tests or behavior still pass.",
    })
}

struct DiffEntry {
    status: String,
    path: String,
}

struct Diff {
    entries: Vec<DiffEntry>,
    deleted: Vec<String>,
}

fn git_root(start: &Path) -> Result<PathBuf, String> {
    let output = git(start, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(output.trim()))
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
            entries.push(DiffEntry { status, path });
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

fn score_file(
    repo: &Path,
    base: &str,
    head: &str,
    entry: &DiffEntry,
    classifier: &CharacteristicMorphism,
    lattice: &Omega,
) -> Result<FileRecap, String> {
    let language = detect_language(Path::new(&entry.path));
    let is_new = entry.status.starts_with('A');
    let before_src = if is_new {
        String::new()
    } else {
        show_file(repo, base, &entry.path)?
    };
    let after_src = if head == "worktree" {
        std::fs::read_to_string(repo.join(&entry.path))
            .map_err(|e| format!("reading {}: {e}", entry.path))?
    } else {
        show_file(repo, head, &entry.path)?
    };
    let before = classify_source(&before_src, &language, classifier);
    let after = classify_source(&after_src, &language, classifier);
    let distance = structural_distance(&before_src, &after_src, &language);
    let before_verdict = measured_verdict(&before);
    let after_verdict = measured_verdict(&after);
    let status = file_status(
        &before,
        &after,
        before_verdict,
        after_verdict,
        distance,
        lattice,
        is_new,
    );
    let hotspots = file_hotspots(
        &entry.path,
        &before_src,
        &after_src,
        &language,
        &before,
        &after,
    );
    let (lines_added, lines_removed) = line_delta(&before_src, &after_src);
    Ok(FileRecap {
        path: entry.path.clone(),
        status: status.as_str().to_string(),
        lines_added,
        lines_removed,
        medal_before: (!is_new).then(|| medal_label(before_verdict)),
        medal_after: Some(medal_label(after_verdict)),
        verdict_before: (!is_new).then(|| before_verdict.name().to_string()),
        verdict_after: Some(after_verdict.name().to_string()),
        pillars: pillar_deltas(&before, &after, is_new),
        structural_distance: distance,
        complexity_relocated_within_file: complexity_relocated(&before, &after),
        hotspots,
    })
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

fn show_file(repo: &Path, rev: &str, path: &str) -> Result<String, String> {
    git(
        repo,
        &["show", "--end-of-options", &format!("{rev}:{path}")],
    )
    .map_err(|_| format!("could not read {path} at {rev}"))
}

fn classify_source(
    source: &str,
    language: &str,
    classifier: &CharacteristicMorphism,
) -> ClassificationResult {
    let mut morphism = ProgramMorphism::new(source, language);
    classify_with_representations(classifier, &mut morphism, None, Priority::Secure)
}

fn structural_distance(before: &str, after: &str, language: &str) -> Option<f64> {
    if before.is_empty() {
        return None;
    }
    let base = ProgramMorphism::new(before, language);
    let proposed = ProgramMorphism::new(after, language);
    match (base.ast.as_ref(), proposed.ast.as_ref()) {
        (Some(base_ast), Some(proposed_ast)) if base.is_valid() && proposed.is_valid() => {
            Some(calculate_ast_distance(base_ast, proposed_ast).normalized_distance)
        }
        _ => None,
    }
}

fn file_status(
    before: &ClassificationResult,
    after: &ClassificationResult,
    before_verdict: EvaluationValue,
    after_verdict: EvaluationValue,
    distance: Option<f64>,
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
    let score_deltas = score_deltas(before, after);
    let suspicious = distance.is_some_and(|d| d < STRUCTURAL_CHANGE_THRESHOLD)
        && score_deltas
            .iter()
            .any(|d| d.abs() >= MEANINGFUL_SCORE_DELTA);
    if before_verdict == after_verdict {
        let improved = score_deltas.iter().any(|d| *d >= SCORE_REGRESSION_FLOOR);
        let regressed = score_deltas.iter().any(|d| *d <= -SCORE_REGRESSION_FLOOR);
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

fn score_deltas(before: &ClassificationResult, after: &ClassificationResult) -> Vec<f64> {
    Generator::ALL
        .into_iter()
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
    // needs the dependency graph, which this command does not attach.
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
    let satisfied: Vec<Generator> = Generator::ALL
        .into_iter()
        .filter(|generator| pillar_passed(result, *generator))
        .collect();
    topos_engine::core::omega::verdict_from_generators(&satisfied)
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

fn medal_label(value: EvaluationValue) -> String {
    format!("{} {}", value.symbol(), value.medal_tier())
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
    let morphism = ProgramMorphism::new(source, language);
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
        if let Some(finding) = new_security_finding(before_src, source, language, after) {
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
) -> Vec<topos_engine::evaluation::security_guidance::SecurityFinding> {
    let mut morphism = ProgramMorphism::new(source, language);
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
    after: &ClassificationResult,
) -> Option<topos_engine::evaluation::security_guidance::SecurityFinding> {
    let before = classify_source(before_src, language, &CharacteristicMorphism);
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
    for finding in dangerous_calls(before_src, language) {
        *seen
            .entry(finding.callee.unwrap_or(finding.snippet))
            .or_insert(0) += 1;
    }
    dangerous_calls(after_src, language)
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

fn headline_for(files: &[FileRecap], base: &str, head: &str) -> (Headline, String) {
    if files.is_empty() {
        let reason = if base == head {
            "Those two commits are the same. Uncommitted edits need --head :worktree.".to_string()
        } else {
            "No supported source files changed.".to_string()
        };
        return (Headline::LateralMove, reason);
    }
    // Worst measured file wins. A mixed change is not an improvement.
    let rank = |status: &str| match status {
        "SUSPICIOUS_NO_STRUCTURAL_CHANGE" => 0,
        "REGRESSION" => 1,
        "REGRESSION_SCORE" => 2,
        "LATERAL_MOVE" => 3,
        "IMPROVEMENT_SCORE" => 4,
        "IMPROVEMENT" => 5,
        _ => 3,
    };
    // A new file has no before-medal, so it cannot make an existing file's
    // lateral move into an improvement, and it cannot hide one either.
    let existing: Vec<&FileRecap> = files
        .iter()
        .filter(|file| file.medal_before.is_some())
        .collect();
    let all: Vec<&FileRecap> = files.iter().collect();
    let judged: &[&FileRecap] = if existing.is_empty() { &all } else { &existing };
    let worst = judged
        .iter()
        .min_by_key(|file| rank(&file.status))
        .expect("judged is non-empty");
    let best = judged
        .iter()
        .max_by_key(|file| rank(&file.status))
        .expect("judged is non-empty");
    let headline = match worst.status.as_str() {
        "SUSPICIOUS_NO_STRUCTURAL_CHANGE" => Headline::SuspiciousNoStructuralChange,
        "REGRESSION" => Headline::Regression,
        "REGRESSION_SCORE" => Headline::RegressionScore,
        "IMPROVEMENT" if rank(&best.status) == rank(&worst.status) => Headline::Improvement,
        "IMPROVEMENT_SCORE" if rank(&best.status) == rank(&worst.status) => {
            Headline::ImprovementScore
        }
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
        Headline::LateralMove => {
            if existing.is_empty() {
                "New files arrived; no existing file was compared.".to_string()
            } else {
                "Existing files kept their medals.".to_string()
            }
        }
    };
    (headline, reason)
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

fn render_recap(recap: &PrRecap, options: RenderOptions) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(paint(
        format!("◇  Reviewed {}", range_label(recap)),
        Style::new().bold(),
        options,
    ));
    lines.push(guide_line(context_line(recap), Style::new().dim(), options));
    lines.push(guide('│', options));
    lines.push(guide_line(
        format!(
            "{:<12}  {:<9}  {:>5}  {:>5}  FILE",
            "PILLAR", "CHANGE", "FROM", "TO"
        ),
        Style::new().bold().dim(),
        options,
    ));
    for (file, pillar) in change_rows(recap) {
        lines.push(change_row(file, pillar, options));
    }
    lines.push(guide('│', options));
    lines.push(floor_line(recap, options));
    if let Some((mark, style, note)) = new_files_line(recap) {
        lines.push(format!(
            "{}  {} {}",
            guide(' ', options),
            paint(mark, style.clone(), options),
            paint(note, style, options),
        ));
    }
    if let Some(note) = relocated_note(recap) {
        lines.push(guide_line(note, Style::new().dim(), options));
    }
    if let Some(note) = split_note(recap) {
        lines.push(guide_line(note, Style::new().dim(), options));
    }
    if recap
        .files
        .iter()
        .any(|file| file.status == "REGRESSION_SCORE")
        && recap.headline != Headline::RegressionScore
    {
        lines.push(guide_line(
            "Score-only dips are in --json; they did not move a medal.",
            Style::new().dim(),
            options,
        ));
    }
    if !recap.deleted.is_empty() {
        lines.push(guide_line(
            format!("Deleted, not scored: {}", recap.deleted.join(", ")),
            Style::new().dim(),
            options,
        ));
    }
    if !recap.hotspots.is_empty() {
        lines.push(String::new());
        lines.push(guide_line("Where to look", Style::new().bold(), options));
        for spot in &recap.hotspots {
            lines.push(guide_line(
                format!("{}:{}  {}", spot.path, spot.line, spot.detail),
                Style::new(),
                options,
            ));
            lines.push(guide_line(
                format!("  {}", spot.advice),
                Style::new().dim(),
                options,
            ));
        }
    }
    lines
}

fn range_label(recap: &PrRecap) -> String {
    let n = recap.scope.files_scored;
    format!(
        "{n} changed file{}  +{}/-{ }",
        if n == 1 { "" } else { "s" },
        recap.scope.lines_added,
        recap.scope.lines_removed,
    )
}

fn short_rev(rev: &str) -> &str {
    if rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return &rev[..7.min(rev.len())];
    }
    rev
}

fn context_line(recap: &PrRecap) -> String {
    let range = if let Some(review) = &recap.review {
        format!(
            "#{} {} → {}",
            review.number, review.head_ref, review.base_ref
        )
    } else {
        format!("{}…{}", short_rev(&recap.base), short_rev(&recap.head))
    };
    let coupling = if recap.scope.coupling_available {
        "COMPOSABLE measured"
    } else {
        "COMPOSABLE not measured. topos depgraph generate-pr <number>"
    };
    let mut line = format!("{range} · {coupling}");
    if recap.scope.files_skipped > 0 {
        line.push_str(&format!(" · {} skipped", recap.scope.files_skipped));
    }
    if recap.scope.files_capped > 0 {
        line.push_str(&format!(
            " · {} over the file cap",
            recap.scope.files_capped
        ));
    }
    line
}

fn change_rows(recap: &PrRecap) -> Vec<(&FileRecap, &str)> {
    let mut rows = Vec::new();
    for file in &recap.files {
        for pillar in Generator::ALL.map(Generator::as_str) {
            let Some(delta) = file.pillars.get(pillar) else {
                continue;
            };
            let lost = delta.before_passed == Some(true) && delta.after_passed == Some(false);
            let cleared = delta.before_passed == Some(false) && delta.after_passed == Some(true);
            let shift = delta
                .before_score
                .zip(delta.after_score)
                .map(|(before, after)| after - before);
            let moved = shift.is_some_and(|shift| shift.abs() >= 1.0);
            if lost || cleared || moved {
                rows.push((file, pillar));
            }
        }
    }
    rows
}

fn change_row(file: &FileRecap, pillar: &str, options: RenderOptions) -> String {
    let delta = &file.pillars[pillar];
    let lost = delta.before_passed == Some(true) && delta.after_passed == Some(false);
    let cleared = delta.before_passed == Some(false) && delta.after_passed == Some(true);
    let shift = delta
        .before_score
        .zip(delta.after_score)
        .map(|(before, after)| after - before)
        .unwrap_or(0.0);
    let (mark, label, style) = if lost {
        ("X", "LOST", Style::new().red().bold())
    } else if cleared {
        ("✓", "CLEARED", Style::new().green().bold())
    } else if shift <= -1.0 {
        ("!", "DOWN", Style::new().yellow().bold())
    } else {
        ("✓", "UP", Style::new().green().bold())
    };
    let before = score_cell(delta.before_score);
    let after = score_cell(delta.after_score);
    let gate = delta
        .lost_gate
        .as_deref()
        .map(|gate| format!("  {gate}"))
        .unwrap_or_default();
    format!(
        "{}  {:<12}  {}  {before}  {after}  {}{gate}",
        guide('│', options),
        pillar.to_ascii_uppercase(),
        paint(format!("{mark} {label:<7}"), style, options),
        file.path,
    )
}

fn score_cell(score: Option<f64>) -> String {
    score
        .map(|value| format!("{value:>4.0}%"))
        .unwrap_or_else(|| "   —".to_string())
}

fn floor_line(recap: &PrRecap, options: RenderOptions) -> String {
    let (mark, style, word) = match recap.headline {
        Headline::Regression => ("X", Style::new().red().bold(), "REGRESSION"),
        Headline::RegressionScore => ("!", Style::new().yellow().bold(), "SCORE DOWN"),
        Headline::SuspiciousNoStructuralChange => ("!", Style::new().yellow().bold(), "SUSPICIOUS"),
        Headline::Improvement | Headline::ImprovementScore => {
            ("✓", Style::new().green().bold(), recap.headline.as_str())
        }
        Headline::LateralMove => ("·", Style::new().dim(), "LATERAL"),
    };
    format!(
        "{}  {} {} · {}",
        guide('└', options),
        paint(mark, style.clone(), options),
        paint(word, style, options),
        recap.reason,
    )
}

fn new_files_line(recap: &PrRecap) -> Option<(&'static str, Style, String)> {
    let new_files: Vec<&FileRecap> = recap
        .files
        .iter()
        .filter(|file| file.medal_before.is_none())
        .collect();
    if new_files.is_empty() {
        return None;
    }
    let mut medals: BTreeMap<&str, usize> = BTreeMap::new();
    for file in &new_files {
        let medal = file
            .medal_after
            .as_deref()
            .and_then(|label| label.split_whitespace().nth(1))
            .unwrap_or("unscored");
        *medals.entry(medal).or_insert(0) += 1;
    }
    let order = ["SLOP", "BRONZE", "SILVER", "GOLD", "PLATINUM"];
    let mut counted: Vec<(&str, usize)> = medals.into_iter().collect();
    counted.sort_by_key(|(medal, _)| order.iter().position(|tier| tier == medal).unwrap_or(9));
    let counts = counted
        .iter()
        .map(|(medal, count)| format!("{count} {medal}"))
        .collect::<Vec<_>>()
        .join(", ");
    let slop = counted
        .iter()
        .any(|(medal, count)| *medal == "SLOP" && *count > 0);
    let (mark, style) = if slop {
        ("X", Style::new().red().bold())
    } else {
        ("+", Style::new().green().bold())
    };
    Some((
        mark,
        style,
        format!(
            "{} new file{}: {counts}",
            new_files.len(),
            if new_files.len() == 1 { "" } else { "s" }
        ),
    ))
}

fn split_note(recap: &PrRecap) -> Option<String> {
    let simpler: Vec<&str> = recap
        .files
        .iter()
        .filter(|file| file.medal_before.is_some())
        .filter(|file| {
            file.pillars.values().any(|delta| {
                delta
                    .before_score
                    .zip(delta.after_score)
                    .is_some_and(|(before, after)| after - before >= 1.0)
            })
        })
        .map(|file| file.path.as_str())
        .collect();
    let arrived: Vec<&str> = recap
        .files
        .iter()
        .filter(|file| file.medal_before.is_none())
        .map(|file| file.path.as_str())
        .collect();
    if simpler.is_empty() || arrived.is_empty() {
        return None;
    }
    Some(format!(
        "{} got simpler as {} new file{} arrived. This does not trace which function moved.",
        simpler.join(", "),
        arrived.len(),
        if arrived.len() == 1 { "" } else { "s" }
    ))
}

fn relocated_note(recap: &PrRecap) -> Option<String> {
    let paths: Vec<&str> = recap
        .files
        .iter()
        .filter(|file| file.complexity_relocated_within_file)
        .map(|file| file.path.as_str())
        .collect();
    if paths.is_empty() {
        return None;
    }
    Some(format!(
        "Complexity stayed inside {} rather than leaving it.",
        paths.join(", ")
    ))
}

fn print_recap(recap: &PrRecap) {
    for line in render_recap(recap, RenderOptions::stdout()) {
        println!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_repo(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().to_path_buf();
        git(&repo, &["init", "-q"]).expect("init");
        git(&repo, &["config", "user.email", "recap@example.com"]).unwrap();
        git(&repo, &["config", "user.name", "Recap"]).unwrap();
        for (path, body) in files {
            let full = repo.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, body).unwrap();
        }
        git(&repo, &["add", "."]).unwrap();
        git(&repo, &["commit", "-qm", "base"]).unwrap();
        (dir, repo)
    }

    fn commit_all(repo: &Path, message: &str) {
        git(repo, &["add", "."]).unwrap();
        git(repo, &["commit", "-qm", message]).unwrap();
    }

    #[test]
    fn empty_diff_is_a_lateral_move() {
        let (_keep, repo) = write_repo(&[("src/a.py", "def ready():\n    return 1\n")]);
        let recap = build_recap(&repo, "HEAD", "HEAD", 40).unwrap();
        assert_eq!(recap.headline, Headline::LateralMove);
        assert!(recap.reason.contains("same"));
        assert!(recap.files.is_empty());
        assert_eq!(recap.check, "pass");
    }

    #[test]
    fn added_source_is_scored_and_markdown_is_skipped() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/new.py"), "def ready():\n    return 1\n").unwrap();
        std::fs::write(repo.join("notes.md"), "not code\n").unwrap();
        commit_all(&repo, "add");
        let recap = build_recap(&repo, "HEAD~1", "HEAD", 40).unwrap();
        assert_eq!(recap.files.len(), 1);
        assert_eq!(recap.files[0].path, "src/new.py");
        assert!(recap.skipped.iter().any(|s| s.path == "notes.md"));
        assert!(!recap.scope.coupling_available);
        let card = render_recap(
            &recap,
            RenderOptions {
                styled: false,
                width: 100,
            },
        )
        .join("\n");
        assert!(card.contains("◇  Reviewed 1 changed file  +"));
        assert!(card.contains("1 new file"));
        assert!(!card.contains("Files that moved"));
        assert!(card.contains("COMPOSABLE not measured"));
        assert!(!card.contains("notes.md"));
        assert!(!card.contains("X  REGRESSION"));
    }

    #[test]
    fn deleted_file_is_listed_not_scored() {
        let (_keep, repo) = write_repo(&[("src/gone.py", "def ready():\n    return 1\n")]);
        std::fs::remove_file(repo.join("src/gone.py")).unwrap();
        commit_all(&repo, "delete");
        let recap = build_recap(&repo, "HEAD~1", "HEAD", 40).unwrap();
        assert!(recap.files.is_empty());
        assert_eq!(recap.deleted, vec!["src/gone.py".to_string()]);
    }

    #[test]
    fn a_dangerous_call_introduced_against_base_is_a_regression() {
        let (_keep, repo) = write_repo(&[("src/run.py", "def ready():\n    return 1\n")]);
        std::fs::write(
            repo.join("src/run.py"),
            "import os\n\ndef ready(cmd):\n    os.system(cmd)\n",
        )
        .unwrap();
        commit_all(&repo, "shell");
        let recap = build_recap(&repo, "HEAD~1", "HEAD", 40).unwrap();
        assert_eq!(recap.headline, Headline::Regression);
        assert_eq!(recap.check, "fail");
        assert!(recap
            .hotspots
            .iter()
            .any(|h| h.metric == "cpg.dangerous_calls"));
        let card = render_recap(
            &recap,
            RenderOptions {
                styled: false,
                width: 100,
            },
        )
        .join("\n");
        assert!(card.contains("SECURE") && card.contains("X LOST"));
        assert!(card.contains("src/run.py"));
        assert!(card.contains("X LOST") || card.contains("! DOWN"));
        assert!(!card.contains("+ GAIN"));
        assert!(!card.contains("· HELD"));
        assert!(card.contains("lost a structural pillar"));
        assert!(card.contains("os.system") || card.contains("dangerous call"));
        assert!(!card.contains("LATERAL_MOVE"));
        assert!(!card.contains("held  "));
        assert!(
            recap.files[0]
                .pillars
                .get("secure")
                .and_then(|p| p.after_passed)
                == Some(false)
        );
    }

    #[test]
    fn cap_skips_the_overflow_instead_of_hiding_it() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        std::fs::create_dir_all(repo.join("src")).unwrap();
        for i in 0..3 {
            std::fs::write(repo.join(format!("src/f{i}.py")), "x = 1\n").unwrap();
        }
        commit_all(&repo, "three");
        let recap = build_recap(&repo, "HEAD~1", "HEAD", 1).unwrap();
        assert_eq!(recap.files.len(), 1);
        assert_eq!(recap.scope.files_capped, 2);
        assert_eq!(recap.skipped.len(), 2);
    }

    #[test]
    fn refuses_a_revision_that_looks_like_an_option() {
        let (_keep, repo) = write_repo(&[("src/a.py", "x = 1\n")]);
        let err = build_recap(&repo, "--output=/tmp/x", "HEAD", 40).unwrap_err();
        assert!(err.contains("refusing"));
    }

    #[test]
    fn missing_gh_names_the_workaround() {
        let message = missing_gh(12);
        assert!(message.contains("gh is not installed"));
        assert!(message.contains("brew install gh"));
        assert!(message.contains("--base <base-sha> --head <head-sha>"));
    }
}
