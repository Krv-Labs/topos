//! Paths in and out of the summary.
//!
//! Out: every path the summary reports is absolute, under the repository
//! root, so an agent can pass it straight to another tool. In: `paths`
//! accepts what the summary reports or a repo-relative path, and is matched
//! against the scored files only; no file is read.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use super::super::model::{FileRecap, PrRecap};

/// Repo-relative paths made absolute under the repository root.
pub struct Absolute<'a>(&'a Path);

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
pub struct Resolved {
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
pub fn resolve_paths(recap: &PrRecap, root: &Path, requested: &[String]) -> Resolved {
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
