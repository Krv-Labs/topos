//! Per-PR file leases and the short repository worktree-registration lock.
//!
//! Lock order is fixed: in-process PR mutex, then the per-PR file lease, then
//! the repository worktree lock, then the inventory lock as a leaf. A PR lease
//! is never taken while holding a repository or inventory lock, and the
//! repository lock never spans analysis.
//!
//! The lease file lives beside the PR directory, not inside it, so cleanup and
//! uninstall cannot accidentally remove the lock a concurrent worker holds.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::Duration;

use super::git;
use super::inventory::harden;

const WAIT_STEP: Duration = Duration::from_millis(25);

/// One in-process queue for a PR or repository key.
///
/// `std` has no owned mutex guard, so this is a tiny condition-variable lock
/// whose token is an `Arc`. It is held for exactly as long as the lease (or
/// repository lock) value lives.
#[derive(Debug, Default)]
struct ProcessLock {
    locked: Mutex<bool>,
    ready: Condvar,
}

#[derive(Debug)]
struct ProcessGuard {
    lock: Arc<ProcessLock>,
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        let mut locked = self
            .lock
            .locked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *locked = false;
        drop(locked);
        self.lock.ready.notify_one();
    }
}

static PR_MUTEXES: LazyLock<Mutex<HashMap<String, Arc<ProcessLock>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static REPO_MUTEXES: LazyLock<Mutex<HashMap<String, Arc<ProcessLock>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    /// Another worker or cleanup currently owns the lease.
    Busy,
    /// The waiter observed cancellation before acquiring the lease.
    Cancelled,
    Io(String),
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => write!(f, "in use"),
            Self::Cancelled => write!(f, "cancelled while waiting for the cache lease"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

fn process_lock(
    map: &'static LazyLock<Mutex<HashMap<String, Arc<ProcessLock>>>>,
    key: &str,
) -> Arc<ProcessLock> {
    let mut map = map.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    Arc::clone(map.entry(key.to_string()).or_default())
}

fn try_process_lock(
    map: &'static LazyLock<Mutex<HashMap<String, Arc<ProcessLock>>>>,
    key: &str,
) -> Result<ProcessGuard, LeaseError> {
    let lock = process_lock(map, key);
    let mut locked = lock
        .locked
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if *locked {
        return Err(LeaseError::Busy);
    }
    *locked = true;
    drop(locked);
    Ok(ProcessGuard { lock })
}

fn wait_process_lock(
    map: &'static LazyLock<Mutex<HashMap<String, Arc<ProcessLock>>>>,
    key: &str,
    cancel: Option<&AtomicBool>,
) -> Result<ProcessGuard, LeaseError> {
    let lock = process_lock(map, key);
    let mut locked = lock
        .locked
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(LeaseError::Cancelled);
        }
        if !*locked {
            *locked = true;
            drop(locked);
            return Ok(ProcessGuard { lock });
        }
        let (guard, _) = lock
            .ready
            .wait_timeout(locked, WAIT_STEP)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        locked = guard;
    }
}

pub fn lease_path(common_dir: &Path, pr: u64) -> PathBuf {
    common_dir.join(format!("topos-pr-{pr}.lease"))
}

pub fn worktree_lock_path(common_dir: &Path) -> PathBuf {
    common_dir.join("topos-worktrees.lock")
}

