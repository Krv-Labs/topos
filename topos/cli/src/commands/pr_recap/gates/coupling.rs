//! The coupling gates: what the change did to the file dependency graph.
//!
//! These read the two [`FileGraph`]s, so they fire only when coupling was
//! measured, and they see every changed file, not just the ones scored
//! under `--max-files`:
//!
//! - `import_cycle` — a head import cycle the base did not have, pointed at
//!   the introduced edge to cut. Its severity is per language
//!   (`[pr_recap.import_cycle]`), decided in [`super::judged`].
//! - `fan_in_growth` — more files now depend on a file that fails SIMPLE.
//! - `blast_radius` — how far the change reaches, one finding per range.

use std::collections::{BTreeSet, HashMap};

use topos_engine::config::{GateId, PrGateConfig};
use topos_engine::graphs::mdg::file_graph::{new_import_cycles, CycleDelta, FileGraph};

use super::super::model::{Cluster, FileRecap};
use super::Finding;

/// Sort points per file in a new import cycle.
const CYCLE_MEMBER_WEIGHT: f64 = 10.0;

/// New dependents a fan-in finding names before it trails off.
const LISTED_DEPENDENTS: usize = 3;

/// Dependents a blast-radius finding carries in [`Finding::related`].
const TOP_DEPENDENTS: usize = 5;

/// The two file graphs of a measured range, with what changed between
/// them.
#[derive(Debug, Default)]
pub(crate) struct Coupling {
    pub(crate) base: FileGraph,
    pub(crate) head: FileGraph,
    /// Head path → base path, for renamed files only.
    pub(crate) head_to_base: HashMap<String, String>,
    /// Head paths of every changed source file, scored or not.
    pub(crate) changed: BTreeSet<String>,
    /// Base paths of the deleted files.
    pub(crate) deleted: BTreeSet<String>,
}

impl Coupling {
    fn to_base<'p>(&'p self, head_path: &'p str) -> &'p str {
        self.head_to_base
            .get(head_path)
            .map_or(head_path, String::as_str)
    }

    /// Base paths as head paths: renamed files take their new path.
    fn to_head(&self, base_paths: impl IntoIterator<Item = String>) -> BTreeSet<String> {
        let base_to_head: HashMap<&str, &str> = self
            .head_to_base
            .iter()
            .map(|(head, base)| (base.as_str(), head.as_str()))
            .collect();
        base_paths
            .into_iter()
            .map(|path| match base_to_head.get(path.as_str()) {
                Some(head) => head.to_string(),
                None => path,
            })
            .collect()
    }
}

/// Every coupling finding for the range.
pub(super) fn coupling_findings(
    coupling: &Coupling,
    files: &[FileRecap],
    clusters: &[Cluster],
    cfg: &PrGateConfig,
    found: &mut Vec<Finding>,
) {
    let cycles = new_import_cycles(
        &coupling.base,
        &coupling.head,
        &coupling.head_to_base,
        &coupling.changed,
    );
    found.extend(cycles.into_iter().map(cycle_finding));
    for file in files {
        if let Some(finding) = fan_in_finding(coupling, file, clusters, cfg) {
            found.push(finding);
        }
    }
    found.extend(blast_radius(coupling));
}

/// `New import cycle: a.rs → b.rs → a.rs.`, at the cut's source.
fn cycle_finding(cycle: CycleDelta) -> Finding {
    let chain = if cycle.witness.is_empty() {
        let mut around = cycle.members.clone();
        around.push(cycle.members[0].clone());
        around
    } else {
        cycle.witness.clone()
    };
    let path = cycle
        .cut
        .as_ref()
        .map_or(cycle.members[0].as_str(), |(from, _)| from.as_str());
    let mut finding = Finding::new(
        GateId::ImportCycle,
        path,
        format!("New import cycle: {}.", chain.join(" → ")),
    );
    if let Some((from, to)) = &cycle.cut {
        finding.fix = format!("Break the cycle at {from} → {to} (introduced by this change).");
    }
    finding.magnitude = cycle.members.len() as f64 * CYCLE_MEMBER_WEIGHT;
    finding.related = chain;
    finding.cycle = cycle.members;
    finding
}

