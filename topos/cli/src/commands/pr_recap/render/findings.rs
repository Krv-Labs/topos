//! The card's verdict and its findings: the one-line verdict, the
//! numbered list of what blocks or needs attention, the gate settings
//! that produced it, and the `--info` recommended changes.
//!
//! Every sentence here restates a [`Finding`] the gates already judged;
//! nothing is ranked again. The list is the recap's order, merged by
//! place and capped at `max_hotspots`.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;
use topos_engine::config::{GateId, Severity};
use topos_engine::evaluation::suggestions::Suggestion;
use topos_mcp::schemas::RefactorTarget;

use super::layout::{budget, dim_wrapped, plain_budget, rail, wrapped};
use crate::commands::evaluate::info_render::{recommendation_lines, FileDetails};
use crate::commands::pr_recap::gates::{number, Finding, Readiness};
use crate::commands::pr_recap::model::{FileRecap, PrRecap};
use crate::commands::pr_recap::view::{
    common_parent, drop_of, lost_a_pillar, pillar_names, relative, short_name, visible_dip, Item,
    RecapView, PILLARS,
};
use crate::commands::render::RenderOptions;

/// Where the numbered list's facts start on a two-line item: under the
/// location, past `1. X `.
const FACT_INDENT: usize = 5;

// ---------------------------------------------------------------- verdict

