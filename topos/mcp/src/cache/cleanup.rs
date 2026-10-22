//! Deletion and age-based pruning for managed pull-request stores.
//!
//! Every candidate is validated twice: once before the deletion marker is
//! written, and again under the repository worktree lock immediately before
//! Git is asked to remove a side. Cleanup removes only the exact
//! `<common>/topos-pr-<N>` slot, only content it can prove is disposable, and
//! never touches leases, the worktree lock, a project `.gitnexus/`, GitNexus
//! snapshots, or the GitNexus registry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::git;
use super::inventory::{canonical_string, CacheKind, CacheRecord, Inventory};
use super::lease::{LeaseError, PrLease, RepositoryLock};
use super::marker::{self, PrStoreDeleteMarker};
use super::ownership::{self, StoreIdentity};
use super::INDEX_SCHEMA;

/// The minimum grace period for `prune`, regardless of `--older-than`.
const MIN_PRUNE_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy)]
pub struct CleanOptions {
    pub pr: Option<u64>,
    pub all_prs: bool,
    pub global: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PruneOptions {
    pub older_than: Duration,
    pub global: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceReport {
    pub schema: &'static str,
    pub command: &'static str,
    pub dry_run: bool,
    pub entries: Vec<MaintenanceEntry>,
    pub totals: MaintenanceTotals,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceEntry {
    pub kind: String,
    pub path: String,
    pub pr: Option<u64>,
    pub action: String,
    pub reason: Option<String>,
    pub estimated_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MaintenanceTotals {
    pub candidates: usize,
    pub removed: usize,
    pub skipped: usize,
    pub partial: usize,
    pub busy: usize,
    pub unsafe_skipped: usize,
    pub estimated_bytes: u64,
}

#[derive(Debug, Clone)]
struct Candidate {
    common: PathBuf,
    pr: u64,
    path: PathBuf,
    record: Option<CacheRecord>,
}

#[derive(Debug, Clone, Copy, Default)]
struct Flags {
    busy: bool,
    unsafe_skipped: bool,
}

/// Parse a duration written as `<number><s|m|h|d|w>`.
pub fn parse_duration(raw: &str) -> Result<Duration, String> {
    let trimmed = raw.trim();
    let Some((index, unit)) = trimmed.char_indices().next_back() else {
        return Err("duration is empty (expected e.g. 24h or 7d)".to_string());
    };
    let number = trimmed[..index].trim();
    if number.is_empty() {
        return Err(format!(
            "duration '{raw}' has no number (expected e.g. 24h or 7d)"
        ));
    }
    let value: u64 = number
        .parse()
        .map_err(|_| format!("invalid duration '{raw}' (expected e.g. 24h or 7d)"))?;
    let seconds = match unit.to_ascii_lowercase() {
        's' => 1,
        'm' => 60,
        'h' => 60 * 60,
        'd' => 24 * 60 * 60,
        'w' => 7 * 24 * 60 * 60,
        other => {
            return Err(format!(
                "invalid duration unit '{other}' (expected s, m, h, d, or w)"
            ));
        }
    };
    value
        .checked_mul(seconds)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("duration '{raw}' is too large"))
}

/// Remove one managed PR store, or all managed PR stores in scope.
pub fn clean(options: CleanOptions) -> Result<MaintenanceReport, String> {
    let cwd = std::env::current_dir().map_err(|error| format!("current directory: {error}"))?;
    let (inventory, inventory_error) = match Inventory::open() {
        Ok(inventory) => (Some(inventory), None),
        Err(error) => (None, Some(error)),
    };
    let mut report = clean_with(options, &cwd, inventory.as_ref())?;
    if let Some(error) = inventory_error {
        report
            .diagnostics
            .push(format!("cache inventory unavailable: {error}"));
    }
    Ok(report)
}

/// Remove managed PR stores whose newest activity is at least the requested
/// age old, never less than [`MIN_PRUNE_GRACE`].
pub fn prune(options: PruneOptions) -> Result<MaintenanceReport, String> {
    let cwd = std::env::current_dir().map_err(|error| format!("current directory: {error}"))?;
    let (inventory, inventory_error) = match Inventory::open() {
        Ok(inventory) => (Some(inventory), None),
        Err(error) => (None, Some(error)),
    };
    let mut report = prune_with(options, &cwd, inventory.as_ref())?;
    if let Some(error) = inventory_error {
        report
            .diagnostics
            .push(format!("cache inventory unavailable: {error}"));
    }
    Ok(report)
}

fn clean_with(
    options: CleanOptions,
    cwd: &Path,
    inventory: Option<&Inventory>,
) -> Result<MaintenanceReport, String> {
    if options.pr.is_some() && options.all_prs {
        return Err("pass --pr or --all-prs, not both".to_string());
    }
    if options.pr.is_none() && !options.all_prs {
        return Err("pass --pr <N> or --all-prs".to_string());
    }
    let (records, mut diagnostics) = inventory_records(inventory);
    let candidates = collect_candidates(
        cwd,
        options.global,
        options.pr,
        options.all_prs,
        &records,
        &mut diagnostics,
    )?;
    let mut entries = Vec::new();
    let mut flags = Vec::new();
    for candidate in candidates {
        let (entry, entry_flags) =
            clean_candidate(candidate, options.dry_run, inventory, &mut diagnostics);
        entries.push(entry);
        flags.push(entry_flags);
    }
    Ok(finish_report(
        "clean",
        options.dry_run,
        entries,
        flags,
        diagnostics,
    ))
}

fn prune_with(
    options: PruneOptions,
    cwd: &Path,
    inventory: Option<&Inventory>,
) -> Result<MaintenanceReport, String> {
    let (records, mut diagnostics) = inventory_records(inventory);
    let candidates =
        collect_candidates(cwd, options.global, None, true, &records, &mut diagnostics)?;
    let mut entries = Vec::new();
    let mut flags = Vec::new();
    for candidate in candidates {
        let (entry, entry_flags) =
            prune_candidate(candidate, &options, inventory, &mut diagnostics);
        entries.push(entry);
        flags.push(entry_flags);
    }
    Ok(finish_report(
        "prune",
        options.dry_run,
        entries,
        flags,
        diagnostics,
    ))
}

fn inventory_records(inventory: Option<&Inventory>) -> (Vec<CacheRecord>, Vec<String>) {
    match inventory {
        Some(inventory) => inventory.records(),
        None => (Vec::new(), Vec::new()),
    }
}

fn finish_report(
    command: &'static str,
    dry_run: bool,
    entries: Vec<MaintenanceEntry>,
    flags: Vec<Flags>,
    mut diagnostics: Vec<String>,
) -> MaintenanceReport {
    // Pair flags with their entries before sorting. Sorting the two parallel
    // vectors separately would attribute a busy or unsafe result to whichever
    // PR happened to land at that index.
    let mut entries: Vec<_> = entries.into_iter().zip(flags).collect();
    entries.sort_by(|(a, _), (b, _)| a.pr.cmp(&b.pr).then_with(|| a.path.cmp(&b.path)));
    let mut totals = MaintenanceTotals {
        candidates: entries.len(),
        ..MaintenanceTotals::default()
    };
    for (entry, flag) in &entries {
        match entry.action.as_str() {
            "removed" => totals.removed += 1,
            "skipped" => totals.skipped += 1,
            "partial" => totals.partial += 1,
            _ => {}
        }
        if flag.busy {
            totals.busy += 1;
        }
        if flag.unsafe_skipped {
            totals.unsafe_skipped += 1;
        }
        if matches!(
            entry.action.as_str(),
            "removed" | "would-remove" | "partial"
        ) {
            totals.estimated_bytes = totals
                .estimated_bytes
                .saturating_add(entry.estimated_bytes.unwrap_or(0));
        }
    }
    diagnostics.sort();
    diagnostics.dedup();
    let entries = entries.into_iter().map(|(entry, _)| entry).collect();
    MaintenanceReport {
        schema: INDEX_SCHEMA,
        command,
        dry_run,
        entries,
        totals,
        diagnostics,
    }
}

fn collect_candidates(
    cwd: &Path,
    global: bool,
    only_pr: Option<u64>,
    all_prs: bool,
    records: &[CacheRecord],
    diagnostics: &mut Vec<String>,
) -> Result<Vec<Candidate>, String> {
    let current = current_common(cwd);
    if current.is_none() && !global {
        return Err(
            "not inside a Git repository\nhint: run with --global to use the cache inventory"
                .to_string(),
        );
    }

    let mut by_key: BTreeMap<(String, u64), Candidate> = BTreeMap::new();
    if let Some(common) = current.as_deref() {
        if let Some(pr) = only_pr {
            let identity = StoreIdentity::expected(common, pr);
            by_key.insert(
                key_for(common, pr),
                Candidate {
                    common: identity.common_dir,
                    pr,
                    path: identity.parent,
                    record: None,
                },
            );
        } else if all_prs {
            match std::fs::read_dir(common) {
                Ok(read_dir) => {
                    let mut prs = Vec::new();
                    for entry in read_dir.flatten() {
                        let name = entry.file_name();
                        let Some(name) = name.to_str() else {
                            continue;
                        };
                        let Some(number) = name.strip_prefix("topos-pr-") else {
                            continue;
                        };
                        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
                            continue;
                        }
                        let Ok(pr) = number.parse::<u64>() else {
                            continue;
                        };
                        prs.push((pr, entry.path()));
                    }
                    prs.sort_by_key(|(pr, _)| *pr);
                    for (pr, path) in prs {
                        by_key.insert(
                            key_for(common, pr),
                            Candidate {
                                common: common.to_path_buf(),
                                pr,
                                path,
                                record: None,
                            },
                        );
                    }
                }
                Err(error) => diagnostics.push(format!(
                    "reading PR stores in {}: {error}",
                    common.display()
                )),
            }
        }
    }

    for record in records {
        if record.kind() != Some(CacheKind::Pr) {
            continue;
        }
        let (Some(common), Some(pr)) = (record.common_git_dir.as_deref(), record.pr) else {
            continue;
        };
        if only_pr.is_some_and(|wanted| wanted != pr) {
            continue;
        }
        let common = git::normalize_path(Path::new(common));
        let key = key_for(&common, pr);
        if let Some(existing) = by_key.get_mut(&key) {
            existing.record = Some(record.clone());
            continue;
        }
        if !global {
            continue;
        }
        let identity = StoreIdentity::expected(&common, pr);
        let recorded = git::normalize_path(Path::new(&record.path));
        if recorded != identity.parent {
            diagnostics.push(format!(
                "inventory record for PR {pr} points at {} instead of {}",
                recorded.display(),
                identity.parent.display()
            ));
        }
        by_key.insert(
            key,
            Candidate {
                common,
                pr,
                path: identity.parent,
                record: Some(record.clone()),
            },
        );
    }

    if only_pr.is_some() && by_key.is_empty() {
        diagnostics.push(format!(
            "no cache inventory record for PR {}",
            only_pr.unwrap_or_default()
        ));
    }
    let mut candidates: Vec<Candidate> = by_key.into_values().collect();
    candidates.sort_by(|a, b| a.pr.cmp(&b.pr).then_with(|| a.path.cmp(&b.path)));
    Ok(candidates)
}

fn key_for(common: &Path, pr: u64) -> (String, u64) {
    (canonical_string(common), pr)
}

fn current_common(cwd: &Path) -> Option<PathBuf> {
    let root = git::git_root(cwd).ok()?;
    git::git_common_dir(&root).ok()
}

fn clean_candidate(
    candidate: Candidate,
    dry_run: bool,
    inventory: Option<&Inventory>,
    diagnostics: &mut Vec<String>,
) -> (MaintenanceEntry, Flags) {
    let mut flags = Flags::default();
    let mut entry = entry_for(&candidate, "skipped", None, None);

    let identity = match ownership::identity(&candidate.common, candidate.pr, &candidate.path) {
        Ok(identity) => identity,
        Err(error) => {
            flags.unsafe_skipped = true;
            entry.reason = Some(error);
            return (entry, flags);
        }
    };

    match std::fs::symlink_metadata(&identity.parent) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            entry.action = "none".to_string();
            entry.reason = Some("no such PR store".to_string());
            if !dry_run {
                remove_inventory(inventory, &identity, diagnostics);
            }
            return (entry, flags);
        }
        Err(error) => {
            flags.unsafe_skipped = true;
            entry.reason = Some(format!("reading {}: {error}", identity.parent.display()));
            return (entry, flags);
        }
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            flags.unsafe_skipped = true;
            entry.reason = Some(format!(
                "{} is not a real directory",
                identity.parent.display()
            ));
            return (entry, flags);
        }
        Ok(_) => {}
    }
    entry.estimated_bytes = Some(estimate_bytes(&identity.parent));

    let lease = match PrLease::try_acquire(&identity.common_dir, candidate.pr) {
        Ok(lease) => lease,
        Err(LeaseError::Busy) => {
            flags.busy = true;
            entry.reason = Some("in use by another process".to_string());
            return (entry, flags);
        }
        Err(error) => {
            flags.unsafe_skipped = true;
            entry.reason = Some(format!("could not acquire the PR lease: {error}"));
            return (entry, flags);
        }
    };
    // Re-check after owning the lease: the slot may have disappeared while we
    // waited for a non-blocking acquisition.
    if !identity.parent.exists() {
        entry.action = "none".to_string();
        entry.reason = Some("no such PR store".to_string());
        drop(lease);
        if !dry_run {
            remove_inventory(inventory, &identity, diagnostics);
        }
        return (entry, flags);
    }

    let repo_root = ownership::repo_for_common(&identity.common_dir);
    let (marker, adopted) = match marker::read_marker(&identity.parent) {
        Err(error) => {
            flags.unsafe_skipped = true;
            entry.reason = Some(error);
            return (entry, flags);
        }
        Ok(Some(marker)) => {
            match marker::validate_marker(&marker, &identity.common_dir, candidate.pr) {
                Ok(()) => (Some(marker), false),
                Err(error) => {
                    flags.unsafe_skipped = true;
                    entry.reason = Some(error);
                    return (entry, flags);
                }
            }
        }
        Ok(None) => match ownership::legacy_store(
            &identity.common_dir,
            &repo_root,
            candidate.pr,
            &identity.parent,
        ) {
            Ok(_) => {
                if dry_run {
                    (None, true)
                } else {
                    // Validate the checkout before claiming it. Adoption is
                    // durable bookkeeping; a store with user content or a
                    // hostile registration must stay unowned.
                    let content = ownership::assess_content(
                        &identity.common_dir,
                        candidate.pr,
                        &identity.parent,
                        &repo_root,
                    );
                    if !content.safe() {
                        flags.unsafe_skipped = true;
                        entry.reason = content.reason();
                        return (entry, flags);
                    }
                    match marker::write_marker(
                        &identity.parent,
                        &identity.common_dir,
                        &repo_root,
                        candidate.pr,
                        Some("topos-mcp".to_string()),
                    ) {
                        Ok(marker) => (Some(marker), true),
                        Err(error) => {
                            entry.action = "partial".to_string();
                            entry.reason =
                                Some(format!("could not adopt the legacy store: {error}"));
                            return (entry, flags);
                        }
                    }
                }
            }
            Err(error) => {
                flags.unsafe_skipped = true;
                entry.reason = Some(format!(
                    "no ownership marker and not a valid legacy store: {error}"
                ));
                return (entry, flags);
            }
        },
    };

    if let Err(error) = validate_delete_marker_if_present(&identity) {
        flags.unsafe_skipped = true;
        entry.reason = Some(error);
        return (entry, flags);
    }

    let content = ownership::assess_content(
        &identity.common_dir,
        candidate.pr,
        &identity.parent,
        &repo_root,
    );
    if !content.safe() {
        flags.unsafe_skipped = true;
        entry.reason = content.reason();
        return (entry, flags);
    }

    if dry_run {
        entry.action = "would-remove".to_string();
        let mut reason = if adopted {
            "valid legacy store; would adopt and remove".to_string()
        } else {
            "managed PR store; would remove".to_string()
        };
        if !content.unknown_entries.is_empty() {
            reason.push_str(&format!(
                "; would leave unknown entries: {}",
                content.unknown_entries.join(", ")
            ));
        }
        entry.reason = Some(reason);
        return (entry, flags);
    }

    let project_root = marker
        .as_ref()
        .map(|marker| PathBuf::from(&marker.project_root))
        .filter(|path| path.is_dir())
        .unwrap_or(repo_root);
    let (action, reason) = delete_managed(&identity, &project_root, inventory, diagnostics);
    entry.action = action;
    entry.reason = reason;
    (entry, flags)
}

