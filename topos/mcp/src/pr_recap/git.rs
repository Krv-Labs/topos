//! Recap-specific git plumbing: which files changed, what each held at
//! a revision, and how much each one churned.

use std::collections::BTreeMap;
use std::path::Path;

use topos_engine::graphs::ast::languages::language_for_path;

use super::gh::git;
use super::model::FileChange;

/// Unambiguous non-source directory names skipped at any depth in a diff.
///
/// Aligned with [`topos_engine::adapters::discovery`]'s `ALWAYS_SKIP_DIR_NAMES`
/// and `BUILTIN_IGNORE_DIR_NAMES`, plus `openwiki` (generated repo wiki).
/// Note that `"dist"` is intentionally handled separately in [`is_dist_output_dir`]
/// rather than listed here unconditionally so hand-written source modules such as
/// `src/dist/mod.rs` are not falsely skipped (matching `discovery.rs`'s
/// `ROOT_ONLY_SKIP_DIR_NAMES` rationale and GitHub Linguist / `go-enry`
/// generated-path conventions).
const SKIP_DIRS: &[&str] = &[
    "openwiki",
    "target",
    "node_modules",
    "vendor",
    ".git",
    ".gitnexus",
    ".next",
    ".turbo",
    ".venv",
    "venv",
    "__pycache__",
];

/// Ambiguous build/report directory names skipped at the repository root.
const SKIP_ROOT_DIRS: &[&str] = &["build", "builds", "out", "coverage", "dist"];

pub struct DiffEntry {
    pub status: String,
    pub path: String,
    /// Pre-rename path, set only for `R*` entries. The base revision only
    /// has the old path, so callers reading the base side must use this
    /// instead of `path`.
    pub old_path: Option<String>,
}

pub struct Diff {
    pub entries: Vec<DiffEntry>,
    pub deleted: Vec<String>,
}

pub fn worktree_files(repo: &Path, base: &str) -> Result<Diff, String> {
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
pub fn churn(
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
pub fn cap_by_churn(
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

pub fn changed_files(repo: &Path, base: &str, head: &str) -> Result<Diff, String> {
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

pub fn skip_reason(entry: &DiffEntry) -> Option<String> {
    let path = entry.path.replace('\\', "/");
    if is_generated_or_vendor_path(&path) {
        return Some("generated or vendor path".to_string());
    }
    if language_for_path(&path).is_none() {
        return Some("not a supported source file".to_string());
    }
    None
}

/// Classify whether a git-relative path points to generated build output,
/// vendored third-party code, or minified/bundled artifacts.
///
/// Follows GitHub Linguist (`generated.rb`) and `go-enry` path heuristics
/// alongside [`topos_engine::adapters::discovery`]:
/// - Root build/report directories (`SKIP_ROOT_DIRS`) and unambiguous
///   cache/vendor directories at any depth (`SKIP_DIRS`) are skipped.
/// - Nested `dist/` directories (e.g. `packages/web/dist/app.js` or
///   `builds/local-plugin/agent-smith/runtime/dist/server.js`) are skipped
///   unless directly inside a `src/` tree (preserving source modules like
///   `src/dist/mod.rs` or `crates/core/src/dist/worker.rs`).
/// - Minified and bundled filenames (`*.min.*`, `*.bundle.*`) are skipped.
fn is_generated_or_vendor_path(path: &str) -> bool {
    let mut parts = path.split('/');
    let file_name = parts.next_back().unwrap_or(path);
    let dirs: Vec<&str> = parts.collect();
    let root_skipped = dirs
        .first()
        .is_some_and(|root| SKIP_ROOT_DIRS.contains(root));
    root_skipped
        || dirs.iter().any(|dir| SKIP_DIRS.contains(dir))
        || is_dist_output_dir(&dirs)
        || file_name.contains(".min.")
        || file_name.contains(".bundle.")
}

fn is_dist_output_dir(dirs: &[&str]) -> bool {
    dirs.iter()
        .enumerate()
        .any(|(i, &dir)| dir == "dist" && (i == 0 || dirs[i - 1] != "src"))
}

pub fn show_file(repo: &Path, rev: &str, path: &str) -> Result<String, String> {
    git(
        repo,
        &["show", "--end-of-options", &format!("{rev}:{path}")],
    )
    .map_err(|_| format!("could not read {path} at {rev}"))
}

pub fn file_change(status: &str) -> FileChange {
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

    #[test]
    fn skip_reason_matches_suffixes_case_insensitively() {
        let entry = |path: &str| DiffEntry {
            status: "M".to_string(),
            path: path.to_string(),
            old_path: None,
        };
        assert_eq!(skip_reason(&entry("src/Main.RS")), None);
        assert_eq!(
            skip_reason(&entry("README.md")).as_deref(),
            Some("not a supported source file")
        );
    }

    #[test]
    fn skip_reason_catches_nested_dist_root_builds_and_minified_files() {
        let entry = |path: &str| DiffEntry {
            status: "M".to_string(),
            path: path.to_string(),
            old_path: None,
        };
        for skipped in [
            "dist/bundle.js",
            "packages/web/dist/server.js",
            "builds/local-plugin/agent-smith/runtime/dist/server.js",
            "builds/local-plugin/agent-smith/hooks/views.tsx",
            "packages/web/node_modules/pkg/index.js",
            "src/app.min.js",
            "src/app.bundle.js",
        ] {
            assert_eq!(
                skip_reason(&entry(skipped)).as_deref(),
                Some("generated or vendor path"),
                "{skipped} should be skipped"
            );
        }
        assert_eq!(skip_reason(&entry("src/build/mod.rs")), None);
        assert_eq!(skip_reason(&entry("src/dist/mod.rs")), None);
        assert_eq!(skip_reason(&entry("crates/core/src/dist/worker.rs")), None);
        assert_eq!(skip_reason(&entry("src/dist.rs")), None);
    }
}
