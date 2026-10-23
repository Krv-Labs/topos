//! Read-only status for the current project or the known global inventory.
//!
//! Default status opens no database and never writes: it observes metadata,
//! PR markers/commits, and worktree registrations. Recursive sizing is opt-in
//! and is reported as an estimate, not as guaranteed reclaimable disk space.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::discovery::{self, graph_evidence};
use super::git;
use super::inventory::{self, CacheKind, CacheRecord, Inventory};
use super::{CacheEntryView, StructuralState, INDEX_SCHEMA};

pub type StatusEntry = CacheEntryView;

#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub schema: &'static str,
    pub command: &'static str,
    pub scope: &'static str,
    pub entries: Vec<StatusEntry>,
    pub totals: StatusTotals,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StatusTotals {
    pub entries: usize,
    pub managed: usize,
    pub estimated_bytes: u64,
    pub unavailable_sizes: usize,
}

/// Observe the current project, or every known inventory record with
/// `global`. Global still includes the current project when one exists.
pub fn status(global: bool, sizes: bool) -> Result<StatusReport, String> {
    let cwd = std::env::current_dir().map_err(|error| format!("current directory: {error}"))?;
    let inside_git = git::git_root(&cwd).is_ok();
    let current = current_project_entries(&cwd)?;
    let inventory = Inventory::open();
    let (records, mut diagnostics) = match &inventory {
        Ok(inventory) => inventory.records(),
        Err(error) => (Vec::new(), vec![error.clone()]),
    };

    if !global && current.is_empty() && !inside_git {
        return Err(
            "not inside a Git repository or a recognized non-Git Topos project\n\
             hint: run `topos cache status --global` to inspect known locations"
                .to_string(),
        );
    }

    let mut entries: BTreeMap<String, CacheEntryView> = BTreeMap::new();
    if global {
        for record in &records {
            let key = record.key();
            let entry = view_from_record(record.clone(), sizes);
            entries.insert(key, entry);
        }
    }
    for entry in current {
        let key = entry_key(&entry);
        match entries.get_mut(&key) {
            Some(existing) => merge_view(existing, entry),
            None => {
                entries.insert(key, entry);
            }
        }
    }

    // Inventory timestamps are useful even for a freshly scanned current
    // project; they are read-only here. In global scope the records were
    // already merged, so this only refreshes scanned rows.
    for record in records {
        let key = record_key(&record);
        if let Some(entry) = entries.get_mut(&key) {
            entry.last_built = max_time(entry.last_built, record.last_built);
            entry.last_used = max_time(entry.last_used, record.last_used);
            entry.last_seen = max_time(entry.last_seen, record.last_seen);
        }
    }

    let mut entries: Vec<StatusEntry> = entries.into_values().collect();
    entries.sort_by_key(CacheEntryView::sort_key);
    if sizes {
        for entry in &mut entries {
            if entry.estimated_bytes.is_none() {
                size_entry(entry);
            }
        }
    }
    let mut totals = StatusTotals {
        entries: entries.len(),
        managed: entries.iter().filter(|entry| entry.managed).count(),
        estimated_bytes: 0,
        unavailable_sizes: 0,
    };
    for entry in &entries {
        if let Some(bytes) = entry.estimated_bytes {
            totals.estimated_bytes = totals.estimated_bytes.saturating_add(bytes);
        }
        if entry.size_error.is_some() {
            totals.unavailable_sizes += 1;
        }
    }
    diagnostics.sort();
    diagnostics.dedup();
    Ok(StatusReport {
        schema: INDEX_SCHEMA,
        command: "status",
        scope: if global { "global" } else { "project" },
        entries,
        totals,
        diagnostics,
    })
}