fn prune_candidate(
    candidate: Candidate,
    options: &PruneOptions,
    inventory: Option<&Inventory>,
    diagnostics: &mut Vec<String>,
) -> (MaintenanceEntry, Flags) {
    let mut flags = Flags::default();
    let mut entry = entry_for(&candidate, "skipped", None, None);
    let identity = match ownership::identity(&candidate.common, candidate.pr, &candidate.path) {
        Ok(identity) => identity,
        Err(error) => {
            flags.unsafe_skipped = true;
            entry.reason = Some(error);
            return (entry, flags);
        }
    };
    match std::fs::symlink_metadata(&identity.parent) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            entry.action = "none".to_string();
            entry.reason = Some("no such PR store".to_string());
            if !options.dry_run {
                remove_inventory(inventory, &identity, diagnostics);
            }
            return (entry, flags);
        }
        Err(error) => {
            flags.unsafe_skipped = true;
            entry.reason = Some(format!("reading {}: {error}", identity.parent.display()));
            return (entry, flags);
        }
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            flags.unsafe_skipped = true;
            entry.reason = Some(format!(
                "{} is not a real directory",
                identity.parent.display()
            ));
            return (entry, flags);
        }
        Ok(_) => {}
    }
    entry.estimated_bytes = Some(estimate_bytes(&identity.parent));

    let _marker = match marker::read_marker(&identity.parent) {
        Err(error) => {
            flags.unsafe_skipped = true;
            entry.reason = Some(error);
            return (entry, flags);
        }
        Ok(None) => {
            entry.reason = Some("no ownership marker; prune never adopts".to_string());
            return (entry, flags);
        }
        Ok(Some(marker)) => {
            if let Err(error) = marker::validate_marker(&marker, &identity.common_dir, candidate.pr)
            {
                flags.unsafe_skipped = true;
                entry.reason = Some(error);
                return (entry, flags);
            }
            marker
        }
    };
    if let Err(error) = validate_delete_marker_if_present(&identity) {
        flags.unsafe_skipped = true;
        entry.reason = Some(error);
        return (entry, flags);
    }

    let grace = options.older_than.max(MIN_PRUNE_GRACE);
    let Some(newest) = newest_activity(&identity, candidate.record.as_ref()) else {
        entry.reason = Some("no usable timestamp".to_string());
        return (entry, flags);
    };
    let age = SystemTime::now()
        .duration_since(newest)
        .unwrap_or(Duration::ZERO);
    if age < grace {
        entry.reason = Some(format!(
            "younger than the {} grace period",
            duration_label(grace)
        ));
        return (entry, flags);
    }

    let _lease = match PrLease::try_acquire(&identity.common_dir, candidate.pr) {
        Ok(lease) => lease,
        Err(LeaseError::Busy) => {
            flags.busy = true;
            entry.reason = Some("in use by another process".to_string());
            return (entry, flags);
        }
        Err(error) => {
            flags.unsafe_skipped = true;
            entry.reason = Some(format!("could not acquire the PR lease: {error}"));
            return (entry, flags);
        }
    };
    // Ownership and age may have changed while another worker held the lease.
    let marker = match marker::read_marker(&identity.parent) {
        Ok(Some(marker)) => {
            match marker::validate_marker(&marker, &identity.common_dir, candidate.pr) {
                Ok(()) => marker,
                Err(error) => {
                    flags.unsafe_skipped = true;
                    entry.reason = Some(error);
                    return (entry, flags);
                }
            }
        }
        Ok(None) => {
            entry.reason = Some("ownership marker disappeared".to_string());
            return (entry, flags);
        }
        Err(error) => {
            flags.unsafe_skipped = true;
            entry.reason = Some(error);
            return (entry, flags);
        }
    };
    let Some(newest) = newest_activity(&identity, candidate.record.as_ref()) else {
        entry.reason = Some("no usable timestamp".to_string());
        return (entry, flags);
    };
    if SystemTime::now()
        .duration_since(newest)
        .unwrap_or(Duration::ZERO)
        < grace
    {
        entry.reason = Some(format!(
            "younger than the {} grace period",
            duration_label(grace)
        ));
        return (entry, flags);
    }

    let repo_root = ownership::repo_for_common(&identity.common_dir);
    let content = ownership::assess_content(
        &identity.common_dir,
        candidate.pr,
        &identity.parent,
        &repo_root,
    );
    if !content.safe() {
        flags.unsafe_skipped = true;
        entry.reason = content.reason();
        return (entry, flags);
    }
    if options.dry_run {
        entry.action = "would-remove".to_string();
        entry.reason =
            Some("managed PR store older than the grace period; would remove".to_string());
        return (entry, flags);
    }
    let project_root = PathBuf::from(&marker.project_root);
    let project_root = if project_root.is_dir() {
        project_root
    } else {
        repo_root
    };
    let (action, reason) = delete_managed(&identity, &project_root, inventory, diagnostics);
    entry.action = action;
    entry.reason = reason;
    (entry, flags)
}

