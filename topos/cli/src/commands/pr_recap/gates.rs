//! Readiness: the configured gates applied to one recap.
//!
//! Every file, split and scoring cap in the document can raise findings.
//! Which of them count, and how much, is the `[pr_recap]` policy's call
//! alone ([`PrGateConfig`]): this module names what happened, looks the
//! severity up, and orders the findings so the one a reviewer should read
//! first comes first. The readiness is the worst severity kept.
//!
//! Two things soften a finding without hiding it. A loss the range's moves
//! explain ([`RangeMoves`]) is reported under `moved_pillar`, and a score
//! drop they explain is immaterial. A finding a `[[pr_recap.waive]]` entry
//! covers keeps its severity and its place in the document, marked
//! [`Finding::waived`], but no longer counts toward the readiness.

use std::cmp::Ordering;
use std::path::Path;

use serde::{Serialize, Serializer};
use topos_engine::config::{FailOn, GateId, PrGateConfig, Severity};
use topos_engine::evaluation::waivers::{self, Waiver};

use super::hotspots::FUNCTION_COMPLEXITY;
use super::model::{
    percent_change, Cluster, FileRecap, GateCrossing, GateSummary, Headline, WaiverSummary,
};
use super::moves::{MoveCause, RangeMoves};

/// Sort points per SECURE finding a split added, so one new finding
/// outranks a few points of score drop or added complexity.
const SPLIT_SECURE_FINDING_WEIGHT: f64 = 10.0;

/// Whether the change is ready to merge under the configured gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Readiness {
    Ready,
    NeedsAttention,
    Blocked,
}

impl Readiness {
    /// A block blocks, a warning needs attention, and info alone (or
    /// nothing at all) is ready.
    fn from_worst(worst: Option<Severity>) -> Readiness {
        match worst {
            Some(Severity::Block) => Readiness::Blocked,
            Some(Severity::Warn) => Readiness::NeedsAttention,
            _ => Readiness::Ready,
        }
    }

    /// The word a card prints.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Readiness::Ready => "READY",
            Readiness::NeedsAttention => "NEEDS ATTENTION",
            Readiness::Blocked => "BLOCKED",
        }
    }

    pub(crate) fn mark(self) -> char {
        match self {
            Readiness::Ready => '✓',
            Readiness::NeedsAttention => '!',
            Readiness::Blocked => 'X',
        }
    }

    /// 1 fails the check: always when blocked, and when the change needs
    /// attention under `fail_on = "warn"`.
    pub(crate) fn exit_code(self, fail_on: FailOn) -> i32 {
        match (self, fail_on) {
            (Readiness::Blocked, _) | (Readiness::NeedsAttention, FailOn::Warn) => 1,
            _ => 0,
        }
    }
}

/// One thing a gate caught, with where to look and what to do about it.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Finding {
    #[serde(serialize_with = "gate_key")]
    pub(crate) gate: GateId,
    #[serde(serialize_with = "severity_name")]
    pub(crate) severity: Severity,
    /// The file, or the split parent; empty for a range-wide finding.
    pub(crate) path: String,
    pub(crate) line: Option<usize>,
    pub(crate) function: Option<String>,
    pub(crate) pillar: Option<String>,
    pub(crate) metric: Option<String>,
    /// The metric's values and its bound for a gate finding; the displayed
    /// pillar scores for a score drop; finding counts or decisions for a
    /// split.
    pub(crate) before: Option<f64>,
    pub(crate) after: Option<f64>,
    pub(crate) limit: Option<f64>,
    pub(crate) fix: String,
    /// The pillar was already failing at base; this change made it worse.
    pub(crate) inherited: bool,
    /// False only for a score drop below the `[pr_recap.score_drop]`
    /// thresholds, or explained by moved code, which is capped at info.
    pub(crate) material: bool,
    /// The file the code behind this finding moved from, when the range's
    /// moves explain it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) moved_from: Option<String>,
    /// Set when a `[[pr_recap.waive]]` entry covers the finding: it keeps
    /// its severity but no longer counts toward the readiness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) waived: Option<Waived>,
    pub(crate) text: String,
    /// How big the finding is, in points, so every gate sorts on one
    /// scale: a pillar finding's score drop plus how far its metric sits
    /// past the limit, a score drop's points,
    /// [`SPLIT_SECURE_FINDING_WEIGHT`] per SECURE finding a split added,
    /// or the complexity a split added.
    #[serde(skip)]
    magnitude: f64,
}

impl Finding {
    /// A finding of `gate` at `path` with nothing else known yet. The
    /// severity is looked up from the policy once every finding is in.
    fn new(gate: GateId, path: &str, text: String) -> Finding {
        Finding {
            gate,
            severity: Severity::Off,
            path: path.to_string(),
            line: None,
            function: None,
            pillar: None,
            metric: None,
            before: None,
            after: None,
            limit: None,
            fix: default_fix(gate).to_string(),
            inherited: false,
            material: true,
            moved_from: None,
            waived: None,
            text,
            magnitude: 0.0,
        }
    }

    /// Counts toward the readiness and the exit code.
    pub(crate) fn counts(&self) -> bool {
        self.waived.is_none()
    }

    fn is_secure(&self) -> bool {
        self.pillar.as_deref() == Some("secure")
    }
}

/// Why a finding does not count: the waiver that covers it.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Waived {
    pub(crate) reason: String,
    /// The file the waiver is written in.
    pub(crate) source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) expires: Option<String>,
}

