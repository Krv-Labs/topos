//! SECURE diagnostics surfaced by MCP tools.
//!
//! Findings come from the embedded [Sighthound](https://github.com/Corgea/Sighthound)
//! engine (see [`crate::sighthound`]) when its scan succeeds, mirroring the
//! Python integration's "prefer Sighthound" behavior — except the engine is
//! now compiled in rather than discovered on `$PATH`. The local CPG probes
//! remain the fallback for scan failures and unsupported languages.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use topos_engine::functors::probes::cpg::danger::{
    callee_from_text, effective_registry, matches_registry,
};
use topos_engine::functors::probes::cpg::taint::taint_sources;
use topos_engine::graphs::cpg::models::CPGEdgeKind;
use topos_engine::graphs::cpg::object::CodePropertyGraph;

use crate::schemas::{SecurityAdvisory, SecurityFinding};
use crate::sighthound::{self, SEVERITIES};

/// Display cap for `security_findings` on MCP results.
pub const MAX_SECURITY_FINDINGS: usize = 20;

/// Capped findings plus the advisory computed over the uncapped list.
#[derive(Debug, Default)]
pub struct SecurityReport {
    pub findings: Vec<SecurityFinding>,
    /// `Some` only when Sighthound produced the findings.
    pub advisory: Option<SecurityAdvisory>,
}

impl SecurityReport {
    /// Cap a full (already sorted) list at `max_findings`, computing the
    /// advisory over all of it first when `scanned` (Sighthound ran).
    pub fn from_full(mut all: Vec<SecurityFinding>, scanned: bool, max_findings: usize) -> Self {
        let advisory = scanned.then(|| advisory(&all, all.len().min(max_findings)));
        all.truncate(max_findings);
        SecurityReport {
            findings: all,
            advisory,
        }
    }
}

/// Summarize `all` findings, of which the first `shown` are displayed.
pub fn advisory(all: &[SecurityFinding], shown: usize) -> SecurityAdvisory {
    let max_severity = SEVERITIES
        .iter()
        .find(|level| all.iter().any(|f| f.severity.as_deref() == Some(**level)))
        .copied()
        .unwrap_or("none");
    let mut by_family: BTreeMap<String, usize> = BTreeMap::new();
    for f in all {
        *by_family
            .entry(f.family.unwrap_or("other").to_string())
            .or_default() += 1;
    }
    SecurityAdvisory {
        max_severity: max_severity.to_string(),
        actionable: all
            .iter()
            .filter(|f| {
                matches!(f.severity.as_deref(), Some("critical" | "high"))
                    && f.confidence.as_deref() != Some("low")
            })
            .count(),
        taint: all
            .iter()
            .filter(|f| f.mode.as_deref() == Some("taint"))
            .count(),
        omitted: all.len().saturating_sub(shown),
        by_family,
    }
}

/// Every finding for `cpg` (uncapped), and whether Sighthound produced them
/// (`false` means the CPG-probe fallback did).
pub fn all_security_findings(
    cpg: &CodePropertyGraph,
    allow: Option<&HashSet<String>>,
    file_path: Option<&Path>,
) -> (Vec<SecurityFinding>, bool) {
    if let Some(findings) =
        sighthound::sighthound_all_findings(&cpg.source, &cpg.language, allow, file_path)
    {
        return (findings, true);
    }
    let mut findings = dangerous_call_findings(cpg, usize::MAX, allow);
    findings.extend(taint_flow_findings(cpg, usize::MAX, allow));
    (findings, false)
}

/// Capped findings plus the Sighthound advisory (see [`SecurityReport`]).
pub fn security_report(
    cpg: Option<&CodePropertyGraph>,
    max_findings: usize,
    allow: Option<&HashSet<String>>,
    file_path: Option<&Path>,
) -> SecurityReport {
    let Some(cpg) = cpg else {
        return SecurityReport::default();
    };
    let (all, scanned) = all_security_findings(cpg, allow, file_path);
    SecurityReport::from_full(all, scanned, max_findings)
}