/// More files depend on `file`, which fails SIMPLE, than the base had:
/// at least `min_new_dependents` new ones, and growth of at least that
/// many or `min_growth_percent` of the base count, whichever is larger.
///
/// A split's own children and test files are not new dependents, and
/// tests count on neither side. An added file has no base to grow from:
/// the `new_file_pillar` gate already speaks for it. Neither does a file
/// the base graph never indexed; every dependent would look new.
fn fan_in_finding(
    coupling: &Coupling,
    file: &FileRecap,
    clusters: &[Cluster],
    cfg: &PrGateConfig,
) -> Option<Finding> {
    let fails_simple = file
        .pillars
        .get("simple")
        .is_some_and(|delta| delta.after_passed == Some(false));
    let base_path = coupling.to_base(&file.path);
    if !fails_simple || file.is_new() || !coupling.base.contains(base_path) {
        return None;
    }
    let children: BTreeSet<&str> = clusters
        .iter()
        .filter(|cluster| cluster.parent == file.path)
        .flat_map(|cluster| cluster.children.iter().map(|child| child.path.as_str()))
        .collect();
    let before: BTreeSet<String> = coupling
        .to_head(coupling.base.dependents(base_path))
        .into_iter()
        .filter(|path| !is_test_path(path))
        .collect();
    let after: BTreeSet<String> = coupling
        .head
        .dependents(&file.path)
        .into_iter()
        .filter(|path| !is_test_path(path) && !children.contains(path.as_str()))
        .collect();
    let new: Vec<String> = after.difference(&before).cloned().collect();

    let threshold = cfg.fan_in_growth;
    let min_new = threshold.min_new_dependents as usize;
    let by_share = (before.len() * threshold.min_growth_percent as usize).div_ceil(100);
    let growth = after.len().saturating_sub(before.len());
    if new.len() < min_new || growth < min_new.max(by_share) {
        return None;
    }
    let mut listed = new
        .iter()
        .take(LISTED_DEPENDENTS)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if new.len() > LISTED_DEPENDENTS {
        listed.push_str(", …");
    }
    let mut finding = Finding::new(
        GateId::FanInGrowth,
        &file.path,
        format!(
            "{} new {} on {}, which fails SIMPLE: {listed}.",
            new.len(),
            if new.len() == 1 {
                "file depends"
            } else {
                "files depend"
            },
            file.path
        ),
    );
    finding.before = Some(before.len() as f64);
    finding.after = Some(after.len() as f64);
    finding.magnitude = new.len() as f64;
    finding.related = new;
    Some(finding)
}

/// `The change reaches 12 files directly, 40 transitively (most through
/// src/core.rs).`: one finding for the range, at the changed file with the
/// most transitive dependents, and none when nothing depends on the change.
/// A deleted file's dependents come from the base graph.
fn blast_radius(coupling: &Coupling) -> Option<Finding> {
    let head_changed = || coupling.changed.iter().map(String::as_str);
    let base_deleted = || coupling.deleted.iter().map(String::as_str);
    let touched: BTreeSet<String> = coupling
        .changed
        .iter()
        .chain(&coupling.to_head(coupling.deleted.iter().cloned()))
        .cloned()
        .collect();

    let mut direct: BTreeSet<String> = head_changed()
        .flat_map(|path| coupling.head.dependents(path))
        .collect();
    direct.extend(coupling.to_head(base_deleted().flat_map(|path| coupling.base.dependents(path))));
    direct.retain(|path| !touched.contains(path));
    if direct.is_empty() {
        return None;
    }
    let mut transitive = coupling.head.transitive_dependents(head_changed());
    transitive.extend(coupling.to_head(coupling.base.transitive_dependents(base_deleted())));
    transitive.retain(|path| !touched.contains(path));

    // Most transitive dependents, ties to the path that sorts first.
    let reach = |graph: &FileGraph, path: &str| graph.transitive_dependents([path]).len();
    let (_, widest) = head_changed()
        .map(|path| (reach(&coupling.head, path), path))
        .chain(base_deleted().map(|path| (reach(&coupling.base, path), path)))
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(a.1)))?;

    let mut finding = Finding::new(
        GateId::BlastRadius,
        widest,
        format!(
            "The change reaches {} directly, {} transitively (most through {widest}).",
            files(direct.len()),
            transitive.len()
        ),
    );
    finding.before = Some(direct.len() as f64);
    finding.after = Some(transitive.len() as f64);
    finding.magnitude = direct.len() as f64;
    // The direct dependents that the most files depend on in turn.
    let mut ranked: Vec<(usize, String)> = direct
        .into_iter()
        .map(|path| (coupling.head.dependents(&path).len(), path))
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    finding.related = ranked
        .into_iter()
        .take(TOP_DEPENDENTS)
        .map(|(_, path)| path)
        .collect();
    Some(finding)
}

/// `1 file`, `12 files`.
fn files(count: usize) -> String {
    format!("{count} file{}", if count == 1 { "" } else { "s" })
}

/// A test by the common layout conventions: under a `tests`, `test` or
/// `__tests__` directory, or named `*_test.*` (Go, Rust), `test_*.py`
/// (pytest), `*.test.*` or `*.spec.*` (Jest, Vitest, Mocha). Tests depend
/// on what they cover, so a new one is not fan-in worth reporting.
fn is_test_path(path: &str) -> bool {
    let mut segments: Vec<&str> = path.split('/').collect();
    let name = segments.pop().unwrap_or_default();
    if segments
        .iter()
        .any(|dir| matches!(*dir, "tests" | "test" | "__tests__"))
    {
        return true;
    }
    let stem = name.split('.').next().unwrap_or_default();
    stem.ends_with("_test")
        || (name.starts_with("test_") && name.ends_with(".py"))
        || name.contains(".test.")
        || name.contains(".spec.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_follow_the_common_conventions() {
        for path in [
            "tests/cli.rs",
            "src/test/Main.go",
            "web/__tests__/app.tsx",
            "pkg/run_test.go",
            "tools/test_split.py",
            "web/app.test.ts",
            "web/app.spec.js",
        ] {
            assert!(is_test_path(path), "{path}");
        }
        for path in [
            "src/testing.rs",
            "src/contest.py",
            "test_data.rs",
            "src/latest/app.ts",
        ] {
            assert!(!is_test_path(path), "{path}");
        }
    }
}
