//! The one ranking of the files that need attention.
//!
//! Files with a block finding come first, then a warning, then files that
//! are only hotspots; within a tier by finding count, then by the order the
//! gates ranked their first finding. The file `reason` names always leads,
//! whatever made it the reason. At least [`MAX_ATTENTION`] files are listed,
//! and every file with a block finding.

use std::cmp::Ordering;

use topos_engine::config::Severity;

use super::super::gates::Finding;
use super::super::model::{FileRecap, PrRecap};
use super::types::{AttentionFile, AttentionSpot};

/// Files in `attention`, unless more files carry a block finding.
const MAX_ATTENTION: usize = 5;

/// How a file got on the list, most pressing first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    /// A hotspot, or named in `reason`, with no block or warn finding.
    Listed,
    Warn,
    Block,
}

struct Entry<'a> {
    path: &'a str,
    tier: Tier,
    count: usize,
    /// Position of the file's first finding in the gates' ranking.
    first: usize,
    why: String,
}

/// The files that need attention, paths repo-relative.
pub(super) fn rank(recap: &PrRecap, counting: &[(usize, &Finding)]) -> Vec<AttentionFile> {
    let mut entries = finding_entries(counting);
    add_reason_file(&mut entries, recap, counting);
    add_hotspot_files(&mut entries, recap);
    let lead = recap.reason_path.as_deref();
    entries.sort_by(|a, b| precedes(a, b, lead));
    let block_files = entries.iter().filter(|e| e.tier == Tier::Block).count();
    entries.truncate(MAX_ATTENTION.max(block_files));
    entries.iter().map(|entry| listed(entry, recap)).collect()
}

fn why(finding: &Finding) -> String {
    format!("{} {}", finding.severity.as_str(), finding.gate.key())
}

fn tier_of(severity: Severity) -> Option<Tier> {
    match severity {
        Severity::Block => Some(Tier::Block),
        Severity::Warn => Some(Tier::Warn),
        _ => None,
    }
}

/// One entry per file with a counting block or warn finding.
fn finding_entries<'a>(counting: &[(usize, &'a Finding)]) -> Vec<Entry<'a>> {
    let mut entries: Vec<Entry> = Vec::new();
    let ranked = counting.iter().filter(|(_, f)| !f.path.is_empty());
    for (index, finding) in ranked {
        let Some(tier) = tier_of(finding.severity) else {
            continue;
        };
        match entries.iter_mut().find(|entry| entry.path == finding.path) {
            Some(entry) => {
                entry.tier = entry.tier.max(tier);
                entry.count += 1;
            }
            None => entries.push(Entry {
                path: &finding.path,
                tier,
                count: 1,
                first: *index,
                why: why(finding),
            }),
        }
    }
    entries
}

/// The file `reason` names is listed even when it has no block or warn
/// finding: an info finding (a score drop under the thresholds) or only the
/// direction text led to it.
fn add_reason_file<'a>(
    entries: &mut Vec<Entry<'a>>,
    recap: &'a PrRecap,
    counting: &[(usize, &Finding)],
) {
    let Some(path) = recap.reason_path.as_deref() else {
        return;
    };
    if entries.iter().any(|entry| entry.path == path) {
        return;
    }
    let top = counting.iter().find(|(_, finding)| finding.path == path);
    entries.push(Entry {
        path,
        tier: Tier::Listed,
        count: 0,
        first: top.map_or(usize::MAX, |(index, _)| *index),
        why: top.map_or_else(|| "named in reason".to_string(), |(_, f)| why(f)),
    });
}

fn add_hotspot_files<'a>(entries: &mut Vec<Entry<'a>>, recap: &'a PrRecap) {
    for file in recap.files.iter().filter(|file| !file.hotspots.is_empty()) {
        if entries.iter().all(|entry| entry.path != file.path) {
            entries.push(Entry {
                path: &file.path,
                tier: Tier::Listed,
                count: file.hotspots.len(),
                first: usize::MAX,
                why: "hotspot".to_string(),
            });
        }
    }
}

