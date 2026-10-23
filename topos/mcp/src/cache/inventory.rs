//! Versioned, locked inventory of candidate cache locations.
//!
//! One record per identity, one file per record. The inventory is a discovery
//! hint: a corrupt or stale record can hide a location, but it can never make
//! a location deletable. Writers take an exclusive lock and merge timestamps
//! monotonically; readers take a shared lock. Files are replaced through a
//! unique temporary file in the same directory, never written in place.

#[cfg(test)]
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::paths::{home_dir, state_dir};

use super::git;

/// The public JSON schema for status/discover/clean/prune documents.
pub const INDEX_SCHEMA: &str = "topos.cache.v1";
const ENTRY_SCHEMA: &str = "topos.cache_entry.v1";
const ENTRIES_DIR: &str = "entries";
const LOCK_FILE: &str = ".lock";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// What kind of generated data a record points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheKind {
    Pr,
    Graph,
}

impl CacheKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pr => "pr",
            Self::Graph => "graph",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "pr" => Some(Self::Pr),
            "graph" => Some(Self::Graph),
            _ => None,
        }
    }
}

/// Generator/cache-format identity, kept separate from the inventory schema.
///
/// The inventory schema only describes bookkeeping. `layout` describes the
/// managed PR-store layout; `generator` is the graph generator that produced
/// it when known. Ordinary project graphs are GitNexus-owned, so they carry
/// no Topos compatibility claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheCompatibility {
    pub layout: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
}

/// One candidate location. All timestamps are Unix seconds and optional when
/// the historical value is unknown; they are never invented.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheRecord {
    pub schema: String,
    pub kind: String,
    pub managed: bool,
    pub project_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub common_git_dir: Option<String>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_built: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<CacheCompatibility>,
}

impl CacheRecord {
    pub fn kind(&self) -> Option<CacheKind> {
        CacheKind::parse(&self.kind)
    }

    /// Stable identity: canonical common directory + PR slot, or canonical
    /// graph path.
    pub fn key(&self) -> String {
        if self.kind == CacheKind::Pr.as_str() {
            if let (Some(common), Some(pr)) = (&self.common_git_dir, self.pr) {
                return key_for(CacheKind::Pr, Some(common), Some(pr), "");
            }
        }
        key_for(CacheKind::Graph, None, None, &self.path)
    }

    fn merge(mut self, other: &CacheRecord) -> Self {
        self.managed |= other.managed;
        self.last_built = max_time(self.last_built, other.last_built);
        self.last_used = max_time(self.last_used, other.last_used);
        self.last_seen = max_time(self.last_seen, other.last_seen);
        if self.compatibility.is_none() {
            self.compatibility = other.compatibility.clone();
        }
        self
    }
}

/// What a registration wants to record. `None` timestamps mean "leave the
/// existing value alone"; merge still takes the maximum under the lock.
#[derive(Debug, Clone)]
pub struct RecordUpdate {
    pub kind: CacheKind,
    pub managed: bool,
    pub project_root: String,
    pub common_git_dir: Option<String>,
    pub path: String,
    pub pr: Option<u64>,
    pub last_built: Option<f64>,
    pub last_used: Option<f64>,
    pub last_seen: Option<f64>,
    pub compatibility: Option<CacheCompatibility>,
}

impl RecordUpdate {
    fn into_record(self) -> CacheRecord {
        CacheRecord {
            schema: ENTRY_SCHEMA.to_string(),
            kind: self.kind.as_str().to_string(),
            managed: self.managed,
            project_root: self.project_root,
            common_git_dir: self.common_git_dir,
            path: self.path,
            pr: self.pr,
            last_built: self.last_built,
            last_used: self.last_used,
            last_seen: self.last_seen,
            compatibility: self.compatibility,
        }
    }

    pub fn key(&self) -> String {
        key_for(
            self.kind,
            self.common_git_dir.as_deref(),
            self.pr,
            &self.path,
        )
    }
}