/// Apply the gates in `cfg` to the scored files, the split clusters and
/// the scoring cap (`incomplete` files left unscored), with the range's
/// `moves` to tell a moved regression from a new one.
///
/// Findings are sorted most important first: severity, then SECURE, then
/// magnitude, then path and line. The readiness is the first one's
/// severity; with none left, the change is ready.
pub(crate) fn evaluate(
    files: &[FileRecap],
    clusters: &[Cluster],
    incomplete: usize,
    cfg: &PrGateConfig,
    moves: &RangeMoves,
) -> (Readiness, Vec<Finding>) {
    let mut found = Vec::new();
    for file in files {
        if file.is_new() {
            new_file_findings(file, &mut found);
        } else {
            // A split parent's fan-out rose only because it imports its
            // own children: COMPOSABLE is set aside, as its status did.
            let routed = clusters
                .iter()
                .any(|cluster| cluster.parent == file.path && cluster.routes_fan_out());
            changed_file_findings(file, routed, cfg, moves, &mut found);
        }
    }
    for cluster in clusters {
        split_findings(cluster, files, &mut found);
    }
    if incomplete > 0 {
        found.push(incomplete_finding(incomplete));
    }
    let mut findings: Vec<Finding> = found
        .into_iter()
        .filter_map(|finding| judged(finding, cfg))
        .collect();
    findings.sort_by(importance);
    (readiness(&findings), findings)
}

/// The worst severity among the findings that count.
pub(crate) fn readiness(findings: &[Finding]) -> Readiness {
    Readiness::from_worst(
        findings
            .iter()
            .filter(|finding| finding.counts())
            .map(|finding| finding.severity)
            .max(),
    )
}

/// Mark every finding a `[[pr_recap.waive]]` entry covers on `today`
/// (`YYYY-MM-DD`), move the waived ones after the rest, and summarize what
/// each waiver did. `source` is the file the waivers came from, reported
/// relative to the repository `root` so a CI document names no runner
/// path. Call this
/// after [`evaluate`], so an `off` gate's findings are already gone and a
/// moved regression is matched under `moved_pillar`.
pub(crate) fn waive(
    findings: &mut [Finding],
    list: &[Waiver],
    today: &str,
    source: Option<&Path>,
    root: &Path,
) -> Vec<WaiverSummary> {
    let source = source.map_or_else(|| ".topos.toml".to_string(), |path| relative_to(path, root));
    let (waived_by, outcomes) = waivers::apply(
        list,
        today,
        findings
            .iter()
            .map(|finding| (finding.gate.key(), finding.path.as_str())),
    );
    for (finding, index) in findings.iter_mut().zip(waived_by) {
        finding.waived = index.map(|index| Waived {
            reason: list[index].reason().to_string(),
            source: source.clone(),
            expires: list[index].expires.clone(),
        });
    }
    // Stable: each group keeps its importance order.
    findings.sort_by_key(|finding| !finding.counts());
    list.iter()
        .zip(outcomes)
        .map(|(waiver, outcome)| WaiverSummary {
            gate: waiver.gate().to_string(),
            path: waiver.path().to_string(),
            reason: waiver.reason().to_string(),
            expires: waiver.expires.clone(),
            matched: outcome.matched,
            status: outcome.status.as_str(),
        })
        .collect()
}

/// The policy's severity for the finding, or `None` when its gate is off.
/// A score drop under the materiality thresholds is at most info.
fn judged(mut finding: Finding, cfg: &PrGateConfig) -> Option<Finding> {
    let severity = cfg.severity(finding.gate);
    finding.severity = if finding.gate == GateId::ScoreDrop && !finding.material {
        severity.min(Severity::Info)
    } else {
        severity
    };
    (finding.severity != Severity::Off).then_some(finding)
}

fn importance(a: &Finding, b: &Finding) -> Ordering {
    b.severity
        .cmp(&a.severity)
        .then_with(|| b.is_secure().cmp(&a.is_secure()))
        .then_with(|| b.magnitude.total_cmp(&a.magnitude))
        .then_with(|| a.path.cmp(&b.path))
        .then_with(|| a.line.cmp(&b.line))
}

/// An added file is judged on arrival. A split child is left to its
/// split's gates: code moved out of the parent brings its findings along,
/// and charging them to the child would fail every split of an imperfect
/// file.
fn new_file_findings(file: &FileRecap, found: &mut Vec<Finding>) {
    if file.is_split_child() || file.medal_after.is_none() {
        return;
    }
    for (pillar, delta) in &file.pillars {
        if delta.after_passed != Some(false) {
            continue;
        }
        let gate = if pillar == "secure" {
            GateId::NewFileInsecure
        } else {
            GateId::NewFilePillar
        };
        found.push(pillar_finding(gate, file, pillar, delta.gate.as_ref(), 0.0));
    }
    if file.landed_slop() {
        found.push(Finding::new(
            GateId::NewFileSlop,
            &file.path,
            format!("{} is new and passes no pillar (SLOP).", file.path),
        ));
    }
}