/// `a` ranks ahead of `b`: the reason file, then tier, count, gate order,
/// path.
fn precedes(a: &Entry, b: &Entry, lead: Option<&str>) -> Ordering {
    (Some(b.path) == lead)
        .cmp(&(Some(a.path) == lead))
        .then(b.tier.cmp(&a.tier))
        .then(b.count.cmp(&a.count))
        .then(a.first.cmp(&b.first))
        .then(a.path.cmp(b.path))
}

fn listed(entry: &Entry, recap: &PrRecap) -> AttentionFile {
    let file = recap.files.iter().find(|file| file.path == entry.path);
    let blocking = entry.tier != Tier::Listed;
    AttentionFile {
        path: entry.path.to_string(),
        severity: match entry.tier {
            Tier::Block => Some("block"),
            Tier::Warn => Some("warn"),
            Tier::Listed => None,
        },
        findings: if blocking { entry.count } else { 0 },
        why: entry.why.clone(),
        hotspots: file.map_or(0, |file| file.hotspots.len()),
        hotspot: file.and_then(top_hotspot),
    }
}

fn top_hotspot(file: &FileRecap) -> Option<AttentionSpot> {
    file.hotspots.first().map(|spot| AttentionSpot {
        function: spot.function.clone(),
        line: spot.line,
        metric: spot.metric.clone(),
        detail: spot.detail.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::{abs, fixture, info_on, summarize};
    use crate::pr_recap::gates::Waived;
    use crate::pr_recap::tests::recap;
    use topos_engine::config::Severity;

    #[test]
    fn attention_names_the_reason_file_and_every_block_file() {
        let (_keep, repo) = fixture(4, 2);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let summary = summarize(&recap, &repo);
        let attention: Vec<&str> = summary.attention.iter().map(|a| a.path.as_str()).collect();
        let lead = recap.findings.iter().find(|f| f.counts()).unwrap();
        assert_eq!(attention[0], abs(&repo, &lead.path));
        for finding in recap.findings.iter().filter(|f| f.counts()) {
            if finding.severity == Severity::Block {
                assert!(attention.contains(&abs(&repo, &finding.path).as_str()));
            }
        }
        let blocks = summary
            .attention
            .iter()
            .take_while(|a| a.severity == Some("block"))
            .count();
        assert!(blocks > 0);
        assert!(summary.attention[blocks..]
            .iter()
            .all(|a| a.severity != Some("block")));
    }

    #[test]
    fn attention_leads_with_the_reason_file_whatever_made_it_the_reason() {
        let (_keep, repo) = fixture(8, 0);
        let base = recap(&repo, "HEAD~1", "HEAD", 0);
        let last = base.files.last().unwrap().path.clone();
        let last_abs = abs(&repo, &last);

        let mut info = base.clone();
        info_on(&mut info, &last);
        info.reason_path = Some(last.clone());
        let summary = summarize(&info, &repo);
        assert_eq!(summary.attention[0].path, last_abs);
        assert!(summary.attention[0].why.starts_with("info"));
        assert!(summary.findings.is_empty());
        assert_eq!(summary.findings_total["info"], 1);

        let mut waived = base.clone();
        for finding in &mut waived.findings {
            finding.waived = Some(Waived {
                reason: "r".into(),
                source: "s".into(),
                expires: None,
            });
        }
        waived.reason_path = Some(last.clone());
        let summary = summarize(&waived, &repo);
        assert_eq!(summary.attention[0].path, last_abs);
        assert!(summary.findings.is_empty());
        assert_eq!(summary.findings_total["waived"], waived.findings.len());

        let mut none = base.clone();
        none.findings.clear();
        none.reason_path = Some(last);
        let summary = summarize(&none, &repo);
        assert_eq!(summary.attention[0].path, last_abs);
        assert_eq!(summary.attention[0].why, "named in reason");

        let lead = base.findings.iter().find(|f| f.counts()).unwrap();
        assert_eq!(base.reason_path.as_deref(), Some(lead.path.as_str()));
    }
}
