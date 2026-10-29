//! The fact `topos_evaluate_project` states for a very large tree: how many
//! files were found and where they are, so a narrower `path` can be chosen.

use std::collections::BTreeMap;
use std::path::Path;

use crate::schemas::ProjectFileEntry;

/// Above this many files, `warnings` lists where the files are.
pub const LARGE_TREE_FILES: usize = 1_000;
/// Directories named in the warning.
const LISTED_DIRS: usize = 5;

/// The warning for `file_count` files under `root`, or `None` for a tree
/// that is not large. Counts come from the scored `entries`; `unread` files
/// are in none of them.
pub fn warning(
    file_count: usize,
    root: &Path,
    entries: &[ProjectFileEntry],
    unread: usize,
) -> Option<String> {
    if file_count <= LARGE_TREE_FILES {
        return None;
    }
    let ranked = directory_counts(root, entries);
    if ranked.is_empty() {
        return Some(format!(
            "{file_count} files found; none could be scored ({unread} unreadable). `path` can \
             name a narrower directory."
        ));
    }
    let scope = if ranked.len() > LISTED_DIRS {
        format!(
            "top {LISTED_DIRS} of {} top-level directories",
            ranked.len()
        )
    } else {
        "top-level directories".to_string()
    };
    let listed: Vec<String> = ranked
        .iter()
        .take(LISTED_DIRS)
        .map(|(dir, count)| format!("{dir} {count}"))
        .collect();
    let unread_note = if unread > 0 {
        format!(" {unread} unreadable file(s) are not in these counts.")
    } else {
        String::new()
    };
    Some(format!(
        "{file_count} files found; {scope} by scored-file count: {}.{unread_note} `path` can \
         name one of them.",
        listed.join(", ")
    ))
}

/// Scored files per top-level directory of `root` (`.` for files directly
/// in it), most files first.
fn directory_counts(root: &Path, entries: &[ProjectFileEntry]) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in entries {
        *counts.entry(top_level(root, &entry.filepath)).or_default() += 1;
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked
}

fn top_level(root: &Path, filepath: &str) -> String {
    let path = Path::new(filepath);
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut parts = relative.components();
    match (parts.next(), parts.next()) {
        (Some(dir), Some(_)) => dir.as_os_str().to_string_lossy().to_string(),
        _ => ".".to_string(),
    }
}