/// An existing file, one pillar at a time, so losing one pillar while
/// clearing another is still a loss.
fn changed_file_findings(
    file: &FileRecap,
    routed: bool,
    cfg: &PrGateConfig,
    moves: &RangeMoves,
    found: &mut Vec<Finding>,
) {
    let moved = largest_move(file);
    if file.cosmetic {
        let mut finding = Finding::new(
            GateId::Cosmetic,
            &file.path,
            format!(
                "{} moved its scores while the syntax tree barely changed.",
                file.path
            ),
        );
        finding.magnitude = moved;
        found.push(finding);
    }
    if file.status == Headline::SuspiciousNoStructuralChange {
        let mut finding = Finding::new(
            GateId::Suspicious,
            &file.path,
            format!(
                "{} raised its scores while the syntax tree barely changed.",
                file.path
            ),
        );
        finding.magnitude = moved;
        found.push(finding);
    }
    for (pillar, delta) in &file.pillars {
        if !delta.measured || (routed && pillar == "composable") {
            continue;
        }
        // Both sides scored, or one side did not parse and there is
        // nothing to compare.
        let Some(shift) = delta.shift() else {
            continue;
        };
        let drop = tenths(-shift);
        let crossing = delta.gate.as_ref();
        if delta.lost() {
            let lost = pillar_finding(GateId::PillarLost, file, pillar, crossing, drop);
            let cause = moves.caused(&file.path, pillar, lost.function.as_deref(), lost.line);
            found.push(match cause {
                Some(cause) => moved_pillar(lost, file, pillar, crossing, cause),
                None => lost,
            });
            continue;
        }
        let inherited = delta.before_passed == Some(false)
            && delta.after_passed == Some(false)
            && crossing.is_some_and(|gate| gate.worse);
        if inherited {
            found.push(pillar_finding(
                GateId::PillarInherited,
                file,
                pillar,
                crossing,
                drop,
            ));
        }
        if drop > 0.0 {
            let mut finding = score_drop(file, pillar, delta.before_score, drop, cfg);
            let worst = file.worst_function_after.as_ref();
            if let Some(cause) = moves.caused(
                &file.path,
                pillar,
                worst.map(|worst| worst.name.as_str()),
                worst.map(|worst| worst.line),
            ) {
                finding.material = false;
                finding.text = format!(
                    "{}, {}.",
                    finding.text.trim_end_matches('.'),
                    moved_here(cause.from.as_deref())
                );
                finding.moved_from = cause.from;
            }
            found.push(finding);
        }
    }
}

/// A finding about one pillar's gate, pointed at the hotspot for that
/// gate's metric, else at the worst function when the gate is the
/// per-function complexity one.
fn pillar_finding(
    gate: GateId,
    file: &FileRecap,
    pillar: &str,
    crossing: Option<&GateCrossing>,
    drop: f64,
) -> Finding {
    let spot = crossing.and_then(|crossed| {
        file.hotspots
            .iter()
            .find(|spot| spot.metric == crossed.metric)
    });
    let (line, function) = match (spot, crossing, &file.worst_function_after) {
        (Some(spot), _, _) => (Some(spot.line), spot.function.clone()),
        (None, Some(crossed), Some(worst)) if crossed.metric == FUNCTION_COMPLEXITY => {
            (Some(worst.line), Some(worst.name.clone()))
        }
        _ => (None, None),
    };
    let what = match gate {
        GateId::PillarLost => format!("{} lost {}", file.path, pillar.to_ascii_uppercase()),
        GateId::PillarInherited => format!(
            "{} already failed {}, and this change made it worse",
            file.path,
            pillar.to_ascii_uppercase()
        ),
        _ => format!(
            "{} is new and fails {}",
            file.path,
            pillar.to_ascii_uppercase()
        ),
    };
    let text = format!(
        "{what}{}{}.",
        site(function.as_deref(), line),
        crossing.map_or(String::new(), measured)
    );
    let mut finding = Finding::new(gate, &file.path, text);
    finding.line = line;
    finding.function = function;
    finding.pillar = Some(pillar.to_string());
    finding.inherited = gate == GateId::PillarInherited;
    if let Some(crossed) = crossing {
        finding.metric = Some(crossed.metric.clone());
        finding.before = crossed.before;
        finding.after = Some(crossed.after);
        finding.limit = Some(crossed.limit);
        finding.magnitude = (crossed.after - crossed.limit).abs();
        if let Some(advice) = spot
            .map(|spot| spot.advice.as_str())
            .or_else(|| advice_for(&crossed.metric))
        {
            finding.fix = advice.to_string();
        }
    }
    finding.magnitude += drop.max(0.0);
    finding
}

/// A lost pillar the range's moves explain: the same finding, reported
/// under `moved_pillar` and naming where the code came from.
fn moved_pillar(
    mut finding: Finding,
    file: &FileRecap,
    pillar: &str,
    crossing: Option<&GateCrossing>,
    cause: MoveCause,
) -> Finding {
    finding.gate = GateId::MovedPillar;
    finding.text = format!(
        "{} lost {} {}{}{}.",
        file.path,
        pillar.to_ascii_uppercase(),
        moved_here(cause.from.as_deref()),
        site(finding.function.as_deref(), finding.line),
        crossing.map_or(String::new(), measured)
    );
    finding.fix = format!(
        "The code moved here from {}; the regression predates this PR.",
        cause.from.as_deref().unwrap_or("another file")
    );
    finding.moved_from = cause.from;
    finding
}

/// `with code moved here from src/a.py`.
fn moved_here(from: Option<&str>) -> String {
    match from {
        Some(from) => format!("with code moved here from {from}"),
        None => "with code moved here from another file".to_string(),
    }
}

/// A pillar score that fell without failing, or on a pillar already
/// failing. Material only when both the drop and the file's churn reach
/// the `[pr_recap.score_drop]` thresholds.
fn score_drop(
    file: &FileRecap,
    pillar: &str,
    before: Option<f64>,
    drop: f64,
    cfg: &PrGateConfig,
) -> Finding {
    let changed = file.lines_added + file.lines_removed;
    let material = drop >= f64::from(cfg.score_drop.min_points)
        && changed >= cfg.score_drop.min_changed_lines as usize;
    let before = before.unwrap_or_default();
    let after = tenths(before - drop);
    let mut finding = Finding::new(
        GateId::ScoreDrop,
        &file.path,
        format!(
            "{} {} score fell {} → {} ({} points over {changed} changed lines).",
            file.path,
            pillar.to_ascii_uppercase(),
            number(before),
            number(after),
            number(drop),
        ),
    );
    finding.pillar = Some(pillar.to_string());
    finding.before = Some(before);
    finding.after = Some(after);
    finding.material = material;
    finding.magnitude = drop;
    finding
}