fn current_project_entries(cwd: &Path) -> Result<Vec<CacheEntryView>, String> {
    let mut entries = Vec::new();
    if let Ok(root) = git::git_root(cwd) {
        let mut seen_common = BTreeSet::new();
        for root in discovery::repository_roots(&root) {
            let common = match git::git_common_dir(&root) {
                Ok(common) => common,
                Err(_) => continue,
            };
            if seen_common.insert(git::normalize_path(&common)) {
                if let Ok(read_dir) = std::fs::read_dir(&common) {
                    let mut prs: Vec<(u64, PathBuf)> = read_dir
                        .flatten()
                        .filter_map(|entry| {
                            let name = entry.file_name();
                            let number = name.to_str()?.strip_prefix("topos-pr-")?;
                            let pr = number.parse().ok()?;
                            if entry
                                .file_type()
                                .map(|kind| kind.is_symlink())
                                .unwrap_or(false)
                            {
                                return None;
                            }
                            Some((pr, entry.path()))
                        })
                        .collect();
                    prs.sort_by_key(|(pr, _)| *pr);
                    for (pr, parent) in prs {
                        entries.push(discovery::pr_entry(&root, &common, pr, &parent));
                    }
                }
            }
            if let Some(entry) = discovery::graph_entry(&root) {
                entries.push(entry);
            }
        }
    } else if let Some(entry) = discovery::graph_entry(cwd) {
        entries.push(entry);
    }
    Ok(entries)
}

fn view_from_record(record: CacheRecord, sizes: bool) -> CacheEntryView {
    let kind = record.kind().unwrap_or(CacheKind::Graph);
    let mut entry = match kind {
        CacheKind::Pr => match (record.common_git_dir.as_deref(), record.pr) {
            (Some(common), Some(pr)) => {
                let common = PathBuf::from(common);
                let parent = PathBuf::from(&record.path);
                let (state, managed, compatibility, diagnostics) =
                    discovery::classify_pr(&common, pr, &parent);
                CacheEntryView {
                    kind: "pr".to_string(),
                    path: record.path.clone(),
                    project_root: record.project_root.clone(),
                    common_git_dir: Some(record.common_git_dir.clone().unwrap_or_default()),
                    pr: Some(pr),
                    managed,
                    state: state.as_str().to_string(),
                    last_built: record.last_built,
                    last_used: record.last_used,
                    last_seen: record.last_seen,
                    compatibility,
                    estimated_bytes: None,
                    size_error: None,
                    diagnostics,
                }
            }
            _ => missing_view(&record, "PR record lacks a common directory or slot"),
        },
        CacheKind::Graph => {
            let path = Path::new(&record.path);
            let (state, diagnostics) = if !path.exists() {
                (StructuralState::Missing, Vec::new())
            } else if graph_evidence(path) {
                (StructuralState::Ready, Vec::new())
            } else {
                (
                    StructuralState::Corrupt,
                    vec!["path exists but contains no index evidence".to_string()],
                )
            };
            CacheEntryView {
                kind: "graph".to_string(),
                path: record.path.clone(),
                project_root: record.project_root.clone(),
                common_git_dir: record.common_git_dir.clone(),
                pr: None,
                managed: false,
                state: state.as_str().to_string(),
                last_built: record.last_built,
                last_used: record.last_used,
                last_seen: record.last_seen,
                compatibility: record.compatibility.clone(),
                estimated_bytes: None,
                size_error: None,
                diagnostics,
            }
        }
    };
    if sizes {
        size_entry(&mut entry);
    }
    entry
}

fn missing_view(record: &CacheRecord, note: &str) -> CacheEntryView {
    CacheEntryView {
        kind: record.kind.as_str().to_string(),
        path: record.path.clone(),
        project_root: record.project_root.clone(),
        common_git_dir: record.common_git_dir.clone(),
        pr: record.pr,
        managed: false,
        state: StructuralState::Missing.as_str().to_string(),
        last_built: record.last_built,
        last_used: record.last_used,
        last_seen: record.last_seen,
        compatibility: record.compatibility.clone(),
        estimated_bytes: None,
        size_error: None,
        diagnostics: vec![note.to_string()],
    }
}

