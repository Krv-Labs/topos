//! Cross-repository visibility for Topos-owned generated data.
//!
//! This module is deliberately small: a versioned inventory of *candidate*
//! locations, read-only discovery, and a status model. It is not a storage
//! framework and never authorizes deletion by itself. On-disk validation in
//! [`crate::pr_recap::store`] remains the authority for managed PR stores.
//!
//! Inventory records live under `state_dir()/cache-index/`, one JSON file per
//! identity. `TOPOS_CACHE_INDEX_DIR` redirects that bookkeeping for tests and
//! CI; it deliberately does not move operational leases, which stay under the
//! repository's own Git common directory.

pub mod cleanup;
mod discovery;
mod git;
mod inventory;
mod lease;
mod marker;
mod ownership;
mod registration;
mod status;

pub use cleanup::{
    clean, parse_duration, prune, CleanOptions, MaintenanceEntry, MaintenanceReport,
    MaintenanceTotals, PruneOptions,
};
pub use discovery::{discover, DiscoverReport};
pub use git::{
    admin_gitdir_matches, admin_gitdir_path, canonical_common_dir, git, git_common_dir, git_root,
    registration_admin_dir, worktree_admin_dirs, worktree_registrations, WorktreeRegistration,
};
pub use inventory::{
    cache_index_dir, now, CacheCompatibility, CacheKind, CacheRecord, Inventory, RecordUpdate,
    INDEX_SCHEMA,
};
pub use lease::{lease_path, worktree_lock_path, LeaseError, PrLease, RepositoryLock};
pub use marker::{
    delete_marker_path, marker_path, read_delete_marker, read_marker, validate_delete_marker,
    validate_marker, write_delete_marker, write_marker, PrStoreDeleteMarker, PrStoreMarker,
    DELETE_MARKER_FILE, DELETE_MARKER_SCHEMA, MARKER_FILE, MARKER_LAYOUT, MARKER_SCHEMA,
};
pub use ownership::{
    adopt_legacy, assess_content, checkout_blockers, ensure_marker_for_build, identity,
    legacy_store, read_commits, registration_for, remove_disposable_graph,
    remove_stale_registration, repo_for_common, validate_store_marker, ContentReport, LegacyStore,
    StoreIdentity, KNOWN_ENTRIES,
};
pub use registration::{graph_used, pr_built, pr_used};
pub use status::{status, StatusEntry, StatusReport};

use serde::Serialize;

/// One stable, serializable row shared by status and discovery.
#[derive(Debug, Clone, Serialize)]
pub struct CacheEntryView {
    pub kind: String,
    pub path: String,
    pub project_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub common_git_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<u64>,
    pub managed: bool,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_built: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<CacheCompatibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_error: Option<String>,
    pub diagnostics: Vec<String>,
}

impl CacheEntryView {
    pub fn sort_key(&self) -> (u8, Option<u64>, String) {
        let kind = if self.kind == "pr" { 0 } else { 1 };
        (kind, self.pr, self.path.clone())
    }
}

/// The structural state of one candidate location.
///
/// Structural state is separate from a cleanup action outcome (`removed`,
/// `skipped`, ...). In particular, `ready` does not mean "safe to delete".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralState {
    Ready,
    Incomplete,
    InUse,
    Orphaned,
    LegacyAmbiguous,
    Missing,
    Corrupt,
}

impl StructuralState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Incomplete => "incomplete",
            Self::InUse => "in-use",
            Self::Orphaned => "orphaned",
            Self::LegacyAmbiguous => "legacy-ambiguous",
            Self::Missing => "missing",
            Self::Corrupt => "corrupt",
        }
    }
}