/// What a split costs: new SECURE findings, moved code that grew, or a
/// split that bloated or left a child SLOP.
fn split_findings(cluster: &Cluster, files: &[FileRecap], found: &mut Vec<Finding>) {
    let parent = &cluster.parent;
    if let Some(rise) = cluster.secure_rise() {
        let mut finding = Finding::new(
            GateId::SplitSecureRise,
            parent,
            format!(
                "The split of {parent} raised SECURE findings {}→{}.",
                cluster.secure_findings_before, cluster.secure_findings_after
            ),
        );
        finding.pillar = Some("secure".to_string());
        finding.before = Some(cluster.secure_findings_before as f64);
        finding.after = Some(cluster.secure_findings_after as f64);
        finding.magnitude = rise as f64 * SPLIT_SECURE_FINDING_WEIGHT;
        found.push(finding);
    }
    if let Some(gained) = cluster.moved_growth() {
        let mut finding = Finding::new(
            GateId::SplitMovedGrowth,
            parent,
            format!(
                "The split of {parent} moved functions that gained {gained} complexity on the way, \
                 and the worst function did not fall."
            ),
        );
        finding.pillar = Some("simple".to_string());
        finding.magnitude = gained as f64;
        found.push(finding);
    }
    let slop_child = cluster
        .children
        .iter()
        .filter_map(|child| files.iter().find(|file| file.path == child.path))
        .any(FileRecap::landed_slop);
    let bloated = cluster.bloated();
    if bloated || slop_child {
        let (before, after) = (cluster.decisions_before, cluster.decisions_after);
        let mut why = Vec::new();
        if bloated {
            why.push(format!(
                "grew decisions {before}→{after} (+{}%)",
                percent_change(before, after)
            ));
        }
        if slop_child {
            why.push("left a child SLOP or unparsed".to_string());
        }
        let mut finding = Finding::new(
            GateId::SplitBloat,
            parent,
            format!("The split of {parent} {}.", why.join(" and ")),
        );
        finding.before = Some(before as f64);
        finding.after = Some(after as f64);
        finding.magnitude = after.saturating_sub(before) as f64;
        found.push(finding);
    }
}

fn incomplete_finding(unscored: usize) -> Finding {
    Finding::new(
        GateId::Incomplete,
        "",
        format!(
            "{unscored} lower-churn file{} went unscored over the --max-files cap, so the \
             verdict covers only the files scored.",
            if unscored == 1 { "" } else { "s" }
        ),
    )
}

/// The largest displayed-score move on any pillar, either way.
fn largest_move(file: &FileRecap) -> f64 {
    file.pillars
        .values()
        .filter_map(|delta| delta.shift())
        .map(f64::abs)
        .fold(0.0, f64::max)
}

/// ` in pick (line 12)`, ` at line 12`, or nothing.
fn site(function: Option<&str>, line: Option<usize>) -> String {
    match (function, line) {
        (Some(function), Some(line)) => format!(" in {function} (line {line})"),
        (Some(function), None) => format!(" in {function}"),
        (None, Some(line)) => format!(" at line {line}"),
        (None, None) => String::new(),
    }
}

/// `: ast.max_function_complexity 9→14, limit 10`.
fn measured(crossed: &GateCrossing) -> String {
    let value = match crossed.before {
        Some(before) => format!("{}→{}", number(before), number(crossed.after)),
        None => format!("is {}", number(crossed.after)),
    };
    format!(
        ": {} {value}, limit {}",
        crossed.metric,
        number(crossed.limit)
    )
}

/// Up to two decimals, trailing zeros dropped: `14`, `7.5`, `0.35`.
pub(super) fn number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Rounded to the displayed scale's one decimal, so `88.3 - 80.8` is 7.5.
fn tenths(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// What to do about a gating metric, shared with the hotspots: one entry
/// per metric that gates a pillar in the engine's registry.
pub(super) fn advice_for(metric: &str) -> Option<&'static str> {
    Some(match metric {
        "ast.max_function_complexity" => {
            "Extract a decision or a helper so this function clears the gate."
        }
        "ast.entropy" => {
            "Consolidate repeated boilerplate, or split dense logic, to bring token entropy back \
             into band."
        }
        "nav.max_function_divergence" => "Lift the deepest nested block into a named function.",
        "mdg.fan_out" => "Invert a dependency or split the module.",
        "cpg.dangerous_calls" => {
            "Remove the dangerous call, or allowlist it in .topos.toml with a reason."
        }
        "cpg.taint_flows" => "Validate or sanitize the input before it reaches the sink.",
        _ => return None,
    })
}

/// The fix for a finding that no metric's advice covers.
fn default_fix(gate: GateId) -> &'static str {
    match gate {
        GateId::PillarLost
        | GateId::PillarInherited
        | GateId::NewFileInsecure
        | GateId::NewFilePillar => "Bring the failing metric back inside its gate.",
        GateId::MovedPillar => {
            "The code moved here from another file; the regression predates this PR."
        }
        GateId::ScoreDrop => "Simplify what this change added so the score recovers.",
        GateId::NewFileSlop => "Bring at least one pillar inside its gates before merging.",
        GateId::SplitSecureRise => {
            "Remove the dangerous call the split brought in, or allowlist it with a reason."
        }
        GateId::SplitMovedGrowth => "Move functions unchanged, and change them in a separate step.",
        GateId::SplitBloat => {
            "Fold duplicated logic back together, and bring each child out of SLOP."
        }
        GateId::Cosmetic | GateId::Suspicious => {
            "Check that the change does more than reshuffle code the scores react to."
        }
        GateId::Incomplete => "Raise --max-files to score every changed file.",
    }
}

