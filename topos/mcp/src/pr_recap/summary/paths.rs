//! Paths in and out of the summary.
//!
//! Out: the file paths the summary reports in its structured fields are
//! absolute, under the repository root, so an agent can pass them straight
//! to another tool. In: `paths`
//! accepts what the summary reports or a repo-relative path, and is matched
//! against the scored files only; no file is read.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use super::super::model::{FileRecap, PrRecap};

/// Repo-relative paths made absolute under the repository root.
pub(super) struct Absolute<'a>(&'a Path);

impl<'a> Absolute<'a> {
    pub fn new(root: &'a Path) -> Self {
        Absolute(root)
    }

    /// `relative` under the root; an empty path (a range-wide finding)
    /// stays empty.
    pub fn path(&self, relative: &str) -> String {
        if relative.is_empty() {
            return String::new();
        }
        self.0.join(relative).display().to_string()
    }

    /// `file` with every path it carries made absolute.
    pub fn record(&self, file: &FileRecap) -> FileRecap {
        let mut file = file.clone();
        file.path = self.path(&file.path);
        if let Some(cluster) = &mut file.cluster {
            cluster.parent = self.path(&cluster.parent);
        }
        for spot in &mut file.hotspots {
            spot.path = self.path(&spot.path);
        }
        file
    }
}

/// A requested path that matched no scored file, and why.
#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct NotFound {
    /// As requested.
    pub path: String,
    pub reason: String,
}

/// What `paths` resolved to.
#[derive(Debug, Default)]
pub(crate) struct Resolved {
    /// Indexes into `recap.files`, in file order.
    pub found: Vec<usize>,
    pub missing: Vec<NotFound>,
}

/// Why a path could not be placed in the repository.
enum Unplaced {
    /// It resolves to somewhere outside the repository.
    Outside,
    /// It could not be resolved at all (a missing parent, a symlink loop)
    /// and is not lexically under the root.
    Unresolved,
}

/// Match each requested path to a scored file.
///
/// An exact match on a repo-relative path wins, so a file name that holds a
/// character another system reads as a separator is still found. Otherwise
/// the path is cleaned lexically (`.`, empty segments and `..`, which cannot
/// leave the repository) and, when absolute, made relative to the root with
/// symlinks resolved on both sides (`/var` is `/private/var` on macOS).
pub(crate) fn resolve_paths(recap: &PrRecap, root: &Path, requested: &[String]) -> Resolved {
    let mut found = BTreeSet::new();
    let mut missing: Vec<NotFound> = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in requested {
        match lookup(recap, root, raw) {
            Ok(index) => {
                found.insert(index);
            }
            Err((key, reason)) => {
                if seen.insert(key) {
                    missing.push(NotFound {
                        path: raw.clone(),
                        reason,
                    });
                }
            }
        }
    }
    Resolved {
        found: found.into_iter().collect(),
        missing,
    }
}

/// The scored file `raw` names, or the key it was looked up under and why
/// there is none.
fn lookup(recap: &PrRecap, root: &Path, raw: &str) -> Result<usize, (String, String)> {
    if raw.trim().is_empty() {
        return Err((raw.to_string(), "invalid path: empty".to_string()));
    }
    let position = |wanted: &str| recap.files.iter().position(|file| file.path == wanted);
    if let Some(index) = position(raw) {
        return Ok(index);
    }
    match relative_path(raw, root) {
        Ok(wanted) => position(&wanted).ok_or_else(|| {
            let reason = why_missing(recap, &wanted);
            (wanted, reason)
        }),
        Err(Unplaced::Outside) => Err((raw.to_string(), "outside the repository".to_string())),
        Err(Unplaced::Unresolved) => {
            Err((raw.to_string(), "could not resolve the path".to_string()))
        }
    }
}

fn why_missing(recap: &PrRecap, path: &str) -> String {
    if let Some(skipped) = recap.skipped.iter().find(|file| file.path == path) {
        return format!("skipped: {}", skipped.reason);
    }
    if recap.deleted.iter().any(|deleted| deleted == path) {
        return "deleted in this range".to_string();
    }
    "not changed in this range".to_string()
}

/// `raw` as a clean repo-relative `/` path.
fn relative_path(raw: &str, root: &Path) -> Result<String, Unplaced> {
    let path = Path::new(raw);
    let relative = if path.is_absolute() {
        under_root(path, root)?
    } else {
        clean(path).ok_or(Unplaced::Outside)?
    };
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(parts.join("/"))
}

/// The part of the absolute `path` under `root`: through symlinks when both
/// resolve, else lexically.
fn under_root(path: &Path, root: &Path) -> Result<PathBuf, Unplaced> {
    if let (Some(root), Some(path)) = (root.canonicalize().ok(), resolve(path)) {
        return path
            .strip_prefix(&root)
            .map(Path::to_path_buf)
            .map_err(|_| Unplaced::Outside);
    }
    let (root, path) = clean(root).zip(clean(path)).ok_or(Unplaced::Unresolved)?;
    path.strip_prefix(&root)
        .map(Path::to_path_buf)
        .map_err(|_| Unplaced::Unresolved)
}