fn validate_delete_marker_if_present(identity: &StoreIdentity) -> Result<(), String> {
    if let Some(delete) = marker::read_delete_marker(&identity.parent)? {
        marker::validate_delete_marker(&delete, &identity.common_dir, identity.pr)?;
    }
    Ok(())
}

/// Destructive sequence, run only while the caller owns the PR lease and both
/// ownership and content have been validated.
fn delete_managed(
    identity: &StoreIdentity,
    project_root: &Path,
    inventory: Option<&Inventory>,
    diagnostics: &mut Vec<String>,
) -> (String, Option<String>) {
    if let Err(error) =
        ownership::validate_store_marker(&identity.common_dir, identity.pr, &identity.parent)
    {
        return ("partial".to_string(), Some(error));
    }
    let delete_marker: PrStoreDeleteMarker = match marker::read_delete_marker(&identity.parent) {
        Ok(Some(delete)) => {
            match marker::validate_delete_marker(&delete, &identity.common_dir, identity.pr) {
                Ok(()) => delete,
                Err(error) => return ("partial".to_string(), Some(error)),
            }
        }
        Ok(None) => match marker::write_delete_marker(
            &identity.parent,
            &identity.common_dir,
            project_root,
            identity.pr,
        ) {
            Ok(delete) => delete,
            Err(error) => {
                return (
                    "partial".to_string(),
                    Some(format!("could not write the deletion marker: {error}")),
                );
            }
        },
        Err(error) => return ("partial".to_string(), Some(error)),
    };
    let _ = delete_marker;

    if let Err(error) = remove_known_file(&identity.parent.join("commits")) {
        return (
            "partial".to_string(),
            Some(format!("could not invalidate commits: {error}")),
        );
    }

    let repo_root = ownership::repo_for_common(&identity.common_dir);
    let _repo_lock = match RepositoryLock::acquire(&identity.common_dir) {
        Ok(lock) => lock,
        Err(error) => {
            return (
                "partial".to_string(),
                Some(format!(
                    "could not acquire the repository worktree lock: {error}"
                )),
            );
        }
    };

    let content = ownership::assess_content(
        &identity.common_dir,
        identity.pr,
        &identity.parent,
        &repo_root,
    );
    if !content.safe() {
        return (
            "partial".to_string(),
            Some(format!(
                "content changed before removal: {}",
                content.reason().unwrap_or_else(|| "unsafe".to_string())
            )),
        );
    }

    let failures = remove_sides(identity, &repo_root);
    if !failures.is_empty() {
        return ("partial".to_string(), Some(failures.join("; ")));
    }

    let mut unknown = Vec::new();
    match std::fs::read_dir(&identity.parent) {
        Err(error) => {
            return (
                "partial".to_string(),
                Some(format!("reading {}: {error}", identity.parent.display())),
            );
        }
        Ok(entries) => {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if matches!(
                    name.to_str(),
                    Some(marker::MARKER_FILE | marker::DELETE_MARKER_FILE)
                ) {
                    continue;
                }
                unknown.push(name.to_string_lossy().into_owned());
            }
        }
    }
    if !unknown.is_empty() {
        unknown.sort();
        return (
            "partial".to_string(),
            Some(format!("left unknown entries: {}", unknown.join(", "))),
        );
    }
    if let Err(error) = remove_known_file(&marker::delete_marker_path(&identity.parent)) {
        return (
            "partial".to_string(),
            Some(format!("could not remove the deletion marker: {error}")),
        );
    }
    if let Err(error) = remove_known_file(&marker::marker_path(&identity.parent)) {
        return (
            "partial".to_string(),
            Some(format!("could not remove the ownership marker: {error}")),
        );
    }
    if let Err(error) = std::fs::remove_dir(&identity.parent) {
        return (
            "partial".to_string(),
            Some(format!(
                "could not remove {}: {error}",
                identity.parent.display()
            )),
        );
    }
    remove_inventory(inventory, identity, diagnostics);
    (
        "removed".to_string(),
        Some("managed PR store removed".to_string()),
    )
}

