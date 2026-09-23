//! Recap-specific git plumbing: which files changed, what each held at
//! a revision, and how much each one churned.

use std::collections::BTreeMap;
use std::path::Path;

use topos_engine::graphs::ast::languages::all_source_suffixes;

use super::model::FileChange;
use crate::commands::gh::git;

const SKIP_PREFIXES: &[&str] = &[
    "openwiki/",
    "target/",
    "node_modules/",
    "dist/",
    "vendor/",
    ".git/",
];

pub(super) struct DiffEntry {
    pub(super) status: String,
    pub(super) path: String,
    /// Pre-rename path, set only for `R*` entries. The base revision only
    /// has the old path, so callers reading the base side must use this
    /// instead of `path`.
    pub(super) old_path: Option<String>,
}

pub(super) struct Diff {
    pub(super) entries: Vec<DiffEntry>,
    pub(super) deleted: Vec<String>,
}

pub(super) fn worktree_files(repo: &Path, base: &str) -> Result<Diff, String> {
    let output = git(
        repo,
        &[
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            "--end-of-options",
            base,
        ],
    )?;
    let mut diff = parse_name_status(&output);
    // `git diff` never lists untracked files, but a new file the author has
    // not staged yet is still part of the edit under review.
    let untracked = git(repo, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    diff.entries.extend(
        untracked
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(|path| DiffEntry {
                status: "A".to_string(),
                path: path.to_string(),
                old_path: None,
            }),
    );
    Ok(diff)
}

/// Lines added plus removed per head path, from `git diff --numstat`.
/// `head` is `None` for the working tree. Binary files count as zero.
pub(super) fn churn(
    repo: &Path,
    base: &str,
    head: Option<&str>,
) -> Result<BTreeMap<String, usize>, String> {
    let mut args = vec![
        "diff",
        "--numstat",
        "-z",
        "--find-renames",
        "--end-of-options",
        base,
    ];
    args.extend(head);
    Ok(parse_numstat(&git(repo, &args)?))
}

/// `-z` numstat: `added\tremoved\tpath\0`, or for a rename
/// `added\tremoved\t\0old\0new\0`.
fn parse_numstat(output: &str) -> BTreeMap<String, usize> {
    let mut churn = BTreeMap::new();
    let mut fields = output.split('\0');
    while let Some(record) = fields.next() {
        let mut parts = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let path = if path.is_empty() {
            // Rename: skip the old path, keep the new one.
            fields.next();
            fields.next().unwrap_or("")
        } else {
            path
        };
        let lines = added.parse::<usize>().unwrap_or(0) + removed.parse::<usize>().unwrap_or(0);
        churn.insert(path.to_string(), lines);
    }
    churn
}

/// Keep the `max_files` entries with the most churn (ties: smaller path),
/// in their original order; return the rest, most churn first.
pub(super) fn cap_by_churn(
    entries: Vec<DiffEntry>,
    max_files: usize,
    churn: impl Fn(&str) -> usize,
) -> (Vec<DiffEntry>, Vec<DiffEntry>) {
    let mut ranked: Vec<(usize, usize)> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| (i, churn(&entry.path)))
        .collect();
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| entries[a.0].path.cmp(&entries[b.0].path))
    });
    let rank: BTreeMap<usize, usize> = ranked
        .iter()
        .enumerate()
        .map(|(position, (i, _))| (*i, position))
        .collect();
    let (kept, mut dropped): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .enumerate()
        .partition(|(i, _)| rank[i] < max_files);
    dropped.sort_by_key(|(i, _)| rank[i]);
    (
        kept.into_iter().map(|(_, entry)| entry).collect(),
        dropped.into_iter().map(|(_, entry)| entry).collect(),
    )
}

pub(super) fn changed_files(repo: &Path, base: &str, head: &str) -> Result<Diff, String> {
    let output = git(
        repo,
        &[
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            "--end-of-options",
            &format!("{base}...{head}"),
        ],
    )?;
    Ok(parse_name_status(&output))
}

/// `-z` name-status: `status\0path\0`, or for a rename or copy
/// `R100\0old\0new\0`. Without `-z`, git quotes any path with unusual
/// characters and the quoted form names no file.
fn parse_name_status(output: &str) -> Diff {
    let mut entries = Vec::new();
    let mut deleted = Vec::new();
    let mut fields = output.split('\0');
    while let Some(status) = fields.next() {
        if status.is_empty() {
            continue;
        }
        let old_path = (status.starts_with('R') || status.starts_with('C'))
            .then(|| fields.next())
            .flatten();
        let Some(path) = fields.next().filter(|path| !path.is_empty()) else {
            continue;
        };
        if status.starts_with('D') {
            deleted.push(path.to_string());
        } else if status.starts_with('A') || status.starts_with('M') || status.starts_with('R') {
            entries.push(DiffEntry {
                status: status.to_string(),
                path: path.to_string(),
                old_path: old_path.map(str::to_string),
            });
        }
    }
    Diff { entries, deleted }
}

pub(super) fn skip_reason(entry: &DiffEntry) -> Option<String> {
    let path = entry.path.replace('\\', "/");
    if SKIP_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return Some("generated or vendor path".to_string());
    }
    let suffixes = all_source_suffixes();
    let supported = suffixes.iter().any(|suffix| path.ends_with(suffix));
    if !supported {
        return Some("not a supported source file".to_string());
    }
    None
}

pub(super) fn show_file(repo: &Path, rev: &str, path: &str) -> Result<String, String> {
    git(
        repo,
        &["show", "--end-of-options", &format!("{rev}:{path}")],
    )
    .map_err(|_| format!("could not read {path} at {rev}"))
}

pub(super) fn file_change(status: &str) -> FileChange {
    match status.chars().next() {
        Some('A') => FileChange::Added,
        Some('R') => FileChange::Renamed,
        _ => FileChange::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_reads_renames_and_binaries() {
        let churn = parse_numstat("3\t1\tsrc/a.py\0-\t-\tlogo.png\x002\t2\t\0old.py\0new.py\0");
        assert_eq!(churn["src/a.py"], 4);
        assert_eq!(churn["logo.png"], 0);
        assert_eq!(churn["new.py"], 4);
        assert!(!churn.contains_key("old.py"));
    }

    #[test]
    fn name_status_reads_renames_and_deletions() {
        let diff = parse_name_status("M\0src/a b.py\0R090\0old.py\0new.py\0D\0gone.py\0A\0é.py\0");
        let entries: Vec<(&str, &str, Option<&str>)> = diff
            .entries
            .iter()
            .map(|e| (e.status.as_str(), e.path.as_str(), e.old_path.as_deref()))
            .collect();
        assert_eq!(
            entries,
            [
                ("M", "src/a b.py", None),
                ("R090", "new.py", Some("old.py")),
                ("A", "é.py", None),
            ]
        );
        assert_eq!(diff.deleted, ["gone.py"]);
    }
}
