//! Thin Git helpers for cache discovery and ownership validation.
//!
//! These live here rather than in `pr_recap::gh` because the cache commands
//! need byte-oriented plumbing (`worktree list --porcelain -z`) and lexical
//! path comparison for entries that may already be missing. They still call
//! Git for worktree, submodule, and common-directory resolution rather than
//! parsing repository layout themselves.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

/// One live or stale `git worktree` registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRegistration {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub prunable: bool,
}

impl WorktreeRegistration {
    pub fn is_main(&self, common_dir: &Path) -> bool {
        normalize_path(&self.path) == normalize_path(common_dir)
            || self.path.file_name().is_some_and(|name| name == ".git")
    }
}

/// Canonicalize `path` when it exists, else normalize `.`/`..` lexically.
///
/// The lexical fallback is only used for entries that are already missing;
/// it lets a stale registration be compared with the path it claims without
/// resolving through a parent that no longer exists.
pub fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn git_output(root: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "git is not available".to_string()
            } else {
                format!("running git: {e}")
            }
        })
}

/// Run `git -C root ...`, returning stdout on success.
pub fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_output(root, args)?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// The repository root containing `start`, canonicalized.
pub fn git_root(start: &Path) -> Result<PathBuf, String> {
    let output = git_output(start, &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        return Err("not a git repository".to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let path = PathBuf::from(text.trim());
    Ok(std::fs::canonicalize(&path).unwrap_or(path))
}

/// The `.git` directory shared by every worktree of `repo`.
pub fn git_common_dir(repo: &Path) -> Result<PathBuf, String> {
    let output = git(repo, &["rev-parse", "--git-common-dir"])?;
    let raw = PathBuf::from(output.trim());
    let joined = if raw.is_absolute() {
        raw
    } else {
        repo.join(raw)
    };
    Ok(std::fs::canonicalize(&joined).unwrap_or_else(|_| normalize_path(&joined)))
}

/// Best-effort common directory for a path that may not be a repository.
pub fn canonical_common_dir(path: &Path) -> Option<PathBuf> {
    git_common_dir(path).ok()
}

/// Parse `git worktree list --porcelain -z`.
///
/// `-z` keeps paths with spaces or newlines from being split into fields.
/// The fallback to line mode is only for Git versions before 2.20; ordinary
/// paths are identical in both forms.
pub fn worktree_registrations(repo: &Path) -> Result<Vec<WorktreeRegistration>, String> {
    let output = git_output(repo, &["worktree", "list", "--porcelain", "-z"])?;
    if output.status.success() {
        return Ok(parse_worktrees_z(&output.stdout));
    }
    let output = git_output(repo, &["worktree", "list", "--porcelain"])?;
    if !output.status.success() {
        return Err(format!(
            "git worktree list: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(parse_worktrees_lines(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn finish_registration(
    path: Option<PathBuf>,
    head: Option<String>,
    branch: Option<String>,
    detached: bool,
    locked: bool,
    prunable: bool,
    out: &mut Vec<WorktreeRegistration>,
) {
    if let Some(path) = path {
        out.push(WorktreeRegistration {
            path,
            head,
            branch,
            detached,
            locked,
            prunable,
        });
    }
}

fn parse_worktrees_z(bytes: &[u8]) -> Vec<WorktreeRegistration> {
    let mut out = Vec::new();
    let mut path = None;
    let mut head = None;
    let mut branch = None;
    let mut detached = false;
    let mut locked = false;
    let mut prunable = false;
    for field in bytes.split(|byte| *byte == 0) {
        if field.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(field);
        if let Some(rest) = text.strip_prefix("worktree ") {
            finish_registration(
                path.take(),
                head.take(),
                branch.take(),
                detached,
                locked,
                prunable,
                &mut out,
            );
            path = Some(PathBuf::from(rest));
            detached = false;
            locked = false;
            prunable = false;
        } else if let Some(rest) = text.strip_prefix("HEAD ") {
            head = Some(rest.to_string());
        } else if let Some(rest) = text.strip_prefix("branch ") {
            branch = Some(rest.to_string());
        } else if text == "detached" {
            detached = true;
        } else if text == "locked" || text.starts_with("locked ") {
            locked = true;
        } else if text == "prunable" || text.starts_with("prunable ") {
            prunable = true;
        }
    }
    finish_registration(path, head, branch, detached, locked, prunable, &mut out);
    out
}

fn parse_worktrees_lines(text: &str) -> Vec<WorktreeRegistration> {
    let mut out = Vec::new();
    let mut path = None;
    let mut head = None;
    let mut branch = None;
    let mut detached = false;
    let mut locked = false;
    let mut prunable = false;
    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            finish_registration(
                path.take(),
                head.take(),
                branch.take(),
                detached,
                locked,
                prunable,
                &mut out,
            );
            detached = false;
            locked = false;
            prunable = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(rest));
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            head = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("branch ") {
            branch = Some(rest.to_string());
        } else if line == "detached" {
            detached = true;
        } else if line == "locked" || line.starts_with("locked ") {
            locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            prunable = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nul_separated_worktrees() {
        let raw = b"worktree /tmp/main\0HEAD abc\0branch refs/heads/main\0\0worktree /tmp/linked\0HEAD def\0detached\0locked reason\0\0";
        let regs = parse_worktrees_z(raw);
        assert_eq!(regs.len(), 2);
        assert_eq!(regs[0].path, PathBuf::from("/tmp/main"));
        assert_eq!(regs[0].branch.as_deref(), Some("refs/heads/main"));
        assert!(!regs[0].detached);
        assert_eq!(regs[1].path, PathBuf::from("/tmp/linked"));
        assert!(regs[1].detached && regs[1].locked);
    }

    #[test]
    fn lexical_normalization_handles_missing_tails() {
        let miss = Path::new("/definitely-missing-topos-cache/a/../b");
        assert_eq!(
            normalize_path(miss),
            PathBuf::from("/definitely-missing-topos-cache/b")
        );
    }
}