fn remove_sides(identity: &StoreIdentity, repo_root: &Path) -> Vec<String> {
    let mut failures = Vec::new();
    for (name, side) in identity.sides() {
        let exists = match std::fs::symlink_metadata(&side) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                failures.push(format!("{name} is a symlink"));
                false
            }
            Ok(metadata) if metadata.is_dir() => true,
            Ok(_) => {
                failures.push(format!("{name} is not a directory"));
                false
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                failures.push(format!("reading {name}: {error}"));
                false
            }
        };
        let registrations = match git::worktree_registrations(repo_root) {
            Ok(registrations) => registrations,
            Err(error) => {
                failures.push(format!("could not verify worktree registrations: {error}"));
                continue;
            }
        };
        let registration = ownership::registration_for(&registrations, &side);
        if !exists {
            match registration {
                Some(_) => {
                    if let Err(error) =
                        ownership::remove_stale_registration(&identity.common_dir, &side)
                    {
                        failures.push(format!("{name}: {error}"));
                    }
                }
                None => match git::registration_admin_dir(&identity.common_dir, &side) {
                    Ok(Some(_)) => {
                        if let Err(error) =
                            ownership::remove_stale_registration(&identity.common_dir, &side)
                        {
                            failures.push(format!("{name}: {error}"));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => failures.push(format!("{name}: {error}")),
                },
            }
            continue;
        }
        let Some(registration) = registration else {
            failures.push(format!("{name} exists but is not a registered worktree"));
            continue;
        };
        if registration.locked {
            failures.push(format!("{name} is a locked worktree"));
            continue;
        }
        if registration.branch.is_some() || !registration.detached {
            failures.push(format!("{name} is attached to a branch"));
            continue;
        }
        match git::registration_admin_dir(&identity.common_dir, &side) {
            Ok(Some(admin)) if git::admin_gitdir_matches(&admin, &side) => {}
            Ok(_) => {
                failures.push(format!("{name} has no registration that names it exactly"));
                continue;
            }
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        }
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["worktree", "remove", "--force", &side.display().to_string()])
            .output();
        match output {
            Ok(output) if output.status.success() => {}
            Ok(output) => failures.push(format!(
                "{name}: git worktree remove --force: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(error) => failures.push(format!("{name}: running git worktree remove: {error}")),
        }
    }
    failures
}

fn remove_known_file(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("reading {}: {error}", path.display())),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(format!("{} is not a regular file", path.display()))
        }
        Ok(_) => std::fs::remove_file(path)
            .map_err(|error| format!("removing {}: {error}", path.display())),
    }
}