pub(super) fn key_for(
    kind: CacheKind,
    common_git_dir: Option<&str>,
    pr: Option<u64>,
    path: &str,
) -> String {
    let material = match kind {
        CacheKind::Pr => format!(
            "pr\0{}\0{}",
            common_git_dir.unwrap_or_default(),
            pr.unwrap_or_default()
        ),
        CacheKind::Graph => format!("graph\0{path}"),
    };
    let digest = Sha256::digest(material.as_bytes());
    format!(
        "{}-{}",
        kind.as_str(),
        digest[..12]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn max_time(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

#[cfg(test)]
thread_local! {
    static TEST_INDEX: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Run a unit test against an isolated inventory without changing process
/// environment variables. The override is thread-local so parallel tests cannot
/// observe or write through another test's index.
#[cfg(test)]
pub(crate) fn with_test_index<R>(dir: &Path, body: impl FnOnce() -> R) -> R {
    let previous = TEST_INDEX.with(|slot| slot.replace(Some(dir.to_path_buf())));
    let result = body();
    TEST_INDEX.with(|slot| {
        slot.replace(previous);
    });
    result
}

#[cfg(test)]
pub(crate) fn test_index_active() -> bool {
    TEST_INDEX.with(|slot| slot.borrow().is_some())
}

/// Current wall-clock time in Unix seconds.
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

/// `state_dir()/cache-index`, or `TOPOS_CACHE_INDEX_DIR` when set.
///
/// The override exists for test/CI isolation. It is intentionally not used
/// for operational leases, which must stay under the repository.
pub fn cache_index_dir() -> Result<PathBuf, String> {
    if let Some(raw) = std::env::var_os("TOPOS_CACHE_INDEX_DIR") {
        let path = PathBuf::from(raw);
        if !path.as_os_str().is_empty() {
            return Ok(if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|e| format!("current directory: {e}"))?
                    .join(path)
            });
        }
    }
    let home = home_dir()?;
    Ok(state_dir(&home).join("cache-index"))
}

/// A handle to one inventory directory.
#[derive(Debug, Clone)]
pub struct Inventory {
    dir: PathBuf,
}

impl Inventory {
    pub fn open() -> Result<Self, String> {
        #[cfg(test)]
        if let Some(dir) = TEST_INDEX.with(|slot| slot.borrow().clone()) {
            return Ok(Self { dir });
        }
        Ok(Self {
            dir: cache_index_dir()?,
        })
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn entries_dir(&self) -> PathBuf {
        self.dir.join(ENTRIES_DIR)
    }

    fn lock_path(&self) -> PathBuf {
        self.dir.join(LOCK_FILE)
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.entries_dir().join(format!("{key}.json"))
    }

    fn ensure_dir(&self) -> Result<(), String> {
        std::fs::create_dir_all(self.entries_dir())
            .map_err(|e| format!("creating cache inventory {}: {e}", self.dir.display()))?;
        harden(&self.dir, 0o700);
        harden(&self.entries_dir(), 0o700);
        Ok(())
    }

    fn open_lock(&self) -> Result<File, String> {
        self.ensure_dir()?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.lock_path())
            .map_err(|e| format!("opening cache inventory lock: {e}"))?;
        harden(&self.lock_path(), 0o600);
        Ok(file)
    }

    /// Read every valid record. Never fails and never creates the inventory:
    /// a status observation must not write into the user's state directory.
    /// Malformed/unknown entries and a missing directory are reported through
    /// `diagnostics`.
    pub fn records(&self) -> (Vec<CacheRecord>, Vec<String>) {
        let mut diagnostics = Vec::new();
        if !self.dir.exists() {
            return (Vec::new(), diagnostics);
        }
        let entries = self.entries_dir();
        if !entries.exists() {
            return (Vec::new(), diagnostics);
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.lock_path())
            .and_then(|file| {
                file.lock_shared()?;
                Ok(file)
            });
        let _lock = match lock {
            Ok(file) => Some(file),
            Err(error) => {
                diagnostics.push(format!(
                    "reading cache inventory without a shared lock: {error}"
                ));
                None
            }
        };

        let mut by_key: BTreeMap<String, CacheRecord> = BTreeMap::new();
        let Ok(read_dir) = std::fs::read_dir(&entries) else {
            return (Vec::new(), diagnostics);
        };
        let mut paths: Vec<PathBuf> = read_dir
            .flatten()
            .filter(|entry| {
                entry
                    .file_type()
                    .map(|kind| kind.is_file() && !kind.is_symlink())
                    .unwrap_or(false)
            })
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let raw = match std::fs::read_to_string(&path) {
                Ok(raw) => raw,
                Err(error) => {
                    diagnostics.push(format!("skipping {}: {error}", path.display()));
                    continue;
                }
            };
            let value: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(value) => value,
                Err(error) => {
                    diagnostics.push(format!(
                        "malformed inventory entry {}: {error}",
                        path.display()
                    ));
                    continue;
                }
            };
            let schema = value.get("schema").and_then(|v| v.as_str()).unwrap_or("");
            if schema != ENTRY_SCHEMA {
                diagnostics.push(format!(
                    "skipping inventory entry {}: unknown schema '{schema}'",
                    path.display()
                ));
                continue;
            }
            let kind = value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if CacheKind::parse(kind).is_none() {
                diagnostics.push(format!(
                    "skipping inventory entry {}: unknown kind '{kind}'",
                    path.display()
                ));
                continue;
            }
            let record: CacheRecord = match serde_json::from_value(value) {
                Ok(record) => record,
                Err(error) => {
                    diagnostics.push(format!(
                        "invalid inventory entry {}: {error}",
                        path.display()
                    ));
                    continue;
                }
            };
            if record.path.is_empty() {
                diagnostics.push(format!(
                    "skipping inventory entry {}: empty path",
                    path.display()
                ));
                continue;
            }
            let key = record.key();
            match by_key.remove(&key) {
                Some(existing) => {
                    diagnostics.push(format!(
                        "duplicate inventory entries for {key}; merged timestamps"
                    ));
                    by_key.insert(key, existing.merge(&record));
                }
                None => {
                    by_key.insert(key, record);
                }
            }
        }
        (by_key.into_values().collect(), diagnostics)
    }

    /// Merge one record under the exclusive inventory lock.
    pub fn upsert(&self, update: RecordUpdate) -> Result<(), String> {
        let file = self.open_lock()?;
        file.lock()
            .map_err(|e| format!("locking cache inventory for write: {e}"))?;
        let key = update.key();
        let path = self.entry_path(&key);
        let existing = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<CacheRecord>(&raw).ok())
            .filter(|record| record.schema == ENTRY_SCHEMA && record.kind() == Some(update.kind));
        let mut record = update.into_record();
        if let Some(existing) = existing {
            record = existing.merge(&record);
        }
        let bytes = serde_json::to_vec_pretty(&record)
            .map_err(|e| format!("serializing cache inventory: {e}"))?;
        atomic_write(&path, &bytes)
    }

    /// Remove one record under the exclusive inventory lock. Missing is fine.
    pub fn remove(
        &self,
        kind: CacheKind,
        common_git_dir: Option<&str>,
        pr: Option<u64>,
        path: &str,
    ) -> Result<bool, String> {
        let file = self.open_lock()?;
        file.lock()
            .map_err(|e| format!("locking cache inventory for write: {e}"))?;
        let key = key_for(kind, common_git_dir, pr, path);
        let entry = self.entry_path(&key);
        match std::fs::remove_file(&entry) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!(
                "removing inventory entry {}: {error}",
                entry.display()
            )),
        }
    }
}