/// `path` relative to `root`, `/` separated, or as given when it is not
/// under `root`. Tried as given, then with symlinks resolved (a temp dir
/// under `/var` is `/private/var` on macOS); only the directory is
/// resolved, so the file itself need not exist.
fn relative_to(path: &Path, root: &Path) -> String {
    let resolved = || {
        let dir = path.parent()?.canonicalize().ok()?;
        Some((dir.join(path.file_name()?), root.canonicalize().ok()?))
    };
    let rest = path
        .strip_prefix(root)
        .ok()
        .map(Path::to_path_buf)
        .or_else(|| {
            let (path, root) = resolved()?;
            path.strip_prefix(root).ok().map(Path::to_path_buf)
        });
    match rest {
        Some(rest) => rest
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        None => path.display().to_string(),
    }
}

/// The worst severity among the findings at `path` that count.
pub(super) fn worst_at(findings: &[Finding], path: &str) -> Option<Severity> {
    findings
        .iter()
        .filter(|finding| finding.path == path && finding.counts())
        .map(|finding| finding.severity)
        .max()
}

/// The JSON `gate` block: which rules produced the verdict.
pub(super) fn summary(cfg: &PrGateConfig, source: Option<&Path>) -> GateSummary {
    GateSummary {
        preset: cfg.preset.as_str(),
        fail_on: cfg.fail_on.as_str(),
        changes: cfg.overrides().len(),
        source: source.map(|path| path.display().to_string()),
        max_hotspots: cfg.max_hotspots as usize,
    }
}

fn gate_key<S: Serializer>(gate: &GateId, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(gate.key())
}

fn severity_name<S: Serializer>(severity: &Severity, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(severity.as_str())
}

