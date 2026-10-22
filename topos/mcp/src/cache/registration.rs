//! Best-effort lazy registration of data Topos actually used.
//!
//! Every function here is advisory: a failed inventory write must never fail
//! an otherwise valid analysis. Access writes are throttled per process so a
//! freshness probe does not touch the user's home directory repeatedly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use topos_engine::adapters::gitnexus::{gitnexus_version, GITNEXUS_FINGERPRINT_FILE};

use super::git;
use super::inventory::{
    canonical_string, now, CacheCompatibility, CacheKind, Inventory, RecordUpdate,
};

const TOUCH_INTERVAL: Duration = Duration::from_secs(60);

static LAST_TOUCH: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Unit tests must not write into the developer's real state directory.
/// Integration tests (compiled without `cfg(test)`) and production do.
fn auto_registration_enabled() -> bool {
    #[cfg(test)]
    {
        super::inventory::test_index_active()
    }
    #[cfg(not(test))]
    {
        true
    }
}

fn throttled(key: &str) -> bool {
    let Ok(mut seen) = LAST_TOUCH.lock() else {
        return true;
    };
    let now = Instant::now();
    match seen.get(key) {
        Some(last) if now.duration_since(*last) < TOUCH_INTERVAL => true,
        _ => {
            seen.insert(key.to_string(), now);
            false
        }
    }
}

struct PrUpdateTimes {
    managed: bool,
    built: Option<f64>,
    used: Option<f64>,
    compatibility: Option<CacheCompatibility>,
}

fn pr_update(
    repo_root: &Path,
    parent: &Path,
    common: &Path,
    pr: u64,
    times: PrUpdateTimes,
) -> RecordUpdate {
    RecordUpdate {
        kind: CacheKind::Pr,
        managed: times.managed,
        project_root: canonical_string(repo_root),
        common_git_dir: Some(canonical_string(common)),
        path: canonical_string(parent),
        pr: Some(pr),
        last_built: times.built,
        last_used: times.used,
        last_seen: Some(now()),
        compatibility: times.compatibility,
    }
}

/// Record a successful PR build. Never fails the build.
pub fn pr_built(repo_root: &Path, pr: u64, parent: &Path) {
    if !auto_registration_enabled() {
        return;
    }
    let Ok(common) = git::git_common_dir(repo_root) else {
        return;
    };
    let Ok(inventory) = Inventory::open() else {
        return;
    };
    let at = now();
    let compatibility = CacheCompatibility {
        layout: super::marker::MARKER_LAYOUT,
        generator: gitnexus_version().map(|version| format!("gitnexus@{version}")),
    };
    let update = pr_update(
        repo_root,
        parent,
        &common,
        pr,
        PrUpdateTimes {
            managed: true,
            built: Some(at),
            used: Some(at),
            compatibility: Some(compatibility),
        },
    );
    let _ = inventory.upsert(update);
}

/// Record that an existing ready store was actually consumed, and backfill
/// legacy stores that are structurally usable.
pub fn pr_used(repo_root: &Path, pr: u64, parent: &Path) {
    if !auto_registration_enabled() {
        return;
    }
    let Ok(common) = git::git_common_dir(repo_root) else {
        return;
    };
    let key = format!("pr\0{}\0{pr}", canonical_string(&common));
    if throttled(&key) {
        return;
    }
    let managed = marker_or_legacy_valid(&common, pr, parent);
    let Ok(inventory) = Inventory::open() else {
        return;
    };
    let update = pr_update(
        repo_root,
        parent,
        &common,
        pr,
        PrUpdateTimes {
            managed,
            built: None,
            used: Some(now()),
            compatibility: None,
        },
    );
    let _ = inventory.upsert(update);
}

/// A cheap structural check used only to decide the inventory's `managed`
/// flag. Deletion still revalidates everything on disk.
fn marker_or_legacy_valid(common: &Path, pr: u64, parent: &Path) -> bool {
    if let Ok(Some(marker)) = super::marker::read_marker(parent) {
        if super::marker::validate_marker(&marker, common, pr).is_ok() {
            return true;
        }
    }
    let commits = std::fs::read_to_string(parent.join("commits")).ok();
    let Some(commits) = commits else {
        return false;
    };
    let mut lines = commits.lines();
    let (Some(base), Some(head)) = (lines.next(), lines.next()) else {
        return false;
    };
    if base.is_empty() || head.is_empty() {
        return false;
    }
    parent.join("base/.git").exists() && parent.join("head/.git").exists()
}

/// Record that an ordinary project graph was resolved or consumed.
pub fn graph_used(gitnexus_dir: &Path) {
    if !auto_registration_enabled() {
        return;
    }
    let path = git::normalize_path(gitnexus_dir);
    let key = format!("graph\0{}", path.to_string_lossy());
    if throttled(&key) {
        return;
    }
    let Some(project_root) = path.parent() else {
        return;
    };
    let Ok(inventory) = Inventory::open() else {
        return;
    };
    let built = graph_built_at(&path);
    let common = common_dir_if_repository(project_root);
    let update = RecordUpdate {
        kind: CacheKind::Graph,
        managed: false,
        project_root: canonical_string(project_root),
        common_git_dir: common.map(|path| canonical_string(&path)),
        path: canonical_string(&path),
        pr: None,
        last_built: built,
        last_used: Some(now()),
        last_seen: Some(now()),
        compatibility: None,
    };
    let _ = inventory.upsert(update);
}

fn common_dir_if_repository(project_root: &Path) -> Option<PathBuf> {
    if !project_root.join(".git").exists() {
        return None;
    }
    git::git_common_dir(project_root).ok()
}

/// GitNexus writes `finished_at` in the Topos fingerprint. That is the only
/// trustworthy build time available without opening the store.
fn graph_built_at(gitnexus_dir: &Path) -> Option<f64> {
    let raw = std::fs::read_to_string(gitnexus_dir.join(GITNEXUS_FINGERPRINT_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value.get("finished_at").and_then(serde_json::Value::as_f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn fingerprint_time_is_used_without_opening_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join(".gitnexus");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(
            store.join(GITNEXUS_FINGERPRINT_FILE),
            r#"{"generated_at": 10.0, "finished_at": 42.5}"#,
        )
        .unwrap();
        assert_eq!(graph_built_at(&store), Some(42.5));
        assert_eq!(graph_built_at(&dir.path().join("missing")), None);
    }

    #[test]
    fn lazy_registration_writes_only_used_locations() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("index");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join(".gitnexus")).unwrap();
        std::fs::write(repo.join(".gitnexus/lbug"), b"x").unwrap();
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());

        let common = git::git_common_dir(&repo).unwrap();
        let parent = common.join("topos-pr-4");
        std::fs::create_dir_all(parent.join("base/.git")).unwrap();
        std::fs::create_dir_all(parent.join("head/.git")).unwrap();
        std::fs::write(parent.join("commits"), "abcdef1\nabcdef2\n").unwrap();
        let (records, diagnostics) = crate::cache::inventory::with_test_index(&index, || {
            pr_used(&repo, 4, &parent);
            graph_used(&repo.join(".gitnexus"));
            Inventory::open().unwrap().records()
        });
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(records.len(), 2, "{records:?}");
        let pr = records.iter().find(|record| record.kind == "pr").unwrap();
        assert_eq!(pr.pr, Some(4));
        assert!(pr.last_seen.is_some());
        let graph = records
            .iter()
            .find(|record| record.kind == "graph")
            .unwrap();
        assert!(graph.last_used.is_some());
        assert_eq!(graph.compatibility, None);
    }
}