/// Return concise dangerous-call and taint-flow diagnostics.
///
/// When `allow` is non-empty, allowlisted patterns are excluded from the
/// registry first. `file_path` (when the source lives on disk) lets the
/// Sighthound engine scan the real file instead of a temp copy.
pub fn security_findings(
    cpg: Option<&CodePropertyGraph>,
    max_findings: usize,
    allow: Option<&HashSet<String>>,
    file_path: Option<&Path>,
) -> Vec<SecurityFinding> {
    security_report(cpg, max_findings, allow, file_path).findings
}

fn allow_set(allow: Option<&HashSet<String>>) -> HashSet<String> {
    allow.cloned().unwrap_or_default()
}

/// Find dangerous API call sites with source locations.
pub fn dangerous_call_findings(
    cpg: &CodePropertyGraph,
    max_findings: usize,
    allow: Option<&HashSet<String>>,
) -> Vec<SecurityFinding> {
    let registry = effective_registry(&cpg.language, &allow_set(allow));
    if registry.is_empty() {
        return Vec::new();
    }

    // Deterministic order: iterate nodes sorted by source position.
    let mut nodes: Vec<_> = cpg.nodes.values().collect();
    nodes.sort_by_key(|n| (n.uast.span.start_line, n.uast.span.start_byte));

    let mut findings = Vec::new();
    for node in nodes {
        if node.kind() != "CallExpr" {
            continue;
        }
        let text = cpg.node_text(node).trim().to_string();
        if text.is_empty() {
            continue;
        }
        let callee = callee_from_text(&text);
        if callee.is_empty() || !matches_registry(&callee, registry.iter().copied()) {
            continue;
        }
        let line = node.uast.span.start_line.max(1);
        let snippet = line_snippet(&cpg.source, line).unwrap_or(text);
        findings.push(SecurityFinding {
            kind: "dangerous_call".to_string(),
            line: line as u32,
            snippet,
            callee: Some(callee),
            mode: Some("pattern".to_string()),
            ..Default::default()
        });
        if findings.len() >= max_findings {
            break;
        }
    }
    findings
}

fn build_forward_ddg_map(cpg: &CodePropertyGraph) -> HashMap<&str, Vec<&str>> {
    let mut forward: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &cpg.edges {
        if edge.kind == CPGEdgeKind::Ddg {
            forward
                .entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
        }
    }
    forward
}

/// Find source-to-dangerous-sink DDG paths with source/sink snippets.
pub fn taint_flow_findings(
    cpg: &CodePropertyGraph,
    max_findings: usize,
    allow: Option<&HashSet<String>>,
) -> Vec<SecurityFinding> {
    let source_registry: HashSet<&str> = taint_sources(&cpg.language).iter().copied().collect();
    let sink_registry = effective_registry(&cpg.language, &allow_set(allow));
    if source_registry.is_empty() || sink_registry.is_empty() {
        return Vec::new();
    }

    let forward = build_forward_ddg_map(cpg);
    if forward.is_empty() {
        return Vec::new();
    }

    let (sources, sinks) = collect_taint_sources_and_sinks(cpg, &source_registry, &sink_registry);
    join_sources_to_sinks(cpg, &forward, &sources, &sinks, max_findings)
}

/// Deterministic single pass: bucket CPG nodes into taint sources and
/// dangerous-sink call sites.
fn collect_taint_sources_and_sinks<'a>(
    cpg: &'a CodePropertyGraph,
    source_registry: &HashSet<&str>,
    sink_registry: &HashSet<&str>,
) -> (Vec<&'a str>, Vec<(&'a str, String)>) {
    let mut node_items: Vec<_> = cpg.nodes.iter().collect();
    node_items.sort_by_key(|(_, n)| (n.uast.span.start_line, n.uast.span.start_byte));

    let mut sources: Vec<&str> = Vec::new();
    let mut sinks: Vec<(&str, String)> = Vec::new();
    for (node_id, node) in &node_items {
        let snippet = cpg.node_text(node).trim().to_string();
        if snippet.is_empty() {
            continue;
        }
        if node.kind() == "CallExpr" {
            let callee = callee_from_text(&snippet);
            if !callee.is_empty() && matches_registry(&callee, sink_registry.iter().copied()) {
                sinks.push((node_id.as_str(), snippet.clone()));
            }
        }
        if (node.kind() == "Identifier" || node.kind() == "MemberExpr")
            && source_registry.contains(snippet.as_str())
        {
            sources.push(node_id.as_str());
        }
    }
    (sources, sinks)
}