pub(super) fn optional_severity_name<S: Serializer>(
    severity: &Option<Severity>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match severity {
        Some(severity) => serializer.serialize_str(severity.as_str()),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use topos_engine::config::PrGatePreset;
    use topos_engine::evaluation::policies::gates::GATE_SPECS;
    use topos_engine::functors::profunctors::uast::ledger::{FunctionMatch, Ledger, MatchKind};

    use super::super::model::{
        ClusterChild, ClusterMark, ClusterMembership, ClusterRole, FileChange, FunctionRef,
        Hotspot, Medal, PillarDelta,
    };
    use super::*;

    const PILLARS: [&str; 4] = ["composable", "navigable", "secure", "simple"];

    /// The gates over a range in which nothing moved.
    fn unmoved(
        files: &[FileRecap],
        clusters: &[Cluster],
        incomplete: usize,
        cfg: &PrGateConfig,
    ) -> (Readiness, Vec<Finding>) {
        evaluate(files, clusters, incomplete, cfg, &RangeMoves::default())
    }

    fn medal(tier: &str) -> Medal {
        Medal {
            symbol: String::new(),
            tier: tier.to_string(),
            verdict: String::new(),
        }
    }

    /// An existing file passing every pillar at 80 on both sides, with 40
    /// lines added and 20 removed.
    fn changed(path: &str) -> FileRecap {
        let pillars = PILLARS
            .iter()
            .map(|key| {
                (
                    (*key).to_string(),
                    PillarDelta {
                        measured: *key != "composable",
                        before_passed: (*key != "composable").then_some(true),
                        after_passed: (*key != "composable").then_some(true),
                        before_score: Some(80.0),
                        after_score: Some(80.0),
                        lost_gate: None,
                        gate: None,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        FileRecap {
            path: path.to_string(),
            change: FileChange::Modified,
            status: Headline::LateralMove,
            severity: None,
            lines_before: 100,
            lines_after: 120,
            lines_added: 40,
            lines_removed: 20,
            medal_before: Some(medal("GOLD")),
            medal_after: Some(medal("GOLD")),
            pillars,
            structural_distance: Some(0.4),
            cosmetic: false,
            complexity_relocated_within_file: false,
            worst_function_before: None,
            worst_function_after: Some(FunctionRef {
                name: "pick".to_string(),
                line: 12,
                complexity: 14,
            }),
            decisions_before: Some(10),
            decisions_after: Some(12),
            fan_in_before: None,
            fan_in_after: None,
            fan_out_before: None,
            fan_out_after: None,
            cluster: None,
            hotspots: Vec::new(),
        }
    }

    /// An added file passing every measured pillar at 80.
    fn added(path: &str) -> FileRecap {
        let mut file = changed(path);
        file.change = FileChange::Added;
        file.medal_before = None;
        for delta in file.pillars.values_mut() {
            delta.before_passed = None;
            delta.before_score = None;
        }
        file
    }

    fn set(file: &mut FileRecap, pillar: &str, passed: (bool, bool), scores: (f64, f64)) {
        let delta = file.pillars.get_mut(pillar).unwrap();
        if !file.change.eq(&FileChange::Added) {
            delta.before_passed = Some(passed.0);
            delta.before_score = Some(scores.0);
        }
        delta.after_passed = Some(passed.1);
        delta.after_score = Some(scores.1);
    }

    fn crossing(metric: &str, before: f64, after: f64, limit: f64) -> Option<GateCrossing> {
        Some(GateCrossing {
            metric: metric.to_string(),
            before: Some(before),
            after,
            limit,
            worse: after > before,
        })
    }

    fn cluster(parent: &str, child: &str) -> Cluster {
        Cluster {
            parent: parent.to_string(),
            children: vec![ClusterChild {
                path: child.to_string(),
                reach: None,
                importers: Vec::new(),
                moved_in: 1,
            }],
            mark: ClusterMark::Ok,
            reasons: Vec::new(),
            lines_before: 100,
            lines_after: 100,
            secure_findings_before: 0,
            secure_findings_after: 0,
            decisions_before: 20,
            decisions_after: 20,
            worst_function_before: None,
            worst_function_after: None,
            parent_fan_out_before: None,
            parent_fan_out_after: None,
            parent_fan_out_after_excluding_children: None,
            symbols_moved: Vec::new(),
            symbols_new: Vec::new(),
            symbols_lost: Vec::new(),
            ledger: None,
        }
    }

    fn recommended() -> PrGateConfig {
        PrGateConfig::for_preset(PrGatePreset::Recommended)
    }

    fn gates(findings: &[Finding]) -> Vec<GateId> {
        findings.iter().map(|finding| finding.gate).collect()
    }

    #[test]
    fn a_lost_pillar_blocks_and_points_at_the_worst_function() {
        let mut file = changed("src/a.rs");
        set(&mut file, "simple", (true, false), (60.0, 40.0));
        file.pillars.get_mut("simple").unwrap().gate =
            crossing("ast.max_function_complexity", 9.0, 14.0, 10.0);
        let (readiness, findings) = unmoved(&[file], &[], 0, &recommended());
        assert_eq!(readiness, Readiness::Blocked);
        assert_eq!(gates(&findings), [GateId::PillarLost]);
        let lost = &findings[0];
        assert_eq!(lost.severity, Severity::Block);
        assert_eq!(
            (lost.line, lost.function.as_deref()),
            (Some(12), Some("pick"))
        );
        assert_eq!(
            (lost.before, lost.after, lost.limit),
            (Some(9.0), Some(14.0), Some(10.0))
        );
        // 20 points of score drop plus 4 over the limit.
        assert_eq!(lost.magnitude, 24.0);
        assert_eq!(
            lost.text,
            "src/a.rs lost SIMPLE in pick (line 12): ast.max_function_complexity 9→14, limit 10."
        );
        assert_eq!(lost.fix, advice_for("ast.max_function_complexity").unwrap());
    }

    #[test]
    fn a_matching_hotspot_supplies_the_line_and_the_fix() {
        let mut file = changed("src/a.rs");
        set(&mut file, "navigable", (true, false), (90.0, 60.0));
        file.pillars.get_mut("navigable").unwrap().gate =
            crossing("nav.max_function_divergence", 3.0, 6.0, 4.0);
        file.hotspots.push(Hotspot {
            path: "src/a.rs".to_string(),
            line: 30,
            function: Some("nest".to_string()),
            metric: "nav.max_function_divergence".to_string(),
            detail: String::new(),
            advice: "Flatten nest.".to_string(),
        });
        let (_, findings) = unmoved(&[file], &[], 0, &recommended());
        assert_eq!(
            (findings[0].line, findings[0].function.as_deref()),
            (Some(30), Some("nest"))
        );
        assert_eq!(findings[0].fix, "Flatten nest.");
    }

    /// B1: SIMPLE lost while NAVIGABLE is cleared leaves the medals
    /// incomparable, so the file's status is LATERAL; the loss still blocks.
    #[test]
    fn losing_one_pillar_while_gaining_another_is_still_a_loss() {
        let mut file = changed("src/a.rs");
        set(&mut file, "simple", (true, false), (60.0, 40.0));
        set(&mut file, "navigable", (false, true), (40.0, 90.0));
        assert_eq!(file.status, Headline::LateralMove);
        let (readiness, findings) = unmoved(&[file], &[], 0, &recommended());
        assert_eq!(readiness, Readiness::Blocked);
        assert_eq!(gates(&findings), [GateId::PillarLost]);
        assert_eq!(findings[0].pillar.as_deref(), Some("simple"));
    }

    #[test]
    fn a_pillar_already_failing_is_inherited_only_when_it_got_worse() {
        let mut worse = changed("src/worse.rs");
        set(&mut worse, "simple", (false, false), (30.0, 30.0));
        worse.pillars.get_mut("simple").unwrap().gate =
            crossing("ast.max_function_complexity", 12.0, 15.0, 10.0);
        let mut same = changed("src/same.rs");
        set(&mut same, "simple", (false, false), (30.0, 30.0));
        same.pillars.get_mut("simple").unwrap().gate =
            crossing("ast.max_function_complexity", 15.0, 15.0, 10.0);
        let (readiness, findings) = unmoved(&[worse, same], &[], 0, &recommended());
        assert_eq!(gates(&findings), [GateId::PillarInherited]);
        assert!(findings[0].inherited);
        assert_eq!(findings[0].path, "src/worse.rs");
        assert_eq!(findings[0].severity, Severity::Info);
        assert_eq!(readiness, Readiness::Ready);
    }

    #[test]
    fn score_drops_are_material_only_past_both_thresholds() {
        let file = |path: &str, drop: f64, lines: usize| {
            let mut file = changed(path);
            set(&mut file, "simple", (true, true), (80.0, 80.0 - drop));
            (file.lines_added, file.lines_removed) = (lines, 0);
            file
        };
        let files = [
            file("src/both.rs", 12.0, 60),
            file("src/lines_only.rs", 5.0, 60),
            file("src/points_only.rs", 12.0, 5),
        ];
        let (readiness, findings) = unmoved(&files, &[], 0, &recommended());
        let by_path = |path: &str| findings.iter().find(|f| f.path == path).unwrap();
        assert!(by_path("src/both.rs").material);
        assert_eq!(by_path("src/both.rs").severity, Severity::Warn);
        for path in ["src/lines_only.rs", "src/points_only.rs"] {
            assert!(!by_path(path).material, "{path}");
            assert_eq!(by_path(path).severity, Severity::Info, "{path}");
        }
        assert_eq!(readiness, Readiness::NeedsAttention);
        assert_eq!(
            by_path("src/both.rs").text,
            "src/both.rs SIMPLE score fell 80 → 68 (12 points over 60 changed lines)."
        );
    }

    /// The threshold is inclusive and read from the policy, not a constant.
    #[test]
    fn materiality_follows_the_configured_thresholds() {
        let mut file = changed("src/a.rs");
        set(&mut file, "simple", (true, true), (80.0, 70.0));
        (file.lines_added, file.lines_removed) = (20, 0);
        let (_, findings) = unmoved(std::slice::from_ref(&file), &[], 0, &recommended());
        assert!(findings[0].material, "10 points over 20 lines is material");
        let mut cfg = recommended();
        cfg.score_drop.min_changed_lines = 21;
        let (_, findings) = unmoved(&[file], &[], 0, &cfg);
        assert!(!findings[0].material);
    }

    #[test]
    fn a_gate_set_to_off_drops_its_findings() {
        let mut file = changed("src/a.rs");
        set(&mut file, "simple", (true, false), (60.0, 40.0));
        let mut cfg = recommended();
        cfg.gates.set(GateId::PillarLost, Severity::Off);
        let (readiness, findings) = unmoved(&[file], &[], 0, &cfg);
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(readiness, Readiness::Ready);
    }

    /// B3: the largest drop leads, not the first file among equals.
    #[test]
    fn the_largest_drop_sorts_first() {
        let file = |path: &str, drop: f64| {
            let mut file = changed(path);
            set(&mut file, "simple", (true, true), (60.0, 60.0 - drop));
            file
        };
        let files = [file("src/configure.rs", 22.5), file("src/status.rs", 30.0)];
        let (readiness, findings) = unmoved(&files, &[], 0, &recommended());
        assert_eq!(readiness, Readiness::NeedsAttention);
        assert_eq!(findings[0].path, "src/status.rs");
        assert_eq!(findings[1].path, "src/configure.rs");
    }

    #[test]
    fn findings_sort_by_severity_then_secure_then_magnitude_then_place() {
        let mut big = changed("src/big.rs");
        set(&mut big, "simple", (true, true), (80.0, 40.0));
        let mut secure = changed("src/secure.rs");
        set(&mut secure, "secure", (true, true), (100.0, 85.0));
        let mut small = changed("src/b.rs");
        set(&mut small, "simple", (true, true), (80.0, 65.0));
        let mut tie = changed("src/a.rs");
        set(&mut tie, "simple", (true, true), (80.0, 65.0));
        let mut lost = changed("src/z.rs");
        set(&mut lost, "navigable", (true, false), (80.0, 79.0));
        let (_, findings) = unmoved(&[big, secure, small, tie, lost], &[], 0, &recommended());
        let order: Vec<&str> = findings.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            order,
            [
                "src/z.rs",
                "src/secure.rs",
                "src/big.rs",
                "src/a.rs",
                "src/b.rs"
            ]
        );
    }

    #[test]
    fn a_new_file_is_judged_on_arrival() {
        let mut insecure = added("src/run.py");
        set(&mut insecure, "secure", (false, false), (0.0, 50.0));
        insecure.pillars.get_mut("secure").unwrap().gate = Some(GateCrossing {
            metric: "cpg.dangerous_calls".to_string(),
            before: None,
            after: 1.0,
            limit: 0.0,
            worse: true,
        });
        let mut slop = added("src/slop.py");
        for pillar in ["navigable", "secure", "simple"] {
            set(&mut slop, pillar, (false, false), (0.0, 10.0));
        }
        slop.medal_after = Some(medal("SLOP"));
        let (readiness, findings) = unmoved(
            &[insecure, slop, added("src/clean.py")],
            &[],
            0,
            &recommended(),
        );
        assert_eq!(readiness, Readiness::Blocked);
        let at = |path: &str| -> Vec<GateId> {
            findings
                .iter()
                .filter(|f| f.path == path)
                .map(|f| f.gate)
                .collect()
        };
        assert_eq!(at("src/run.py"), [GateId::NewFileInsecure]);
        assert_eq!(
            findings[0].text,
            "src/run.py is new and fails SECURE: cpg.dangerous_calls is 1, limit 0."
        );
        let slop = at("src/slop.py");
        assert!(slop.contains(&GateId::NewFileSlop), "{slop:?}");
        assert!(slop.contains(&GateId::NewFileInsecure), "{slop:?}");
        assert_eq!(
            slop.iter().filter(|g| **g == GateId::NewFilePillar).count(),
            2,
            "SIMPLE and NAVIGABLE"
        );
        assert!(at("src/clean.py").is_empty());
    }

    #[test]
    fn a_split_child_is_left_to_its_split() {
        let mut child = added("src/helpers.py");
        set(&mut child, "secure", (false, false), (0.0, 50.0));
        child.medal_after = Some(medal("SLOP"));
        child.cluster = Some(ClusterMembership {
            parent: "src/big.py".to_string(),
            role: ClusterRole::Child,
        });
        let (_, findings) = unmoved(
            &[child],
            &[cluster("src/big.py", "src/helpers.py")],
            0,
            &recommended(),
        );
        assert_eq!(gates(&findings), [GateId::SplitBloat], "{findings:?}");
        assert!(findings[0].text.contains("left a child SLOP"));
    }

    #[test]
    fn each_split_gate_fires_on_its_own_fact() {
        let mut insecure = cluster("src/a.py", "src/a_child.py");
        (
            insecure.secure_findings_before,
            insecure.secure_findings_after,
        ) = (1, 3);
        let mut grown = cluster("src/b.py", "src/b_child.py");
        let mut entry = FunctionMatch {
            kind: MatchKind::MovedModified,
            before: None,
            after: None,
            similarity: 1.0,
            complexity_delta: 4,
        };
        grown.ledger = Some(Ledger {
            matches: vec![entry.clone()],
            totals: Default::default(),
        });
        let mut bloated = cluster("src/c.py", "src/c_child.py");
        bloated.decisions_after = 30;
        let (readiness, findings) =
            unmoved(&[], &[insecure, grown.clone(), bloated], 0, &recommended());
        assert_eq!(readiness, Readiness::Blocked);
        assert_eq!(
            gates(&findings),
            [
                GateId::SplitSecureRise,
                GateId::SplitMovedGrowth,
                GateId::SplitBloat
            ]
        );
        assert_eq!(findings[0].magnitude, 20.0);
        assert_eq!(findings[1].magnitude, 4.0);
        assert_eq!(findings[2].magnitude, 10.0);

        // A worst function that fell pays for the growth.
        grown.worst_function_before = Some(FunctionRef {
            name: "f".to_string(),
            line: 1,
            complexity: 9,
        });
        grown.worst_function_after = Some(FunctionRef {
            name: "f".to_string(),
            line: 1,
            complexity: 5,
        });
        entry.complexity_delta = 2;
        let (_, findings) = unmoved(&[], &[grown], 0, &recommended());
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn cosmetic_suspicious_and_incomplete_are_findings() {
        let mut cosmetic = changed("src/a.rs");
        cosmetic.cosmetic = true;
        set(&mut cosmetic, "simple", (true, true), (60.0, 70.0));
        let mut suspicious = cosmetic.clone();
        suspicious.path = "src/b.rs".to_string();
        suspicious.status = Headline::SuspiciousNoStructuralChange;
        let (readiness, findings) = unmoved(&[cosmetic, suspicious], &[], 2, &recommended());
        assert_eq!(readiness, Readiness::NeedsAttention);
        let found = gates(&findings);
        assert_eq!(
            found.iter().filter(|g| **g == GateId::Cosmetic).count(),
            2,
            "{found:?}"
        );
        assert!(found.contains(&GateId::Suspicious), "{found:?}");
        let incomplete = findings
            .iter()
            .find(|f| f.gate == GateId::Incomplete)
            .unwrap();
        assert_eq!(incomplete.severity, Severity::Info);
        assert!(incomplete.text.starts_with("2 lower-churn files"));
    }

    /// COMPOSABLE is set aside for a split parent whose fan-out went only
    /// to its own children.
    #[test]
    fn a_routed_split_parent_does_not_lose_composable() {
        let mut parent = changed("src/big.py");
        let composable = parent.pillars.get_mut("composable").unwrap();
        composable.measured = true;
        set(&mut parent, "composable", (true, false), (90.0, 60.0));
        let mut routed = cluster("src/big.py", "src/child.py");
        routed.parent_fan_out_before = Some(5);
        routed.parent_fan_out_after_excluding_children = Some(5);
        let (_, findings) = unmoved(&[parent], &[routed], 0, &recommended());
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn readiness_is_the_worst_severity() {
        assert_eq!(Readiness::from_worst(None), Readiness::Ready);
        assert_eq!(
            Readiness::from_worst(Some(Severity::Info)),
            Readiness::Ready
        );
        assert_eq!(
            Readiness::from_worst(Some(Severity::Warn)),
            Readiness::NeedsAttention
        );
        assert_eq!(
            Readiness::from_worst(Some(Severity::Block)),
            Readiness::Blocked
        );
        assert!(Readiness::Blocked > Readiness::NeedsAttention);
        assert!(Readiness::NeedsAttention > Readiness::Ready);
    }

    #[test]
    fn exit_code_follows_fail_on() {
        use Readiness::*;
        let codes = |fail_on| [Ready, NeedsAttention, Blocked].map(|r| r.exit_code(fail_on));
        assert_eq!(codes(FailOn::Block), [0, 0, 1]);
        assert_eq!(codes(FailOn::Warn), [0, 1, 1]);
    }

    #[test]
    fn every_gating_metric_has_advice() {
        for spec in GATE_SPECS.iter().filter(|spec| spec.gates_achieved) {
            assert!(advice_for(spec.metric).is_some(), "{}", spec.metric);
        }
    }

    #[test]
    fn a_finding_serializes_its_gate_and_severity_by_name() {
        let mut file = changed("src/a.rs");
        set(&mut file, "simple", (true, false), (60.0, 40.0));
        let (_, findings) = unmoved(&[file], &[], 0, &recommended());
        let json = serde_json::to_value(&findings[0]).unwrap();
        assert_eq!(json["gate"], "pillar_lost");
        assert_eq!(json["severity"], "block");
        assert_eq!(json["pillar"], "simple");
        assert!(json.get("magnitude").is_none());
        assert_eq!(
            serde_json::to_value(Readiness::NeedsAttention).unwrap(),
            "NEEDS_ATTENTION"
        );
    }

    #[test]
    fn a_waiver_source_is_named_from_the_repository_root() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        std::fs::create_dir_all(root.join("sub/dir")).unwrap();
        assert_eq!(relative_to(&root.join(".topos.toml"), root), ".topos.toml");
        assert_eq!(
            relative_to(&root.join("sub/dir/.topos.toml"), root),
            "sub/dir/.topos.toml"
        );
        let elsewhere = Path::new("/elsewhere/.topos.toml");
        assert_eq!(relative_to(elsewhere, root), "/elsewhere/.topos.toml");
    }
}