/// `path` with symlinks resolved; when the file is gone, its directory.
fn resolve(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok().or_else(|| {
        let parent = path.parent()?.canonicalize().ok()?;
        Some(parent.join(path.file_name()?))
    })
}

/// `path` without `.` and empty segments, and with `..` applied; `None`
/// when a `..` would climb above where the path starts.
fn clean(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component.as_os_str()),
            Component::Normal(part) => {
                out.push(part);
                depth += 1;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                depth = depth.checked_sub(1)?;
                out.pop();
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::tests::{abs, fixture};
    use super::*;
    use crate::pr_recap::model::SkippedFile;
    use crate::pr_recap::tests::{branchy, commit_all, recap, write_files, write_repo};
    use crate::pr_recap::PrRecap;

    fn resolve(recap: &PrRecap, root: &Path, asked: &[&str]) -> Resolved {
        let asked: Vec<String> = asked.iter().map(|p| p.to_string()).collect();
        resolve_paths(recap, root, &asked)
    }

    fn placed(resolved: &Resolved) -> (usize, usize) {
        (resolved.found.len(), resolved.missing.len())
    }

    #[test]
    fn absolute_paths_resolve_through_symlinks() {
        let (_keep, repo) = fixture(3, 1);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let outside = tempfile::tempdir().unwrap();
        let link = outside.path().join("link");
        std::os::unix::fs::symlink(&repo, &link).unwrap();
        let real = repo.canonicalize().unwrap();
        for (root, base) in [
            (&repo, &link),
            (&link, &repo),
            (&repo, &real),
            (&real, &repo),
        ] {
            let path = base.join("src/big1.py").display().to_string();
            let resolved = resolve(&recap, root, &[path.as_str()]);
            assert_eq!(placed(&resolved), (1, 0), "{root:?} {base:?}");
        }
    }

    #[test]
    fn relative_paths_are_cleaned_lexically_and_cannot_leave_the_repository() {
        let (_keep, repo) = fixture(3, 1);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        for raw in ["././src/big1.py", "src/../src/big1.py", "./src//big1.py"] {
            assert_eq!(placed(&resolve(&recap, &repo, &[raw])), (1, 0), "{raw}");
        }
        let escaped = resolve(&recap, &repo, &["../x.py", "src/../../x.py", "/etc/passwd"]);
        assert!(escaped.found.is_empty());
        let reasons: Vec<&str> = escaped.missing.iter().map(|m| m.reason.as_str()).collect();
        assert_eq!(reasons[..2], ["outside the repository"; 2]);
    }

    #[test]
    fn a_backslash_is_part_of_a_file_name_on_unix() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        write_files(
            &repo,
            &[
                ("a\\b.py", &branchy("odd")),
                ("a/b.py", "def ok(x):\n    return x\n"),
            ],
        );
        commit_all(&repo, "add");
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let resolved = resolve(&recap, &repo, &["a\\b.py"]);
        assert_eq!(placed(&resolved), (1, 0));
        assert_eq!(recap.files[resolved.found[0]].path, "a\\b.py");
        assert_eq!(placed(&resolve(&recap, &repo, &["src\\b.py"])), (0, 1));
    }

    #[test]
    fn an_in_repo_path_with_a_missing_directory_is_resolved_lexically() {
        let (_keep, repo) = fixture(2, 0);
        let recap = recap(&repo, "HEAD~1", "HEAD", 0);
        let gone = abs(&repo, "gone/dir/x.py");
        let resolved = resolve(&recap, &repo, &[gone.as_str()]);
        assert_eq!(resolved.missing[0].reason, "not changed in this range");
        let unresolvable = resolve(&recap, &repo, &["/no/such/dir/x.py"]);
        assert_eq!(unresolvable.missing[0].reason, "could not resolve the path");
    }

    #[test]
    fn missing_paths_are_deduped_and_say_why() {
        let (_keep, repo) = fixture(2, 1);
        let mut recap = recap(&repo, "HEAD~1", "HEAD", 0);
        recap.skipped.push(SkippedFile {
            path: "docs/huge.py".to_string(),
            reason: "over the size limit".to_string(),
        });
        recap.deleted.push("old.py".to_string());
        let asked = [
            "nope.py",
            "./nope.py",
            "nope.py",
            "docs/huge.py",
            "old.py",
            "",
            "  ",
        ];
        let resolved = resolve(&recap, &repo, &asked);
        assert!(resolved.found.is_empty());
        let reasons: Vec<(&str, &str)> = resolved
            .missing
            .iter()
            .map(|m| (m.path.as_str(), m.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            vec![
                ("nope.py", "not changed in this range"),
                ("docs/huge.py", "skipped: over the size limit"),
                ("old.py", "deleted in this range"),
                ("", "invalid path: empty"),
                ("  ", "invalid path: empty"),
            ]
        );
    }
}