/// BFS-reachability join: emit one finding per source with a DDG path to a sink.
fn join_sources_to_sinks(
    cpg: &CodePropertyGraph,
    forward: &HashMap<&str, Vec<&str>>,
    sources: &[&str],
    sinks: &[(&str, String)],
    max_findings: usize,
) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    for &source_id in sources {
        let reachable = bfs_reachable(forward, source_id);
        for (sink_id, sink_snippet) in sinks {
            if !reachable.contains(sink_id) {
                continue;
            }
            let source_node = &cpg.nodes[source_id];
            let sink_node = &cpg.nodes[*sink_id];
            let source_snippet = cpg.node_text(source_node).trim().to_string();
            let line = sink_node.uast.span.start_line.max(1);
            let callee = callee_from_text(sink_snippet);
            findings.push(SecurityFinding {
                kind: "taint_flow".to_string(),
                line: line as u32,
                snippet: line_snippet(&cpg.source, line).unwrap_or_else(|| sink_snippet.clone()),
                callee: (!callee.is_empty()).then_some(callee),
                source: Some(source_snippet),
                sink: Some(sink_snippet.clone()),
                mode: Some("taint".to_string()),
                ..Default::default()
            });
            if findings.len() >= max_findings {
                return findings;
            }
        }
    }
    findings
}

fn bfs_reachable<'a>(adj: &HashMap<&'a str, Vec<&'a str>>, start: &'a str) -> HashSet<&'a str> {
    let mut visited: HashSet<&str> = HashSet::from([start]);
    let mut frontier = vec![start];
    while let Some(node) = frontier.pop() {
        for &next in adj.get(node).into_iter().flatten() {
            if visited.insert(next) {
                frontier.push(next);
            }
        }
    }
    visited
}

fn line_snippet(source: &str, line: usize) -> Option<String> {
    if line == 0 {
        return None;
    }
    source
        .lines()
        .nth(line - 1)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use topos_engine::core::morphism::ProgramMorphism;

    fn cpg_for(source: &str) -> CodePropertyGraph {
        let mut morphism = ProgramMorphism::new(source, "python");
        morphism.build_cpg().expect("CPG builds").clone()
    }

    #[test]
    fn dangerous_call_is_reported_with_line() {
        let cpg = cpg_for("import os\n\n\ndef f(cmd):\n    os.system(cmd)\n");
        let findings = dangerous_call_findings(&cpg, 20, None);
        assert!(!findings.is_empty());
        let f = &findings[0];
        assert_eq!(f.kind, "dangerous_call");
        assert_eq!(f.callee.as_deref(), Some("os.system"));
        assert_eq!(f.line, 5);
    }

    #[test]
    fn allowlisted_callee_is_excluded() {
        let cpg = cpg_for("import os\n\n\ndef f(cmd):\n    os.system(cmd)\n");
        let allow: HashSet<String> = HashSet::from(["os.system".to_string()]);
        let findings = dangerous_call_findings(&cpg, 20, Some(&allow));
        assert!(findings.is_empty());
    }

    /// Rust has no Sighthound rule pack: the CPG fallback reports findings
    /// (with `mode` only) and no advisory block.
    #[test]
    fn unsupported_language_has_no_advisory() {
        let src =
            "use std::process::Command;\nfn f(c: &str) { Command::new(c).spawn().unwrap(); }\n";
        let mut morphism = ProgramMorphism::new(src, "rust");
        let cpg = morphism.build_cpg().expect("CPG builds").clone();
        let report = security_report(Some(&cpg), 20, None, None);
        assert!(report.advisory.is_none());
        for f in &report.findings {
            assert!(f.mode.is_some() && f.severity.is_none() && f.cwe.is_none());
        }
    }
}
