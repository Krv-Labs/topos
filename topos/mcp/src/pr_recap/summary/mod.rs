//! `topos.pr_recap.summary.v1`: the MCP default shape of a recap.
//!
//! The full `topos.pr_recap.v3` document runs to tens of thousands of
//! characters, mostly per-file records. This is a distinct struct built from
//! the computed [`PrRecap`]: the verdict, the blocking evidence with handles
//! (path, line, function, metric, before, after, limit) and one ranking of
//! the files that need attention. Everything left out is counted in
//! `omitted`, with the parameter that widens it in `more`. It states facts;
//! per-finding `fix` text stays in the full document, and `metrics` says
//! what each metric shown measures. Paths are absolute under `root`.

mod attention;
mod paths;
mod text;
mod types;

use std::collections::BTreeSet;
use std::path::Path;

use topos_engine::config::Severity;

use super::gates::Finding;
use super::model::{AddedRollup, PrRecap, ProjectRollup};
use super::progress::Timing;
use super::verdict::worst_file;
use crate::shaping::{bound, describe_metrics, round_apart, sig3};

pub use paths::{resolve_paths, NotFound, Resolved};
pub use text::not_found_block;
pub use types::{
    AttentionFile, AttentionSpot, Omitted, Summary, SummaryFinding, SummaryGate, SummaryProject,
    SummaryScope,
};

use attention::rank;
use paths::Absolute;

pub const SUMMARY_SCHEMA: &str = "topos.pr_recap.summary.v1";

/// Findings listed in the summary: every block, then warnings up to this
/// total. Blocks are never dropped past it.
pub const MAX_FINDINGS: usize = 10;

/// How to widen what the summary left out.
const MORE: &str = "detail: \"full\" returns the whole topos.pr_recap.v3 document; paths: [..] \
                    adds the full records for the named files.";

/// The summary of `recap`, with the v3 records of the `requested` paths.
/// `root` is the absolute repository root the reported paths are under.
pub fn build(recap: &PrRecap, root: &Path, requested: &[String], timing: Timing) -> Summary {
    let absolute = Absolute::new(root);
    let counting = counting_findings(recap);
    let shown = shown_findings(&counting);
    let resolved = resolve_paths(recap, root, requested);
    let mut attention = rank(recap, &counting);
    let metrics = metrics_shown(recap, &shown, &attention, &resolved);
    let omitted = omitted(recap, &shown, &attention, &resolved.found);
    for file in &mut attention {
        file.path = absolute.path(&file.path);
    }
    let worst = worst_file(&recap.files);
    Summary {
        schema: SUMMARY_SCHEMA,
        root: root.display().to_string(),
        base: recap.base.clone(),
        head: recap.head.clone(),
        priority: recap.priority,
        readiness: recap.readiness,
        check: recap.check,
        exit_code: recap.exit_code,
        reason: recap.reason.clone(),
        incomplete: recap.incomplete,
        gate: gate_of(recap),
        worst_file_direction: worst.map(|_| recap.direction),
        worst_file: worst.map(|file| absolute.path(&file.path)),
        scope: scope_of(recap, timing),
        project: recap.project.as_ref().map(project_of),
        added: recap.added.as_ref().map(rounded_added),
        findings: shown.iter().map(|f| slim(f, &absolute)).collect(),
        findings_total: findings_total(recap, &counting),
        attention,
        metrics,
        more: (!omitted.is_empty()).then(|| MORE.to_string()),
        omitted,
        paths_not_found: resolved.missing,
        non_claim: recap.non_claim,
        files: resolved
            .found
            .iter()
            .map(|&index| absolute.record(&recap.files[index]))
            .collect(),
    }
}

/// The findings that count toward readiness, with their rank.
fn counting_findings(recap: &PrRecap) -> Vec<(usize, &Finding)> {
    let counts = |(_, finding): &(usize, &Finding)| finding.counts();
    recap.findings.iter().enumerate().filter(counts).collect()
}

/// Every block, then warnings until [`MAX_FINDINGS`] are listed.
fn shown_findings<'a>(counting: &[(usize, &'a Finding)]) -> Vec<&'a Finding> {
    let blocks = counting
        .iter()
        .filter(|(_, finding)| finding.severity == Severity::Block)
        .count();
    let mut room = MAX_FINDINGS.saturating_sub(blocks);
    let mut shown = Vec::new();
    for (_, finding) in counting {
        match finding.severity {
            Severity::Block => shown.push(*finding),
            Severity::Warn if room > 0 => {
                room -= 1;
                shown.push(*finding);
            }
            _ => {}
        }
    }
    shown
}

