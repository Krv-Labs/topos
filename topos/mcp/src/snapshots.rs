//! Content-addressed snapshot store for the Topos MCP refactor loop.
//!
//! Lets an agent capture a file's baseline source *before* an in-place
//! edit, then assess the edited file against that baseline without
//! re-sending the source (`topos_begin_refactor` → edit →
//! `topos_assess_snapshot`).
//!
//! Two deliberate design choices, carried over from the Python original:
//!
//! - **Outside the working tree.** A store under the repo would pollute
//!   `git status`, risk accidental commits, and fight
//!   `topos_assess_worktree_change`'s own `git show HEAD` baseline. So the
//!   store lives under the system temp dir, namespaced per project root.
//!   Override with `TOPOS_SNAPSHOT_DIR`.
//! - **On disk, content-addressed — not an in-process map.** Snapshots
//!   survive a stdio-server restart, and the `snapshot_id` is a
//!   self-contained content hash rather than an opaque server handle.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Reject snapshots past this age on read, and sweep them on write.
const TTL_SECONDS: f64 = 24.0 * 60.0 * 60.0;

/// Outcome of [`read_snapshot`]. `blocked_by` is `None` on success and
/// otherwise `snapshot_not_found` / `snapshot_stale` so the tool layer can
/// surface it directly in the agent contract.
#[derive(Debug, Default)]
pub struct SnapshotLoad {
    pub baseline_src: Option<String>,
    pub meta: Option<HashMap<String, Value>>,
    pub blocked_by: Option<&'static str>,
}

/// Current wall-clock time as a float of Unix seconds.
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn sha256_hex(data: &str) -> String {
    hex::encode(Sha256::digest(data.as_bytes()))
}

fn is_snapshot_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Per-project snapshot directory, outside the working tree.
fn store_root(project_root: &Path) -> PathBuf {
    let base = match std::env::var("TOPOS_SNAPSHOT_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => std::env::temp_dir().join("topos-snapshots"),
    };
    let key = sha256_hex(&project_root.to_string_lossy());
    base.join(&key[..16])
}

/// Best-effort eviction of blobs/sidecars past the TTL.
fn sweep_expired(root: &Path, at: f64) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let mtime = modified
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        if at - mtime > TTL_SECONDS {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(unix)]
fn harden(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn harden(_path: &Path, _mode: u32) {}

/// Persist a baseline and return its `snapshot_id`.
///
/// The id is keyed by `(filepath, content)` — NOT content alone — so two
/// files with identical baseline source get distinct handles. The pure
/// content hash lives in the sidecar as `baseline_hash`, alongside
/// priority/preferences/`gitnexus_dir` so `topos_assess_snapshot` stays a
/// clean 2-arg call.
pub fn write_snapshot(
    project_root: &Path,
    baseline_src: &str,
    mut meta: HashMap<String, Value>,
    created_at: f64,
) -> std::io::Result<String> {
    let baseline_hash = sha256_hex(baseline_src);
    let filepath = meta
        .get("filepath")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let snapshot_id = sha256_hex(&format!("{filepath}\0{baseline_hash}"));
    let root = store_root(project_root);
    std::fs::create_dir_all(&root)?;
    harden(&root, 0o700);
    sweep_expired(&root, created_at);
    let blob = root.join(format!("{snapshot_id}.blob"));
    if !blob.exists() {
        std::fs::write(&blob, baseline_src.as_bytes())?;
        harden(&blob, 0o600);
    }
    meta.insert("baseline_hash".to_string(), Value::from(baseline_hash));
    meta.insert("created_at".to_string(), Value::from(created_at));
    let sidecar = root.join(format!("{snapshot_id}.json"));
    std::fs::write(&sidecar, serde_json::to_vec(&meta)?)?;
    harden(&sidecar, 0o600);
    Ok(snapshot_id)
}

/// Load a baseline by `snapshot_id`.
///
/// Returns `snapshot_not_found` when the id is malformed or the
/// blob/sidecar is missing, and `snapshot_stale` when the snapshot is past
/// its TTL. The tool layer additionally treats a filepath mismatch as
/// `snapshot_stale`.
pub fn read_snapshot(project_root: &Path, snapshot_id: &str, at: f64) -> SnapshotLoad {
    if !is_snapshot_id(snapshot_id) {
        return SnapshotLoad {
            blocked_by: Some("snapshot_not_found"),
            ..Default::default()
        };
    }
    let root = store_root(project_root);
    let baseline_src = std::fs::read_to_string(root.join(format!("{snapshot_id}.blob")));
    let sidecar = std::fs::read_to_string(root.join(format!("{snapshot_id}.json")));
    let (Ok(baseline_src), Ok(sidecar)) = (baseline_src, sidecar) else {
        return SnapshotLoad {
            blocked_by: Some("snapshot_not_found"),
            ..Default::default()
        };
    };
    let Ok(meta): Result<HashMap<String, Value>, _> = serde_json::from_str(&sidecar) else {
        return SnapshotLoad {
            blocked_by: Some("snapshot_not_found"),
            ..Default::default()
        };
    };
    let created_at = meta
        .get("created_at")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if at - created_at > TTL_SECONDS {
        return SnapshotLoad {
            blocked_by: Some("snapshot_stale"),
            ..Default::default()
        };
    }
    SnapshotLoad {
        baseline_src: Some(baseline_src),
        meta: Some(meta),
        blocked_by: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // `TOPOS_SNAPSHOT_DIR` is process-global; serialize the tests that set
    // it so parallel runs never observe each other's value.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn meta_for(filepath: &str) -> HashMap<String, Value> {
        HashMap::from([("filepath".to_string(), Value::from(filepath))])
    }

    #[test]
    fn round_trip_and_ttl() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("TOPOS_SNAPSHOT_DIR", dir.path());
        let project_root = dir.path().join("proj");
        let t0 = now();
        let id = write_snapshot(&project_root, "x = 1\n", meta_for("/proj/a.py"), t0).unwrap();
        assert!(is_snapshot_id(&id));

        let load = read_snapshot(&project_root, &id, t0 + 1.0);
        assert_eq!(load.baseline_src.as_deref(), Some("x = 1\n"));
        assert_eq!(
            load.meta.unwrap().get("filepath").and_then(Value::as_str),
            Some("/proj/a.py")
        );

        let stale = read_snapshot(&project_root, &id, t0 + TTL_SECONDS + 10.0);
        assert_eq!(stale.blocked_by, Some("snapshot_stale"));

        let missing = read_snapshot(&project_root, &"0".repeat(64), t0);
        assert_eq!(missing.blocked_by, Some("snapshot_not_found"));

        let malformed = read_snapshot(&project_root, "../../etc/passwd", t0);
        assert_eq!(malformed.blocked_by, Some("snapshot_not_found"));
        std::env::remove_var("TOPOS_SNAPSHOT_DIR");
    }

    #[test]
    fn same_content_different_files_get_distinct_ids() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("TOPOS_SNAPSHOT_DIR", dir.path());
        let project_root = dir.path().join("proj");
        let t0 = now();
        let a = write_snapshot(&project_root, "", meta_for("/proj/a/__init__.py"), t0).unwrap();
        let b = write_snapshot(&project_root, "", meta_for("/proj/b/__init__.py"), t0).unwrap();
        assert_ne!(a, b);
        std::env::remove_var("TOPOS_SNAPSHOT_DIR");
    }
}