/// `X BLOCKED   dispatch.rs lost SIMPLE and NAVIGABLE (GOLD → BRONZE)`,
/// wrapped under the headline rather than cut.
pub(super) fn verdict_lines(
    view: &RecapView<'_>,
    items: &[Item<'_>],
    options: RenderOptions,
) -> Vec<String> {
    let recap = view.recap;
    let label = format!("{} {}   ", recap.readiness.mark(), recap.readiness.word());
    let continuation = " ".repeat(label.chars().count());
    wrapped(&label, &continuation, &headline(recap, items), options)
}

/// The single most important finding in a phrase, or for a ready change,
/// what did not happen and how small the dips were.
pub(in crate::commands::pr_recap) fn headline(recap: &PrRecap, items: &[Item<'_>]) -> String {
    let paths: Vec<&str> = recap.files.iter().map(|file| file.path.as_str()).collect();
    if let Some(item) = items.first() {
        return finding_phrase(recap, item.lead(), &paths);
    }
    if recap.files.is_empty() {
        return sentence(&recap.reason);
    }
    let medal_lost = recap
        .project
        .as_ref()
        .is_some_and(|project| project.regression);
    if !medal_lost && !recap.files.iter().any(lost_a_pillar) {
        return format!("no pillar or medal lost{}", dips_phrase(recap, &paths));
    }
    recap.findings.first().map_or_else(
        || sentence(&recap.reason),
        |finding| finding_phrase(recap, finding, &paths),
    )
}

/// `; 5 small dips (largest graphs/mod.rs SIMPLE −7.5)`, counting only
/// dips of a point or more.
fn dips_phrase(recap: &PrRecap, paths: &[&str]) -> String {
    let dips: Vec<&Finding> = recap.findings.iter().filter(|f| visible_dip(f)).collect();
    let Some(largest) = dips.iter().max_by(|a, b| drop_of(a).total_cmp(&drop_of(b))) else {
        return String::new();
    };
    let what = format!(
        "{} {} −{}",
        short_name(&largest.path, paths),
        pillar_word(largest),
        number(drop_of(largest))
    );
    if dips.len() == 1 {
        format!("; 1 small dip ({what})")
    } else {
        format!("; {} small dips (largest {what})", dips.len())
    }
}

/// One finding as the verdict states it, the file named by its shortest
/// unambiguous suffix.
fn finding_phrase(recap: &PrRecap, finding: &Finding, paths: &[&str]) -> String {
    let short = short_name(&finding.path, paths);
    let file = recap.files.iter().find(|file| file.path == finding.path);
    match (finding.gate, file) {
        (GateId::PillarLost, Some(file)) => lost_phrase(file, short),
        (GateId::PillarInherited, _) => {
            format!(
                "{short} already failed {} and got worse",
                pillar_word(finding)
            )
        }
        (GateId::NewFileInsecure | GateId::NewFilePillar, Some(file)) => format!(
            "{short} is new and fails {}",
            and_list(&pillar_names(file, |delta| delta.after_passed == Some(false)))
        ),
        (GateId::ScoreDrop, Some(file)) => format!(
            "{short} {} fell {} → {} over {} changed lines",
            pillar_word(finding),
            number(finding.before.unwrap_or_default()),
            number(finding.after.unwrap_or_default()),
            file.lines_added + file.lines_removed
        ),
        _ => sentence(&finding.text.replace(&finding.path, short)),
    }
}

/// `foo.rs traded SIMPLE for NAVIGABLE` when the file cleared a pillar
/// while losing another, else `dispatch.rs lost SIMPLE and NAVIGABLE`
/// with the medal move when there was one.
fn lost_phrase(file: &FileRecap, short: &str) -> String {
    let lost = and_list(&pillar_names(file, |delta| delta.lost()));
    let cleared = pillar_names(file, |delta| delta.cleared());
    if !cleared.is_empty() {
        return format!("{short} traded {lost} for {}", and_list(&cleared));
    }
    match (&file.medal_before, &file.medal_after) {
        (Some(before), Some(after)) if before.tier != after.tier => {
            format!("{short} lost {lost} ({} → {})", before.tier, after.tier)
        }
        _ => format!("{short} lost {lost}"),
    }
}

// ----------------------------------------------------------- finding list

/// The numbered block and warn items, at most `max_hotspots` of them,
/// then one dim line for everything not listed.
///
/// Items sit on one aligned line each when they all fit; otherwise each
/// item gets its location on one line and its facts wrapped underneath.
pub(super) fn list_lines(
    view: &RecapView<'_>,
    items: &[Item<'_>],
    options: RenderOptions,
) -> Vec<String> {
    let recap = view.recap;
    let shown = &items[..items.len().min(recap.gate.max_hotspots)];
    let parent = common_parent(
        shown
            .iter()
            .map(|item| item.lead().path.as_str())
            .filter(|path| !path.is_empty()),
    );
    let rows: Vec<(String, String)> = shown
        .iter()
        .enumerate()
        .map(|(index, item)| {
            (
                format!(
                    "{}. {} {}",
                    index + 1,
                    severity_mark(item.severity()),
                    location(item.lead(), parent.as_deref())
                ),
                facts(item),
            )
        })
        .collect();
    let width = rows
        .iter()
        .map(|(place, _)| place.chars().count())
        .max()
        .unwrap_or(0);
    let one_line = rows
        .iter()
        .all(|(_, facts)| width + 2 + facts.chars().count() <= budget(options));

    let mut lines = Vec::new();
    for (place, facts) in &rows {
        if one_line {
            let gap = " ".repeat(width + 2 - place.chars().count());
            lines.push(rail(format!("{place}{gap}{facts}").trim_end(), options));
        } else {
            lines.extend(wrapped("", &" ".repeat(FACT_INDENT), place, options));
            let indent = " ".repeat(FACT_INDENT);
            lines.extend(wrapped(&indent, &indent, facts, options));
        }
    }
    let rest = rest_line(view, items, shown, parent.as_deref());
    if !rest.is_empty() {
        let indent = " ".repeat(FACT_INDENT);
        lines.extend(dim_wrapped(&indent, &indent, &rest, options));
    }
    lines
}

/// `in topos/cli/src/commands/install/ · 2 more · 3 smaller dips · 1 note`,
/// zero counts left out.
fn rest_line(
    view: &RecapView<'_>,
    items: &[Item<'_>],
    shown: &[Item<'_>],
    parent: Option<&str>,
) -> String {
    let listed = |path: &str| shown.iter().any(|item| item.lead().path == path);
    let notes = &view.notes;
    let dips = notes
        .iter()
        .filter(|finding| visible_dip(finding) && !listed(&finding.path))
        .count();
    let others = notes
        .iter()
        .filter(|finding| finding.gate != GateId::ScoreDrop)
        .count();
    let mut parts = Vec::new();
    if let Some(parent) = parent {
        parts.push(format!("in {parent}"));
    }
    if items.len() > shown.len() {
        parts.push(format!("{} more", items.len() - shown.len()));
    }
    if dips > 0 {
        parts.push(plural(dips, "smaller dip", "smaller dips"));
    }
    if others > 0 {
        parts.push(plural(others, "note", "notes"));
    }
    parts.join(" · ")
}

/// `dispatch.rs · sanitize_typescript_type_imports:62`, `status.rs`, or
/// nothing for a finding about the whole range.
fn location(finding: &Finding, parent: Option<&str>) -> String {
    let path = relative(&finding.path, parent);
    match (&finding.function, finding.line) {
        (Some(function), Some(line)) => format!("{path} · {function}:{line}"),
        (Some(function), None) => format!("{path} · {function}"),
        (None, Some(line)) => format!("{path}:{line}"),
        (None, None) => path.to_string(),
    }
}

/// `SIMPLE 32 > 10 · NAVIGABLE 21.4 > 10 · lift the deepest nested block
/// into a named function`: each finding's measurement in pillar order,
/// then what to do about the most important one.
pub(in crate::commands::pr_recap) fn facts(item: &Item<'_>) -> String {
    let mut findings = item.findings.clone();
    findings.sort_by_key(|finding| pillar_rank(finding));
    let mut parts: Vec<String> = Vec::new();
    for finding in findings {
        let fact = fact(finding);
        if !parts.contains(&fact) {
            parts.push(fact);
        }
    }
    let lead = item.lead();
    if lead.metric.is_some() {
        parts.push(lowercase_first(&sentence(&lead.fix)));
    }
    parts.join(" · ")
}

fn fact(finding: &Finding) -> String {
    let pillar = pillar_word(finding);
    match finding.gate {
        GateId::PillarLost
        | GateId::PillarInherited
        | GateId::NewFileInsecure
        | GateId::NewFilePillar => match (finding.after, finding.limit) {
            (Some(after), Some(limit)) => format!(
                "{pillar} {} {} {}",
                tenths(after),
                if after < limit { '<' } else { '>' },
                tenths(limit)
            ),
            _ => format!(
                "{pillar} {}",
                match finding.gate {
                    GateId::PillarLost => "lost",
                    GateId::PillarInherited => "worse",
                    _ => "fails",
                }
            ),
        },
        GateId::ScoreDrop => format!(
            "{pillar} {} → {}",
            number(finding.before.unwrap_or_default()),
            number(finding.after.unwrap_or_default())
        ),
        _ => without_path(finding),
    }
}

// -------------------------------------------------------------- gate line

/// `gate: custom · 2 changes · ./.topos.toml`, and under a policy that
/// fails only on blocks, why a change needing attention still passes.
pub(in crate::commands::pr_recap) fn gate_line(recap: &PrRecap) -> String {
    let gate = &recap.gate;
    let mut parts = vec![format!("gate: {}", gate.preset)];
    if gate.changes > 0 {
        parts.push(plural(gate.changes, "change", "changes"));
        if let Some(source) = &gate.source {
            parts.push(display_source(source));
        }
    }
    if recap.readiness == Readiness::NeedsAttention && gate.fail_on == "block" {
        parts.push("warnings don't fail the check".to_string());
    }
    parts.join(" · ")
}

/// The config file relative to where the command ran, `./.topos.toml`,
/// or as found when it sits elsewhere.
fn display_source(source: &str) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            Path::new(source)
                .strip_prefix(cwd)
                .ok()
                .map(|rest| format!("./{}", rest.display()))
        })
        .unwrap_or_else(|| source.to_string())
}

// ----------------------------------------------------------------- --info

/// `inspect`'s **Recommended changes**, one per listed-or-not item: the
/// pillars it fails, why, what to do, and where.
pub(super) fn recommendations(
    recap: &PrRecap,
    items: &[Item<'_>],
    options: RenderOptions,
) -> Vec<String> {
    let mut details = FileDetails {
        targets: Vec::new(),
        suggestions: Vec::new(),
    };
    for (index, item) in items.iter().enumerate() {
        let (target, suggestion) = target(recap, item, index + 1);
        details.targets.push(target);
        details.suggestions.push(suggestion);
    }
    recommendation_lines(&details, plain_budget(options), options.styled, false)
}

fn target(recap: &PrRecap, item: &Item<'_>, rank: usize) -> (RefactorTarget, Suggestion) {
    let lead = item.lead();
    let severity = if item.severity() == Severity::Block {
        "fix"
    } else {
        "improve"
    };
    let metric = lead
        .metric
        .clone()
        .unwrap_or_else(|| lead.gate.key().to_string());
    let mut pillars: Vec<String> = Vec::new();
    for key in PILLARS {
        if item
            .findings
            .iter()
            .any(|f| f.pillar.as_deref() == Some(key))
        {
            pillars.push(key.to_string());
        }
    }
    let mut why: Vec<String> = Vec::new();
    let mut fixes: Vec<String> = Vec::new();
    for finding in &item.findings {
        let detail = hotspot_detail(recap, finding).unwrap_or_else(|| without_path(finding));
        if !why.contains(&detail) {
            why.push(detail);
        }
        if !fixes.contains(&finding.fix) {
            fixes.push(finding.fix.clone());
        }
    }
    let symbol = match &lead.function {
        Some(function) => format!("{} · {function}", lead.path),
        None => lead.path.clone(),
    };
    let target = RefactorTarget {
        target_id: format!("pr-recap-{rank}"),
        kind: if lead.path.is_empty() {
            "module"
        } else {
            "function"
        }
        .to_string(),
        filepath: lead.path.clone(),
        symbol: (!lead.path.is_empty()).then_some(symbol),
        line_start: lead.line,
        line_end: lead.line,
        failing_generators: pillars.clone(),
        metric: metric.clone(),
        current_value: lead.after,
        threshold: lead.limit,
        severity: severity.to_string(),
        recommended_operations: Vec::new(),
        constraints: Vec::new(),
        evidence: BTreeMap::from([("interpretation".to_string(), Value::String(why.join("; ")))]),
    };
    let suggestion = Suggestion {
        pillar: pillars.first().cloned().unwrap_or_default(),
        metric: Some(metric),
        severity: severity.to_string(),
        message: fixes.join(" "),
    };
    (target, suggestion)
}

/// The hotspot's own sentence for the finding's metric, `pick complexity
/// is 32, gate is 10`, when the file has one.
fn hotspot_detail(recap: &PrRecap, finding: &Finding) -> Option<String> {
    let metric = finding.metric.as_deref()?;
    recap
        .files
        .iter()
        .find(|file| file.path == finding.path)?
        .hotspots
        .iter()
        .find(|spot| spot.metric == metric)
        .map(|spot| spot.detail.clone())
}

// ---------------------------------------------------------------- shared

pub(in crate::commands::pr_recap) fn severity_mark(severity: Severity) -> char {
    match severity {
        Severity::Block => 'X',
        Severity::Warn => '!',
        Severity::Info | Severity::Off => ' ',
    }
}

fn pillar_word(finding: &Finding) -> String {
    finding
        .pillar
        .as_deref()
        .map_or_else(String::new, str::to_ascii_uppercase)
}

fn pillar_rank(finding: &Finding) -> usize {
    finding
        .pillar
        .as_deref()
        .and_then(|pillar| PILLARS.iter().position(|key| *key == pillar))
        .unwrap_or(PILLARS.len())
}

/// The finding's text without the path it starts with, as a phrase:
/// `split grew decisions 10→15 (+50%)`, `moved its scores while …`.
fn without_path(finding: &Finding) -> String {
    let text = sentence(&finding.text);
    if finding.path.is_empty() {
        return text;
    }
    if let Some(rest) = text.strip_prefix(&format!("The split of {} ", finding.path)) {
        return format!("split {rest}");
    }
    text.strip_prefix(&format!("{} ", finding.path))
        .unwrap_or(&text)
        .to_string()
}

/// `SIMPLE and NAVIGABLE`, `SIMPLE, SECURE and NAVIGABLE`.
fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// One decimal, trailing zero dropped: `21.4`, `10`.
fn tenths(value: f64) -> String {
    number((value * 10.0).round() / 10.0)
}

/// A sentence read as a phrase: the trailing period dropped.
fn sentence(text: &str) -> String {
    text.trim().trim_end_matches('.').to_string()
}

fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    })
}
