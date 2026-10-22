//! Versioned ownership marker for a managed PR store.
//!
//! The marker is created atomically before a new store's data, and legacy
//! stores are adopted only after full structural validation. A marker is
//! evidence, not proof: deletion and rebuild still validate the path,
//! registrations, and contents on disk.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::git::normalize_path;
use super::inventory::{atomic_write, now};

pub const MARKER_FILE: &str = "topos-cache.json";
pub const DELETE_MARKER_FILE: &str = "topos-delete.json";
pub const MARKER_SCHEMA: &str = "topos.pr_store.v1";
pub const MARKER_LAYOUT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrStoreMarker {
    pub schema: String,
    pub layout: u32,
    pub pr: u64,
    pub common_git_dir: String,
    pub project_root: String,
    pub created_at: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
}

pub fn marker_path(parent: &Path) -> PathBuf {
    parent.join(MARKER_FILE)
}

/// Read a marker, distinguishing "absent" from "unreadable or malformed".
pub fn read_marker(parent: &Path) -> Result<Option<PrStoreMarker>, String> {
    let path = marker_path(parent);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("reading {}: {error}", path.display())),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| format!("reading {}: {error}", path.display()))?;
    let marker: PrStoreMarker = serde_json::from_str(&raw)
        .map_err(|error| format!("malformed {}: {error}", path.display()))?;
    Ok(Some(marker))
}

/// Validate the marker against the exact store it claims to own.
pub fn validate_marker(
    marker: &PrStoreMarker,
    common_git_dir: &Path,
    pr: u64,
) -> Result<(), String> {
    if marker.schema != MARKER_SCHEMA {
        return Err(format!("unsupported marker schema '{}'", marker.schema));
    }
    if marker.layout != MARKER_LAYOUT {
        return Err(format!(
            "unsupported PR store layout {} (expected {MARKER_LAYOUT})",
            marker.layout
        ));
    }
    if marker.pr != pr {
        return Err(format!(
            "marker names PR {} but the directory is PR {pr}",
            marker.pr
        ));
    }
    let claimed = normalize_path(Path::new(&marker.common_git_dir));
    let expected = normalize_path(common_git_dir);
    if claimed != expected {
        return Err(format!(
            "marker common directory {} does not match {}",
            claimed.display(),
            expected.display()
        ));
    }
    Ok(())
}

/// Atomically create or replace the marker. Callers must validate any
/// existing marker and the on-disk layout first.
pub fn write_marker(
    parent: &Path,
    common_git_dir: &Path,
    project_root: &Path,
    pr: u64,
    generator: Option<String>,
) -> Result<PrStoreMarker, String> {
    let marker = PrStoreMarker {
        schema: MARKER_SCHEMA.to_string(),
        layout: MARKER_LAYOUT,
        pr,
        common_git_dir: normalize_path(common_git_dir)
            .to_string_lossy()
            .into_owned(),
        project_root: normalize_path(project_root).to_string_lossy().into_owned(),
        created_at: now(),
        generator,
    };
    let bytes = serde_json::to_vec_pretty(&marker)
        .map_err(|error| format!("serializing PR store marker: {error}"))?;
    atomic_write(&marker_path(parent), &bytes)?;
    Ok(marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_round_trip_and_exact_identity() {
        let dir = tempfile::tempdir().unwrap();
        let common = dir.path().join(".git");
        std::fs::create_dir_all(&common).unwrap();
        let parent = common.join("topos-pr-7");
        std::fs::create_dir_all(&parent).unwrap();
        let marker = write_marker(
            &parent,
            &common,
            dir.path(),
            7,
            Some("gitnexus@1.6.8".into()),
        )
        .unwrap();
        let read = read_marker(&parent).unwrap().unwrap();
        assert_eq!(read, marker);
        validate_marker(&read, &common, 7).unwrap();
        assert!(validate_marker(&read, &common, 8).is_err());
    }

    #[test]
    fn symlinked_marker_is_not_trusted() {
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            let parent = dir.path();
            std::os::unix::fs::symlink("/dev/null", marker_path(parent)).unwrap();
            assert!(read_marker(parent).is_err());
        }
    }
}
