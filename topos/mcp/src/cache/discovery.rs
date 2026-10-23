//! Bounded, read-only discovery of existing Topos/GitNexus data.
//!
//! A supplied path is either a repository/worktree root, or a directory whose
//! immediate non-symlink children are candidates. Nothing recurses beyond one
//! level, no Git metadata is mutated, and `--dry-run` never writes inventory.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::git;
use super::inventory::{
    canonical_string, now, CacheCompatibility, CacheKind, Inventory, RecordUpdate,
};
use super::marker::{self, DELETE_MARKER_FILE};
use super::{CacheEntryView, StructuralState, INDEX_SCHEMA};

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "vendor",
    "dist",
    "build",
    "out",
    ".venv",
    "venv",
    "__pycache__",
    ".gradle",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
];

#[derive(Debug, Clone, Serialize)]
pub struct DiscoverReport {
    pub schema: &'static str,
    pub command: &'static str,
    pub dry_run: bool,
    pub entries: Vec<CacheEntryView>,
    pub totals: DiscoverTotals,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DiscoverTotals {
    pub entries: usize,
    pub managed: usize,
    pub candidates: usize,
}

/// Discover existing caches under `paths`. With `dry_run` nothing is written.
pub fn discover(paths: &[PathBuf], dry_run: bool) -> Result<DiscoverReport, String> {
    if paths.is_empty() {
        return Err("pass at least one path to discover".to_string());
    }
    let mut diagnostics = Vec::new();
    let mut entries: BTreeMap<String, CacheEntryView> = BTreeMap::new();
    for raw in paths {
        let path =
            std::fs::canonicalize(raw).map_err(|error| format!("{}: {error}", raw.display()))?;
        if !path.is_dir() {
            return Err(format!("Not a directory: {}", raw.display()));
        }
        let mut found = Vec::new();
        if is_repository_root(&path) {
            inspect_repository(&path, &mut found, &mut diagnostics);
        } else {
            let mut children: Vec<PathBuf> = std::fs::read_dir(&path)
                .map_err(|error| format!("reading {}: {error}", path.display()))?
                .flatten()
                .filter(|entry| {
                    entry
                        .file_type()
                        .map(|kind| kind.is_dir() && !kind.is_symlink())
                        .unwrap_or(false)
                })
                .map(|entry| entry.path())
                .filter(|child| {
                    !child
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| SKIP_DIRS.contains(&name))
                })
                .collect();
            children.sort();
            for child in children {
                if is_repository_root(&child) {
                    inspect_repository(&child, &mut found, &mut diagnostics);
                }
            }
            if let Some(entry) = graph_entry(&path) {
                found.push(entry);
            }
        }
        for entry in found {
            let key = if entry.kind == "pr" {
                format!(
                    "pr\0{}\0{}",
                    entry.common_git_dir.as_deref().unwrap_or_default(),
                    entry.pr.unwrap_or_default()
                )
            } else {
                format!("graph\0{}", entry.path)
            };
            match entries.get_mut(&key) {
                Some(existing) => {
                    existing.managed |= entry.managed;
                    existing.diagnostics.extend(entry.diagnostics);
                }
                None => {
                    entries.insert(key, entry);
                }
            }
        }
    }

    if !dry_run {
        match Inventory::open() {
            Ok(inventory) => {
                for entry in entries.values() {
                    let kind = if entry.kind == "pr" {
                        CacheKind::Pr
                    } else {
                        CacheKind::Graph
                    };
                    let update = RecordUpdate {
                        kind,
                        managed: entry.managed,
                        project_root: entry.project_root.clone(),
                        common_git_dir: entry.common_git_dir.clone(),
                        path: entry.path.clone(),
                        pr: entry.pr,
                        last_built: None,
                        last_used: None,
                        last_seen: Some(now()),
                        compatibility: entry.compatibility.clone(),
                    };
                    if let Err(error) = inventory.upsert(update) {
                        diagnostics.push(format!(
                            "could not record discovered {} {}: {error}",
                            entry.kind, entry.path
                        ));
                    }
                }
            }
            Err(error) => diagnostics.push(format!(
                "could not open the cache inventory; discovered entries were not recorded: {error}"
            )),
        }
    }

    diagnostics.sort();
    diagnostics.dedup();
    let mut entries: Vec<CacheEntryView> = entries.into_values().collect();
    entries.sort_by_key(CacheEntryView::sort_key);
    let totals = DiscoverTotals {
        entries: entries.len(),
        managed: entries.iter().filter(|entry| entry.managed).count(),
        candidates: entries
            .iter()
            .filter(|entry| entry.state != StructuralState::Missing.as_str())
            .count(),
    };
    Ok(DiscoverReport {
        schema: INDEX_SCHEMA,
        command: "discover",
        dry_run,
        entries,
        totals,
        diagnostics,
    })
}

fn is_repository_root(path: &Path) -> bool {
    git::git_root(path)
        .map(|root| git::normalize_path(&root) == git::normalize_path(path))
        .unwrap_or(false)
}