fn open_lock_file(path: &Path, what: &str) -> Result<File, LeaseError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(LeaseError::Io(format!(
                "refusing symlink at {what} {}",
                path.display()
            )));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(LeaseError::Io(format!(
                "{what} {} is not a regular file",
                path.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(LeaseError::Io(format!(
                "reading {what} {}: {error}",
                path.display()
            )));
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| LeaseError::Io(format!("opening {what} {}: {error}", path.display())))?;
    harden(path, 0o600);
    Ok(file)
}

/// An exclusive lease for one pull request.
///
/// Dropping it releases the file lock and the in-process guard. The caller
/// must keep it alive until the blocking work that reads or writes the store
/// has actually stopped; cleanup only probes it non-blockingly.
#[derive(Debug)]
pub struct PrLease {
    // File first, guard second: the OS lock is released before the in-process
    // mutex becomes available again.
    _file: File,
    _process: ProcessGuard,
    common_dir: PathBuf,
    pr: u64,
}

impl Drop for PrLease {
    fn drop(&mut self) {
        // Release the OS lock explicitly before the in-process guard becomes
        // available. File close also releases it, but an explicit unlock avoids
        // a window where a same-process waiter sees the token free before the
        // descriptor is gone.
        let _ = self._file.unlock();
    }
}

impl PrLease {
    pub fn acquire(common_dir: &Path, pr: u64) -> Result<Self, LeaseError> {
        Self::acquire_with_cancel(common_dir, pr, None)
    }

    /// Blocking wait that can be abandoned before ownership is acquired.
    /// Cancellation after ownership is observed by the caller, which must drop
    /// this value; this function never releases a lease it does not own.
    pub fn acquire_with_cancel(
        common_dir: &Path,
        pr: u64,
        cancel: Option<&AtomicBool>,
    ) -> Result<Self, LeaseError> {
        let common_dir = git::normalize_path(common_dir);
        let key = format!("{}\0{pr}", common_dir.to_string_lossy());
        let process = wait_process_lock(&PR_MUTEXES, &key, cancel)?;
        loop {
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                return Err(LeaseError::Cancelled);
            }
            let file = open_lock_file(&lease_path(&common_dir, pr), "PR lease")?;
            match file.try_lock() {
                Ok(()) => {
                    return Ok(Self {
                        _file: file,
                        _process: process,
                        common_dir,
                        pr,
                    });
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    drop(file);
                    std::thread::sleep(WAIT_STEP);
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(LeaseError::Io(format!(
                        "locking PR lease {}: {error}",
                        lease_path(&common_dir, pr).display()
                    )));
                }
            }
        }
    }

    /// Take the lease only if nobody else holds it.
    pub fn try_acquire(common_dir: &Path, pr: u64) -> Result<Self, LeaseError> {
        let common_dir = git::normalize_path(common_dir);
        let key = format!("{}\0{pr}", common_dir.to_string_lossy());
        let process = try_process_lock(&PR_MUTEXES, &key)?;
        let file = open_lock_file(&lease_path(&common_dir, pr), "PR lease")?;
        match file.try_lock() {
            Ok(()) => Ok(Self {
                _file: file,
                _process: process,
                common_dir,
                pr,
            }),
            Err(std::fs::TryLockError::WouldBlock) => Err(LeaseError::Busy),
            Err(std::fs::TryLockError::Error(error)) => Err(LeaseError::Io(format!(
                "locking PR lease {}: {error}",
                lease_path(&common_dir, pr).display()
            ))),
        }
    }

    pub fn pr(&self) -> u64 {
        self.pr
    }

    pub fn common_dir(&self) -> &Path {
        &self.common_dir
    }
}

/// The short repository-level lock serializing worktree registration changes.
/// It must never be held across graph analysis or scoring.
#[derive(Debug)]
pub struct RepositoryLock {
    _file: File,
    _process: ProcessGuard,
}

impl Drop for RepositoryLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

impl RepositoryLock {
    pub fn acquire(common_dir: &Path) -> Result<Self, LeaseError> {
        let common_dir = git::normalize_path(common_dir);
        let key = common_dir.to_string_lossy().into_owned();
        let process = wait_process_lock(&REPO_MUTEXES, &key, None)?;
        let path = worktree_lock_path(&common_dir);
        let file = open_lock_file(&path, "repository worktree lock")?;
        file.lock()
            .map_err(|error| LeaseError::Io(format!("locking {}: {error}", path.display())))?;
        Ok(Self {
            _file: file,
            _process: process,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn common() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        dir
    }

    #[test]
    fn a_second_lease_is_busy_until_the_first_drops() {
        let dir = common();
        let common = dir.path().join(".git");
        let first = PrLease::try_acquire(&common, 1).unwrap();
        assert_eq!(
            PrLease::try_acquire(&common, 1).unwrap_err(),
            LeaseError::Busy
        );
        let other = PrLease::try_acquire(&common, 2).unwrap();
        drop(first);
        drop(other);
        assert!(PrLease::try_acquire(&common, 1).is_ok());
    }

    #[test]
    fn a_cancelled_waiter_never_acquires_or_releases_the_lease() {
        let dir = common();
        let common = dir.path().join(".git");
        let held = PrLease::try_acquire(&common, 3).unwrap();
        let cancel = AtomicBool::new(true);
        assert_eq!(
            PrLease::acquire_with_cancel(&common, 3, Some(&cancel)).unwrap_err(),
            LeaseError::Cancelled
        );
        assert!(matches!(
            PrLease::try_acquire(&common, 3),
            Err(LeaseError::Busy)
        ));
        drop(held);
        assert!(PrLease::try_acquire(&common, 3).is_ok());
    }

    #[test]
    fn cancellation_after_acquiring_keeps_the_lease_owned() {
        let dir = common();
        let common = dir.path().join(".git");
        let cancel = AtomicBool::new(false);
        let lease = PrLease::acquire_with_cancel(&common, 4, Some(&cancel)).unwrap();
        cancel.store(true, Ordering::Relaxed);
        assert!(matches!(
            PrLease::try_acquire(&common, 4),
            Err(LeaseError::Busy)
        ));
        drop(lease);
        assert!(PrLease::try_acquire(&common, 4).is_ok());
    }

    #[test]
    fn the_repository_lock_is_short_lived_but_exclusive() {
        let dir = common();
        let common = dir.path().join(".git");
        let held = RepositoryLock::acquire(&common).unwrap();
        let blocked = std::thread::spawn({
            let common = common.clone();
            move || RepositoryLock::acquire(&common).is_ok()
        });
        std::thread::sleep(Duration::from_millis(80));
        assert!(!blocked.is_finished());
        drop(held);
        assert!(blocked.join().unwrap());
    }
}