/// Replace `destination` with `bytes` through a unique same-directory temp.
pub(super) fn atomic_write(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "cache inventory entry has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    let unique = format!(
        ".tmp-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let temp = parent.join(unique);
    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|e| format!("creating {}: {e}", temp.display()))?;
        file.write_all(bytes)
            .map_err(|e| format!("writing {}: {e}", temp.display()))?;
        file.sync_all()
            .map_err(|e| format!("syncing {}: {e}", temp.display()))?;
    }
    harden(&temp, 0o600);
    let result = replace_file(&temp, destination);
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(not(windows))]
fn replace_file(temp: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(temp, destination)
        .map_err(|e| format!("replacing {}: {e}", destination.display()))
}

/// Windows `MoveFileEx`-style replacement is not assumed: swap through a
/// uniquely named backup so replacing an existing destination works even on
/// filesystems where rename does not overwrite. Readers hold the same lock.
#[cfg(windows)]
fn replace_file(temp: &Path, destination: &Path) -> Result<(), String> {
    let backup = destination.with_extension(format!(
        "bak-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    if destination.exists() {
        std::fs::rename(destination, &backup)
            .map_err(|e| format!("moving old {} aside: {e}", destination.display()))?;
    }
    match std::fs::rename(temp, destination) {
        Ok(()) => {
            let _ = std::fs::remove_file(&backup);
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::rename(&backup, destination);
            Err(format!("replacing {}: {error}", destination.display()))
        }
    }
}

#[cfg(unix)]
fn harden(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
}

#[cfg(not(unix))]
fn harden(_path: &Path, _mode: u32) {}

/// Canonical string for a path in a record, best-effort.
pub fn canonical_string(path: &Path) -> String {
    git::normalize_path(path).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn update(path: &str, used: Option<f64>) -> RecordUpdate {
        RecordUpdate {
            kind: CacheKind::Graph,
            managed: false,
            project_root: path.to_string(),
            common_git_dir: None,
            path: path.to_string(),
            pr: None,
            last_built: None,
            last_used: used,
            last_seen: None,
            compatibility: None,
        }
    }

    #[test]
    fn round_trip_and_monotonic_merge() {
        let dir = tempfile::tempdir().unwrap();
        let inventory = Inventory::at(dir.path().join("index"));
        inventory
            .upsert(update("/repo/.gitnexus", Some(100.0)))
            .unwrap();
        inventory
            .upsert(update("/repo/.gitnexus", Some(40.0)))
            .unwrap();
        let (records, diagnostics) = inventory.records();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].last_used, Some(100.0));
    }

    #[test]
    fn concurrent_writers_do_not_lose_timestamps() {
        let dir = tempfile::tempdir().unwrap();
        let inventory = Arc::new(Inventory::at(dir.path().join("index")));
        let mut handles = Vec::new();
        for value in 1..=12 {
            let inventory = Arc::clone(&inventory);
            handles.push(thread::spawn(move || {
                inventory
                    .upsert(update("/repo/.gitnexus", Some(f64::from(value))))
                    .unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let (records, _) = inventory.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].last_used, Some(12.0));
    }

    #[test]
    fn corrupt_and_unknown_entries_are_reported_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let inventory = Inventory::at(dir.path().join("index"));
        inventory.upsert(update("/repo/.gitnexus", None)).unwrap();
        let entries = inventory.entries_dir();
        std::fs::write(entries.join("bad.json"), "{").unwrap();
        std::fs::write(
            entries.join("unknown.json"),
            r#"{"schema":"topos.cache_entry.v99","kind":"graph","path":"/x"}"#,
        )
        .unwrap();
        std::fs::write(
            entries.join("kind.json"),
            r#"{"schema":"topos.cache_entry.v1","kind":"mystery","path":"/x"}"#,
        )
        .unwrap();
        let (records, diagnostics) = inventory.records();
        assert_eq!(records.len(), 1);
        assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
    }

    #[test]
    fn replacing_an_existing_entry_keeps_new_content() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("entry.json");
        std::fs::write(&destination, b"old").unwrap();
        atomic_write(&destination, b"new").unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"new");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
    }

    #[test]
    fn pr_identity_uses_common_dir_and_slot_not_project_root() {
        let a = RecordUpdate {
            kind: CacheKind::Pr,
            managed: true,
            project_root: "/clone-a".into(),
            common_git_dir: Some("/repo/.git".into()),
            path: "/repo/.git/topos-pr-7".into(),
            pr: Some(7),
            last_built: None,
            last_used: None,
            last_seen: None,
            compatibility: None,
        };
        let b = RecordUpdate {
            project_root: "/clone-a/linked".into(),
            ..a.clone()
        };
        assert_eq!(a.key(), b.key());
        let other_clone = RecordUpdate {
            common_git_dir: Some("/clone-b/.git".into()),
            path: "/clone-b/.git/topos-pr-7".into(),
            ..a
        };
        assert_ne!(b.key(), other_clone.key());
    }
}