fn findings_total(
    recap: &PrRecap,
    counting: &[(usize, &Finding)],
) -> std::collections::BTreeMap<&'static str, usize> {
    let mut total = std::collections::BTreeMap::new();
    for (_, finding) in counting {
        *total.entry(finding.severity.as_str()).or_default() += 1;
    }
    let waived = recap.findings.len() - counting.len();
    if waived > 0 {
        total.insert("waived", waived);
    }
    total
}

fn gate_of(recap: &PrRecap) -> SummaryGate {
    SummaryGate {
        preset: recap.gate.preset,
        fail_on: recap.gate.fail_on,
        changes: recap.gate.changes,
        source: recap.gate.source.clone(),
    }
}

fn scope_of(recap: &PrRecap, timing: Timing) -> SummaryScope {
    SummaryScope {
        counts: recap.scope.clone(),
        elapsed_ms: timing.elapsed_ms,
        scoring_ms: timing.scoring_ms,
    }
}

fn project_of(project: &ProjectRollup) -> SummaryProject {
    let mut pillars = project.pillars.clone();
    for pillar in pillars.values_mut() {
        pillar.before_score = sig3(pillar.before_score);
        pillar.after_score = sig3(pillar.after_score);
    }
    SummaryProject {
        medal_before: project.medal_before.clone(),
        medal_after: project.medal_after.clone(),
        pillars,
        rollup_lost_pillar: project.regression,
        files_before: project.files_before,
        files_after: project.files_after,
        decisions_before: project.decisions_before,
        decisions_after: project.decisions_after,
    }
}

fn rounded_added(added: &AddedRollup) -> AddedRollup {
    let mut added = added.clone();
    for pillar in added.pillars.values_mut() {
        pillar.score = sig3(pillar.score);
    }
    added
}

/// Every metric a finding, an attention hotspot or a requested record
/// shows, with what it measures.
fn metrics_shown(
    recap: &PrRecap,
    shown: &[&Finding],
    attention: &[AttentionFile],
    resolved: &Resolved,
) -> std::collections::BTreeMap<String, &'static str> {
    let findings = shown.iter().filter_map(|finding| finding.metric.as_deref());
    let spots = attention
        .iter()
        .filter_map(|file| file.hotspot.as_ref())
        .map(|spot| spot.metric.as_str());
    let records = resolved
        .found
        .iter()
        .flat_map(|&index| recap.files[index].hotspots.iter())
        .map(|spot| spot.metric.as_str());
    describe_metrics(findings.chain(spots).chain(records))
}

/// What the summary leaves out. A scored file counts as left out only when
/// it has no entry in `attention` or in the requested records; a hotspot
/// only when it is not the one shown for an attention file or inside a
/// requested record. `attention` still holds repo-relative paths.
fn omitted(
    recap: &PrRecap,
    shown: &[&Finding],
    attention: &[AttentionFile],
    included: &[usize],
) -> Omitted {
    let listed: BTreeSet<&str> = attention
        .iter()
        .map(|file| file.path.as_str())
        .chain(included.iter().map(|&i| recap.files[i].path.as_str()))
        .collect();
    let files = recap
        .files
        .iter()
        .filter(|file| !listed.contains(file.path.as_str()))
        .count();
    Omitted {
        findings: recap.findings.len() - shown.len(),
        files,
        hotspots: hotspots_left(recap, attention, included),
        clusters: recap.clusters.len(),
        skipped: recap.skipped.len(),
    }
}

fn hotspots_left(recap: &PrRecap, attention: &[AttentionFile], included: &[usize]) -> usize {
    let shown_for = |path: &str| {
        attention
            .iter()
            .any(|entry| entry.path == path && entry.hotspot.is_some())
    };
    recap
        .files
        .iter()
        .enumerate()
        .filter(|(index, _)| !included.contains(index))
        .map(|(_, file)| file.hotspots.len() - usize::from(shown_for(&file.path)))
        .sum()
}

fn slim(finding: &Finding, absolute: &Absolute) -> SummaryFinding {
    let side = finding
        .after
        .zip(finding.limit)
        .and_then(|(after, limit)| bound(after, limit));
    let [before, after, limit] = round_options([finding.before, finding.after, finding.limit]);
    SummaryFinding {
        gate: finding.gate.key().to_string(),
        severity: finding.severity.as_str(),
        path: absolute.path(&finding.path),
        line: finding.line,
        function: finding.function.clone(),
        pillar: finding.pillar.clone(),
        metric: finding.metric.clone(),
        before,
        after,
        limit,
        bound: side,
        text: finding.text.clone(),
    }
}

/// The values that are there, rounded together so that no two different
/// ones read as equal.
fn round_options(values: [Option<f64>; 3]) -> [Option<f64>; 3] {
    let present: Vec<f64> = values.iter().flatten().copied().collect();
    let mut rounded = round_apart(&present).into_iter();
    values.map(|value| value.and_then(|_| rounded.next()))
}

#[cfg(test)]
mod tests;