fn remove_inventory(
    inventory: Option<&Inventory>,
    identity: &StoreIdentity,
    diagnostics: &mut Vec<String>,
) {
    let Some(inventory) = inventory else {
        return;
    };
    if let Err(error) = inventory.remove(
        CacheKind::Pr,
        Some(&canonical_string(&identity.common_dir)),
        Some(identity.pr),
        &canonical_string(&identity.parent),
    ) {
        diagnostics.push(format!(
            "removed PR {} store but could not remove its inventory record: {error}",
            identity.pr
        ));
    }
}

fn entry_for(
    candidate: &Candidate,
    action: &str,
    reason: Option<String>,
    estimated_bytes: Option<u64>,
) -> MaintenanceEntry {
    MaintenanceEntry {
        kind: "pr".to_string(),
        path: canonical_string(&candidate.path),
        pr: Some(candidate.pr),
        action: action.to_string(),
        reason,
        estimated_bytes,
    }
}

fn newest_activity(identity: &StoreIdentity, record: Option<&CacheRecord>) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    let metadata_paths = [
        marker::marker_path(&identity.parent),
        identity.parent.join("commits"),
        marker::delete_marker_path(&identity.parent),
        identity.base.clone(),
        identity.head.clone(),
    ];
    for path in metadata_paths {
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            if let Ok(modified) = metadata.modified() {
                newest = Some(newest.map_or(modified, |current| current.max(modified)));
            }
        }
    }
    for side in [&identity.base, &identity.head] {
        if let Some(modified) = newest_tree_mtime(&side.join(".gitnexus")) {
            newest = Some(newest.map_or(modified, |current| current.max(modified)));
        }
    }
    if let Some(record) = record {
        for timestamp in [record.last_built, record.last_used] {
            if let Some(time) = unix_time(timestamp) {
                newest = Some(newest.map_or(time, |current| current.max(time)));
            }
        }
    }
    newest
}