fn inspect_repository(root: &Path, found: &mut Vec<CacheEntryView>, diagnostics: &mut Vec<String>) {
    let common = match git::git_common_dir(root) {
        Ok(common) => common,
        Err(error) => {
            diagnostics.push(format!("{}: {error}", root.display()));
            return;
        }
    };
    let mut prs = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&common) {
        for entry in read_dir.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(number) = name.strip_prefix("topos-pr-") else {
                continue;
            };
            let Ok(pr) = number.parse::<u64>() else {
                continue;
            };
            if entry
                .file_type()
                .map(|kind| kind.is_symlink())
                .unwrap_or(false)
            {
                diagnostics.push(format!(
                    "skipping symlinked PR store {}",
                    entry.path().display()
                ));
                continue;
            }
            prs.push((pr, entry.path()));
        }
    }
    prs.sort_by_key(|(pr, _)| *pr);
    for (pr, parent) in prs {
        found.push(pr_entry(root, &common, pr, &parent));
    }
    if let Some(entry) = graph_entry(root) {
        found.push(entry);
    }
}

pub(super) fn graph_entry(project_root: &Path) -> Option<CacheEntryView> {
    let store = project_root.join(".gitnexus");
    if !graph_evidence(&store) {
        return None;
    }
    let common = project_root
        .join(".git")
        .exists()
        .then(|| git::git_common_dir(project_root).ok())
        .flatten();
    Some(CacheEntryView {
        kind: "graph".to_string(),
        path: canonical_string(&store),
        project_root: canonical_string(project_root),
        common_git_dir: common.as_deref().map(canonical_string),
        pr: None,
        managed: false,
        state: StructuralState::Ready.as_str().to_string(),
        last_built: None,
        last_used: None,
        last_seen: None,
        compatibility: None,
        estimated_bytes: None,
        size_error: None,
        diagnostics: Vec::new(),
    })
}

/// Actual index evidence, without opening Ladybug.
pub(super) fn graph_evidence(store: &Path) -> bool {
    if !store.is_dir()
        || std::fs::symlink_metadata(store)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(true)
    {
        return false;
    }
    if store.join("lbug").exists() {
        return true;
    }
    let branches = store.join("branches");
    std::fs::read_dir(branches)
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.path().join("lbug").exists())
        })
        .unwrap_or(false)
}

pub(super) fn pr_entry(
    project_root: &Path,
    common: &Path,
    pr: u64,
    parent: &Path,
) -> CacheEntryView {
    let (state, managed, compatibility, diagnostics) = classify_pr(common, pr, parent);
    CacheEntryView {
        kind: "pr".to_string(),
        path: canonical_string(parent),
        project_root: canonical_string(project_root),
        common_git_dir: Some(canonical_string(common)),
        pr: Some(pr),
        managed,
        state: state.as_str().to_string(),
        last_built: None,
        last_used: None,
        last_seen: None,
        compatibility,
        estimated_bytes: None,
        size_error: None,
        diagnostics,
    }
}