fn record_key(record: &CacheRecord) -> String {
    record.key()
}

fn entry_key(entry: &CacheEntryView) -> String {
    let kind = if entry.kind == "pr" {
        CacheKind::Pr
    } else {
        CacheKind::Graph
    };
    inventory::key_for(kind, entry.common_git_dir.as_deref(), entry.pr, &entry.path)
}

fn merge_view(existing: &mut CacheEntryView, incoming: CacheEntryView) {
    existing.managed |= incoming.managed;
    existing.last_built = max_time(existing.last_built, incoming.last_built);
    existing.last_used = max_time(existing.last_used, incoming.last_used);
    existing.last_seen = max_time(existing.last_seen, incoming.last_seen);
    existing.state = incoming.state;
    for diagnostic in incoming.diagnostics {
        if !existing.diagnostics.contains(&diagnostic) {
            existing.diagnostics.push(diagnostic);
        }
    }
    if incoming.compatibility.is_some() {
        existing.compatibility = incoming.compatibility;
    }
    if existing.estimated_bytes.is_none() {
        existing.estimated_bytes = incoming.estimated_bytes;
        existing.size_error = incoming.size_error;
    }
}

fn max_time(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Recursive estimate that never follows symlinks and tolerates unreadable
/// subtrees: the readable total is still reported, with a warning.
pub fn size_entry(entry: &mut CacheEntryView) {
    let (bytes, error) = estimate_bytes(Path::new(&entry.path));
    entry.estimated_bytes = Some(bytes);
    entry.size_error = error;
}

fn estimate_bytes(path: &Path) -> (u64, Option<String>) {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return (0, Some(format!("reading {}: {error}", path.display())));
        }
    };
    if metadata.file_type().is_symlink() {
        return (0, None);
    }
    if metadata.is_file() {
        return (metadata.len(), None);
    }
    if !metadata.is_dir() {
        return (0, None);
    }
    let mut total = 0u64;
    let mut first_error = None;
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            return (0, Some(format!("reading {}: {error}", path.display())));
        }
    };
    for entry in entries {
        match entry {
            Ok(entry) => {
                let (bytes, error) = estimate_bytes(&entry.path());
                total = total.saturating_add(bytes);
                if first_error.is_none() {
                    first_error = error;
                }
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(format!("reading {}: {error}", path.display()));
                }
            }
        }
    }
    (total, first_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizing_skips_symlinks_and_reports_errors() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), vec![0u8; 10]).unwrap();
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/b"), vec![0u8; 5]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", dir.path().join("link")).unwrap();
        let (total, error) = estimate_bytes(dir.path());
        assert!(total >= 15);
        assert!(error.is_none());
        let (_, error) = estimate_bytes(&dir.path().join("missing"));
        assert!(error.is_some());
    }

    #[test]
    fn merge_view_deduplicates_diagnostics_preserving_order() {
        let mut existing = CacheEntryView {
            kind: "pr".to_string(),
            path: "/repo/.git/topos-pr-1".to_string(),
            project_root: "/repo".to_string(),
            common_git_dir: Some("/repo/.git".to_string()),
            pr: Some(1),
            managed: false,
            state: "legacy-ambiguous".to_string(),
            last_built: None,
            last_used: None,
            last_seen: Some(10.0),
            compatibility: None,
            estimated_bytes: None,
            size_error: None,
            diagnostics: vec!["legacy store has no ownership marker".to_string()],
        };
        let incoming = CacheEntryView {
            diagnostics: vec![
                "legacy store has no ownership marker".to_string(),
                "second note".to_string(),
            ],
            ..existing.clone()
        };
        merge_view(&mut existing, incoming);
        assert_eq!(
            existing.diagnostics,
            vec![
                "legacy store has no ownership marker".to_string(),
                "second note".to_string(),
            ]
        );
    }
}