fn newest_tree_mtime(root: &Path) -> Option<SystemTime> {
    let metadata = std::fs::symlink_metadata(root).ok()?;
    if metadata.file_type().is_symlink() {
        return metadata.modified().ok();
    }
    let mut newest = metadata.modified().ok();
    if !metadata.is_dir() {
        return newest;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                    newest = Some(newest.map_or(modified, |current| current.max(modified)));
                }
                continue;
            }
            if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                newest = Some(newest.map_or(modified, |current| current.max(modified)));
            }
            if kind.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    newest
}

fn unix_time(value: Option<f64>) -> Option<SystemTime> {
    let seconds = value?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    UNIX_EPOCH.checked_add(Duration::from_secs_f64(seconds))
}

fn duration_label(duration: Duration) -> String {
    let seconds = duration.as_secs();
    for (unit, size) in [
        ("w", 7 * 24 * 60 * 60),
        ("d", 24 * 60 * 60),
        ("h", 60 * 60),
        ("m", 60),
    ] {
        if seconds >= size && seconds.is_multiple_of(size) {
            return format!("{}{unit}", seconds / size);
        }
    }
    format!("{seconds}s")
}

/// Recursive estimate that never follows symlinks. Reporting only.
fn estimate_bytes(path: &Path) -> u64 {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return 0,
    };
    if metadata.file_type().is_symlink() {
        return 0;
    }
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(entry.path());
            } else if let Ok(metadata) = entry.metadata() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::now;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn repo() -> (tempfile::TempDir, PathBuf, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.email", "clean@example.com"]);
        git(&repo, &["config", "user.name", "Clean"]);
        let commit = |body: &str| {
            std::fs::write(repo.join("a.txt"), body).unwrap();
            git(&repo, &["add", "."]);
            git(&repo, &["commit", "-qm", body]);
            git(&repo, &["rev-parse", "HEAD"])
        };
        let base = commit("base");
        let head = commit("head");
        (dir, repo, base, head)
    }

    fn common(repo: &Path) -> PathBuf {
        git::git_common_dir(repo).unwrap()
    }

    fn store(repo: &Path, pr: u64, base: &str, head: &str) -> StoreIdentity {
        let identity = StoreIdentity::expected(&common(repo), pr);
        std::fs::create_dir_all(&identity.parent).unwrap();
        marker::write_marker(
            &identity.parent,
            &identity.common_dir,
            repo,
            pr,
            Some("test".to_string()),
        )
        .unwrap();
        for (side, sha) in [(&identity.base, base), (&identity.head, head)] {
            git(
                repo,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "--detach",
                    &side.display().to_string(),
                    sha,
                ],
            );
            std::fs::create_dir_all(side.join(".gitnexus")).unwrap();
        }
        std::fs::write(identity.parent.join("commits"), format!("{base}\n{head}\n")).unwrap();
        identity
    }

    fn inventory(dir: &Path) -> Inventory {
        Inventory::at(dir.join("index"))
    }

    fn clean_at(
        repo: &Path,
        inventory: &Inventory,
        options: CleanOptions,
    ) -> Result<MaintenanceReport, String> {
        clean_with(options, repo, Some(inventory))
    }

    fn prune_at(
        repo: &Path,
        inventory: &Inventory,
        options: PruneOptions,
    ) -> Result<MaintenanceReport, String> {
        prune_with(options, repo, Some(inventory))
    }

    fn record(inventory: &Inventory, repo: &Path, identity: &StoreIdentity) {
        inventory
            .upsert(super::super::inventory::RecordUpdate {
                kind: CacheKind::Pr,
                managed: true,
                project_root: canonical_string(repo),
                common_git_dir: Some(canonical_string(&identity.common_dir)),
                path: canonical_string(&identity.parent),
                pr: Some(identity.pr),
                last_built: Some(now() - 10_000.0),
                last_used: Some(now() - 10_000.0),
                last_seen: Some(now() - 10_000.0),
                compatibility: None,
            })
            .unwrap();
    }

    fn set_old(path: &Path, age: Duration) {
        let when = SystemTime::now() - age;
        std::fs::File::open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    fn age_store(identity: &StoreIdentity, age: Duration) {
        let when = SystemTime::now() - age;
        for path in [
            marker::marker_path(&identity.parent),
            identity.parent.join("commits"),
            marker::delete_marker_path(&identity.parent),
            identity.base.clone(),
            identity.head.clone(),
        ] {
            if let Ok(file) = std::fs::File::open(&path) {
                file.set_modified(when).unwrap();
            }
        }
        for side in [&identity.base, &identity.head] {
            age_tree(&side.join(".gitnexus"), when);
        }
    }

    fn age_tree(root: &Path, when: SystemTime) {
        let Ok(metadata) = std::fs::symlink_metadata(root) else {
            return;
        };
        let _ = std::fs::File::open(root).and_then(|file| file.set_modified(when));
        if !metadata.is_dir() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            age_tree(&entry.path(), when);
        }
    }

    #[test]
    fn clean_removes_a_managed_store_but_keeps_locks_and_project_graphs() {
        let (_keep, repo, base, head) = repo();
        let identity = store(&repo, 1, &base, &head);
        let project_graph = repo.join(".gitnexus");
        std::fs::create_dir_all(&project_graph).unwrap();
        std::fs::write(project_graph.join("lbug"), b"project").unwrap();
        let inventory = inventory(&repo);
        record(&inventory, &repo, &identity);

        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(1),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();

        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].action, "removed", "{:?}", report.entries);
        assert_eq!(report.totals.removed, 1);
        assert!(!identity.parent.exists());
        assert!(!identity.base.exists());
        assert!(!identity.head.exists());
        assert!(identity.common_dir.join("topos-pr-1.lease").exists());
        assert!(identity.common_dir.join("topos-worktrees.lock").exists());
        assert!(project_graph.join("lbug").exists());
        let (records, _) = inventory.records();
        assert!(!records.iter().any(|record| record.pr == Some(1)));
        let registrations = git::worktree_registrations(&repo).unwrap();
        assert!(!registrations.iter().any(|registration| {
            registration.path == identity.base || registration.path == identity.head
        }));
    }

    #[test]
    fn a_missing_target_is_a_successful_none_outcome() {
        let (_keep, repo, _, _) = repo();
        let inventory = inventory(&repo);
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(77),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "none");
        assert!(report.entries[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("no such"));
    }

    #[test]
    fn a_busy_store_is_reported_and_preserved() {
        let (_keep, repo, base, head) = repo();
        let identity = store(&repo, 2, &base, &head);
        let inventory = inventory(&repo);
        let held = PrLease::try_acquire(&identity.common_dir, 2).unwrap();
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(2),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "skipped");
        assert_eq!(report.totals.busy, 1);
        assert!(identity.parent.exists());
        drop(held);
    }

    #[test]
    fn user_content_locked_worktrees_and_unexpected_entries_are_preserved() {
        let (_keep, repo, base, head) = repo();
        let identity = store(&repo, 3, &base, &head);
        let inventory = inventory(&repo);
        std::fs::write(identity.base.join("notes.txt"), "user").unwrap();
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(3),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "skipped");
        assert!(report.entries[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("notes.txt"));
        assert!(identity.base.join("notes.txt").exists());
        std::fs::remove_file(identity.base.join("notes.txt")).unwrap();

        git(
            &repo,
            &["worktree", "lock", &identity.head.display().to_string()],
        );
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(3),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "skipped");
        assert!(report.entries[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("locked"));
        assert!(identity.parent.exists());
        git(
            &repo,
            &["worktree", "unlock", &identity.head.display().to_string()],
        );
    }

    #[test]
    fn a_foreign_or_redirected_store_is_never_removed() {
        let (_keep, repo, base, head) = repo();
        let identity = store(&repo, 4, &base, &head);
        let inventory = inventory(&repo);
        std::fs::write(identity.parent.join("user.txt"), "keep").unwrap();
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(4),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "partial");
        assert!(identity.parent.join("user.txt").exists());
        assert!(!identity.parent.join("commits").exists());
        assert!(identity.parent.join("topos-delete.json").exists());

        // A retry reclaims the managed sides and leaves only the unknown file.
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(4),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "partial");
        assert!(identity.parent.join("user.txt").exists());
        assert!(!identity.base.exists());
        assert!(!identity.head.exists());
    }

    #[test]
    fn cleanup_adopts_a_valid_legacy_store_and_refuses_a_malformed_one() {
        let (_keep, repo, base, head) = repo();
        let identity = StoreIdentity::expected(&common(&repo), 5);
        std::fs::create_dir_all(&identity.parent).unwrap();
        for (side, sha) in [(&identity.base, &base), (&identity.head, &head)] {
            git(
                &repo,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "--detach",
                    &side.display().to_string(),
                    sha,
                ],
            );
        }
        std::fs::write(identity.parent.join("commits"), format!("{base}\n{head}\n")).unwrap();
        let inventory = inventory(&repo);
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(5),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "removed", "{:?}", report.entries);

        let bad = StoreIdentity::expected(&common(&repo), 6);
        std::fs::create_dir_all(&bad.parent).unwrap();
        std::fs::write(bad.parent.join("commits"), "not a commit\n").unwrap();
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(6),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "skipped");
        assert!(bad.parent.exists());
        assert!(!bad.parent.join("topos-cache.json").exists());

        // A structurally valid legacy slot with user content is never marked:
        // content is validated before adoption, not after.
        let guarded = StoreIdentity::expected(&common(&repo), 13);
        std::fs::create_dir_all(&guarded.parent).unwrap();
        for (side, sha) in [(&guarded.base, &base), (&guarded.head, &head)] {
            git(
                &repo,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "--detach",
                    &side.display().to_string(),
                    sha,
                ],
            );
        }
        std::fs::write(guarded.parent.join("commits"), format!("{base}\n{head}\n")).unwrap();
        std::fs::write(guarded.base.join("notes.txt"), "user").unwrap();
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(13),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "skipped", "{:?}", report.entries);
        assert!(report.entries[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("notes.txt"));
        assert!(!guarded.parent.join("topos-cache.json").exists());
        assert!(guarded.base.join("notes.txt").exists());
    }

    #[test]
    fn dry_run_reports_without_writing_or_deleting() {
        let (_keep, repo, base, head) = repo();
        let identity = store(&repo, 7, &base, &head);
        let inventory = inventory(&repo);
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(7),
                all_prs: false,
                global: false,
                dry_run: true,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "would-remove");
        assert!(!identity.parent.join("topos-delete.json").exists());
        assert!(identity.parent.exists());
        assert!(identity.base.exists());
    }

    #[test]
    fn all_prs_keeps_going_after_one_busy_store() {
        let (_keep, repo, base, head) = repo();
        let busy = store(&repo, 8, &base, &head);
        let free = store(&repo, 9, &base, &head);
        let inventory = inventory(&repo);
        let _held = PrLease::try_acquire(&busy.common_dir, 8).unwrap();
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: None,
                all_prs: true,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries.len(), 2);
        assert_eq!(report.totals.busy, 1);
        assert_eq!(report.totals.removed, 1);
        assert!(busy.parent.exists());
        assert!(!free.parent.exists());
    }

    #[test]
    fn prune_requires_the_grace_period_and_a_marker() {
        let (_keep, repo, base, head) = repo();
        let old = store(&repo, 10, &base, &head);
        age_store(&old, Duration::from_secs(3 * 24 * 60 * 60));
        let unmarked = StoreIdentity::expected(&common(&repo), 11);
        std::fs::create_dir_all(&unmarked.parent).unwrap();
        let inventory = inventory(&repo);
        let report = prune_at(
            &repo,
            &inventory,
            PruneOptions {
                older_than: Duration::from_secs(60),
                global: false,
                dry_run: true,
            },
        )
        .unwrap();
        let old_entry = report
            .entries
            .iter()
            .find(|entry| entry.pr == Some(10))
            .unwrap();
        assert_eq!(old_entry.action, "would-remove", "{:?}", report.entries);
        let unmarked_entry = report
            .entries
            .iter()
            .find(|entry| entry.pr == Some(11))
            .unwrap();
        assert_eq!(unmarked_entry.action, "skipped");
        assert!(unmarked.parent.exists());

        let fresh = store(&repo, 12, &base, &head);
        let report = prune_at(
            &repo,
            &inventory,
            PruneOptions {
                older_than: Duration::from_secs(24 * 60 * 60),
                global: false,
                dry_run: true,
            },
        )
        .unwrap();
        let fresh_entry = report
            .entries
            .iter()
            .find(|entry| entry.pr == Some(12))
            .unwrap();
        assert_eq!(fresh_entry.action, "skipped");
        assert!(fresh_entry.reason.as_deref().unwrap().contains("younger"));
        assert!(fresh.parent.exists());
    }

    #[test]
    fn prune_age_uses_newer_nested_graph_data_over_an_old_marker() {
        let (_keep, repo, base, head) = repo();
        let identity = store(&repo, 13, &base, &head);
        set_old(
            &identity.parent.join("topos-cache.json"),
            Duration::from_secs(10 * 24 * 60 * 60),
        );
        let nested = identity.base.join(".gitnexus/deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("lbug"), b"fresh").unwrap();
        let inventory = inventory(&repo);
        let report = prune_at(
            &repo,
            &inventory,
            PruneOptions {
                older_than: Duration::from_secs(24 * 60 * 60),
                global: false,
                dry_run: true,
            },
        )
        .unwrap();
        let entry = report
            .entries
            .iter()
            .find(|entry| entry.pr == Some(13))
            .unwrap();
        assert_eq!(entry.action, "skipped", "{:?}", report.entries);
        assert!(entry.reason.as_deref().unwrap().contains("younger"));
    }

    #[test]
    fn prune_actually_removes_an_old_managed_store() {
        let (_keep, repo, base, head) = repo();
        let identity = store(&repo, 14, &base, &head);
        age_store(&identity, Duration::from_secs(5 * 24 * 60 * 60));
        let inventory = inventory(&repo);
        let report = prune_at(
            &repo,
            &inventory,
            PruneOptions {
                older_than: Duration::from_secs(24 * 60 * 60),
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        let entry = report
            .entries
            .iter()
            .find(|entry| entry.pr == Some(14))
            .unwrap();
        assert_eq!(entry.action, "removed", "{:?}", report.entries);
        assert!(!identity.parent.exists());
    }

    #[test]
    fn parse_duration_understands_seconds_through_weeks() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("3h").unwrap(), Duration::from_secs(10_800));
        assert_eq!(parse_duration("4d").unwrap(), Duration::from_secs(345_600));
        assert_eq!(parse_duration("1w").unwrap(), Duration::from_secs(604_800));
        assert!(parse_duration("").is_err());
        assert!(parse_duration("7").is_err());
        assert!(parse_duration("7y").is_err());
        assert!(parse_duration("x7d").is_err());
    }

    #[test]
    fn stale_registration_is_removed_only_when_the_admin_record_names_the_side() {
        let (_keep, repo, base, head) = repo();
        let identity = store(&repo, 15, &base, &head);
        let inventory = inventory(&repo);
        std::fs::remove_dir_all(&identity.base).unwrap();
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(15),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(report.entries[0].action, "removed", "{:?}", report.entries);
        assert!(!identity.parent.exists());

        // A registration redirected at another path is not pruned.
        let other = StoreIdentity::expected(&common(&repo), 16);
        std::fs::create_dir_all(&other.parent).unwrap();
        marker::write_marker(&other.parent, &other.common_dir, &repo, 16, None).unwrap();
        std::fs::create_dir_all(&other.base).unwrap();
        std::fs::create_dir_all(&other.head).unwrap();
        let redirected = git::worktree_admin_dirs(&other.common_dir)
            .unwrap()
            .into_iter()
            .find(|admin| {
                git::admin_gitdir_path(admin)
                    .ok()
                    .is_some_and(|path| path.starts_with(&other.parent))
            });
        if let Some(admin) = redirected {
            std::fs::write(
                admin.join("gitdir"),
                other
                    .parent
                    .join("elsewhere/.git")
                    .to_string_lossy()
                    .as_bytes(),
            )
            .unwrap();
        }
        let report = clean_at(
            &repo,
            &inventory,
            CleanOptions {
                pr: Some(16),
                all_prs: false,
                global: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert!(matches!(
            report.entries[0].action.as_str(),
            "skipped" | "partial"
        ));
        assert!(other.parent.exists());
    }
}