/// Structural classification shared by discovery and status. It never
/// mutates the store and never adopts a legacy store.
pub(super) fn classify_pr(
    common: &Path,
    pr: u64,
    parent: &Path,
) -> (
    StructuralState,
    bool,
    Option<CacheCompatibility>,
    Vec<String>,
) {
    let mut diagnostics = Vec::new();
    let expected = common.join(format!("topos-pr-{pr}"));
    if git::normalize_path(parent) != git::normalize_path(&expected) {
        diagnostics.push(format!(
            "PR store {} is not the expected slot {}",
            parent.display(),
            expected.display()
        ));
        return (StructuralState::Corrupt, false, None, diagnostics);
    }
    let metadata = match std::fs::symlink_metadata(parent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (StructuralState::Missing, false, None, diagnostics);
        }
        Err(error) => {
            diagnostics.push(format!("reading {}: {error}", parent.display()));
            return (StructuralState::Corrupt, false, None, diagnostics);
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        diagnostics.push(format!("{} is not a real directory", parent.display()));
        return (StructuralState::Corrupt, false, None, diagnostics);
    }

    let deleting = parent.join(DELETE_MARKER_FILE).exists();
    match marker::read_marker(parent) {
        Err(error) => {
            diagnostics.push(error);
            (StructuralState::Corrupt, false, None, diagnostics)
        }
        Ok(Some(marker)) => {
            if let Err(error) = marker::validate_marker(&marker, common, pr) {
                diagnostics.push(error);
                return (StructuralState::Corrupt, false, None, diagnostics);
            }
            let compatibility = Some(CacheCompatibility {
                layout: marker.layout,
                generator: marker.generator.clone(),
            });
            if deleting {
                diagnostics.push("deletion is already in progress".to_string());
                return (
                    StructuralState::Incomplete,
                    true,
                    compatibility,
                    diagnostics,
                );
            }
            let Some(commits) = read_commits(parent) else {
                diagnostics.push(
                    "marked store has no valid two-commit record; it may be a partial build"
                        .to_string(),
                );
                return (
                    StructuralState::Incomplete,
                    true,
                    compatibility,
                    diagnostics,
                );
            };
            let mut side_diagnostic = None;
            for side in ["base", "head"] {
                let path = parent.join(side);
                if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                    if metadata.file_type().is_symlink() {
                        side_diagnostic = Some(format!("{} is a symlink", path.display()));
                    }
                }
            }
            if let Some(error) = side_diagnostic {
                diagnostics.push(error);
                return (StructuralState::Corrupt, true, compatibility, diagnostics);
            }
            let base_built = graph_evidence(&parent.join("base").join(".gitnexus"));
            let head_built = graph_evidence(&parent.join("head").join(".gitnexus"));
            if base_built && head_built && !commits.0.is_empty() && !commits.1.is_empty() {
                if let Some(state) =
                    verify_registrations(common, parent, &commits, &mut diagnostics)
                {
                    return (state, true, compatibility, diagnostics);
                }
                return (StructuralState::Ready, true, compatibility, diagnostics);
            }
            diagnostics.push(format!(
                "commits record {}..{} but {} side(s) lack an indexed graph",
                commits.0,
                commits.1,
                usize::from(!base_built) + usize::from(!head_built)
            ));
            (
                StructuralState::Incomplete,
                true,
                compatibility,
                diagnostics,
            )
        }
        Ok(None) => {
            if deleting {
                diagnostics.push("deletion is in progress on an unmarked store".to_string());
                return (StructuralState::Incomplete, false, None, diagnostics);
            }
            let Some(commits) = read_commits(parent) else {
                diagnostics.push(
                    "no ownership marker and no valid commits record; report-only".to_string(),
                );
                return (StructuralState::Orphaned, false, None, diagnostics);
            };
            let base_side = parent.join("base");
            let head_side = parent.join("head");
            let mut valid = true;
            for side in [&base_side, &head_side] {
                match std::fs::symlink_metadata(side) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                    _ => valid = false,
                }
            }
            if valid {
                diagnostics.push(format!(
                    "legacy store for {}..{} has no ownership marker; report-only until consumed or explicitly cleaned",
                    commits.0, commits.1
                ));
            } else {
                diagnostics.push("legacy layout is incomplete or unsafe; report-only".to_string());
            }
            (StructuralState::LegacyAmbiguous, false, None, diagnostics)
        }
    }
}

fn verify_registrations(
    common: &Path,
    parent: &Path,
    commits: &(String, String),
    diagnostics: &mut Vec<String>,
) -> Option<StructuralState> {
    let repo = common.parent().unwrap_or(common);
    let registrations = match git::worktree_registrations(repo) {
        Ok(registrations) => registrations,
        Err(error) => {
            diagnostics.push(format!("could not verify worktree registrations: {error}"));
            return Some(StructuralState::Corrupt);
        }
    };
    for (side, sha) in [("base", &commits.0), ("head", &commits.1)] {
        let path = parent.join(side);
        let registration = registrations.iter().find(|registration| {
            git::normalize_path(&registration.path) == git::normalize_path(&path)
        });
        let Some(registration) = registration else {
            diagnostics.push(format!(
                "{} is not registered as a worktree of {}",
                path.display(),
                repo.display()
            ));
            return Some(StructuralState::Corrupt);
        };
        if registration.locked {
            diagnostics.push(format!("{} is a locked Git worktree", path.display()));
            return Some(StructuralState::InUse);
        }
        if registration.branch.is_some() || !registration.detached {
            diagnostics.push(format!(
                "{} is attached to a branch instead of a detached commit",
                path.display()
            ));
            return Some(StructuralState::Corrupt);
        }
        let matches = registration
            .head
            .as_deref()
            .is_some_and(|head| head == sha || head.starts_with(sha) || sha.starts_with(head));
        if !matches {
            diagnostics.push(format!(
                "{} is registered at {} but commits records {sha}",
                path.display(),
                registration.head.as_deref().unwrap_or("no HEAD")
            ));
            return Some(StructuralState::Corrupt);
        }
    }
    None
}

fn read_commits(parent: &Path) -> Option<(String, String)> {
    let path = parent.join("commits");
    if std::fs::symlink_metadata(&path)
        .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
        .unwrap_or(true)
    {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let base = lines.next()?.trim().to_string();
    let head = lines.next()?.trim().to_string();
    let safe = |sha: &str| {
        (7..=64).contains(&sha.len()) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    (safe(&base) && safe(&head)).then_some((base, head))
}

/// Deduplicated repository roots for a status scope.
pub(super) fn repository_roots(root: &Path) -> BTreeSet<PathBuf> {
    let mut roots = BTreeSet::new();
    roots.insert(git::normalize_path(root));
    if let Ok(registrations) = git::worktree_registrations(root) {
        for registration in registrations {
            if !registration.prunable && registration.path.is_dir() {
                roots.insert(git::normalize_path(&registration.path));
            }
        }
    }
    roots
}
