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

mod discovery;
mod git;
mod inventory;
mod marker;
mod registration;
mod status;

pub use discovery::{discover, DiscoverReport};
pub use git::{
    canonical_common_dir, git, git_common_dir, git_root, worktree_registrations,
    WorktreeRegistration,
};
pub use inventory::{
    cache_index_dir, now, CacheCompatibility, CacheKind, CacheRecord, Inventory, RecordUpdate,
    INDEX_SCHEMA,
};
pub use marker::{
    marker_path, read_marker, validate_marker, write_marker, PrStoreMarker, MARKER_FILE,
    MARKER_SCHEMA,
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
