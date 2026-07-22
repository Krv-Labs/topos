//! Git ref / mtime helpers used by freshness and dep-graph loading.

use std::path::Path;
use std::time::UNIX_EPOCH;

use topos_engine::adapters::gitnexus::resolve_lbug_store;

pub(crate) fn mtime_f64(path: &Path) -> Option<f64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
}

/// Cheap mtime signal for the gitnexus snapshot, used as a cache key.
pub fn gitnexus_mtime(gitnexus_dir: &Path, current_branch: Option<&str>) -> Option<f64> {
    let resolved = resolve_lbug_store(gitnexus_dir, current_branch);
    match resolved.path {
        Some(lbug) if lbug.exists() => mtime_f64(&lbug),
        _ => mtime_f64(gitnexus_dir),
    }
}

fn resolve_ref_mtime(git_dir: &Path, ref_line: &str) -> Option<f64> {
    let ref_path = git_dir.join(ref_line.strip_prefix("ref: ").unwrap_or(ref_line).trim());
    mtime_f64(&ref_path)
}

/// mtime of the HEAD ref (or HEAD itself when detached).
pub fn git_head_mtime(project_root: &Path) -> Option<f64> {
    let git_dir = project_root.join(".git");
    let head = git_dir.join("HEAD");
    let head_text = std::fs::read_to_string(&head).ok()?;
    let head_text = head_text.trim();
    if head_text.starts_with("ref: ") {
        resolve_ref_mtime(&git_dir, head_text)
    } else {
        mtime_f64(&head)
    }
}

fn packed_ref_sha(git_dir: &Path, git_ref: &str) -> Option<String> {
    let text = std::fs::read_to_string(git_dir.join("packed-refs")).ok()?;
    for line in text.lines() {
        if line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let (Some(sha), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        if name.trim() == git_ref {
            return Some(sha.trim().to_string());
        }
    }
    None
}

/// Current HEAD commit SHA, read from `.git` without shelling out.
pub fn git_head_sha(project_root: &Path) -> Option<String> {
    let git_dir = project_root.join(".git");
    let head_text = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head_text = head_text.trim();
    let Some(git_ref) = head_text.strip_prefix("ref: ") else {
        // Detached HEAD: contents are the SHA.
        return (!head_text.is_empty()).then(|| head_text.to_string());
    };
    let git_ref = git_ref.trim();
    match std::fs::read_to_string(git_dir.join(git_ref)) {
        Ok(sha) => {
            let sha = sha.trim().to_string();
            (!sha.is_empty()).then_some(sha)
        }
        Err(_) => packed_ref_sha(&git_dir, git_ref),
    }
}
