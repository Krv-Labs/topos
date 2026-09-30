//! Shared `git` and `gh` plumbing for the commands that work on a pull
//! request (`topos pr-recap`, `topos depgraph generate-pr`).
//!
//! Every helper shells out and reports failures as a `String` ready to show
//! the user.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The two ends of a same-repository pull request, as `gh` reports them.
pub(crate) struct PullRequestRefs {
    pub(crate) number: u64,
    pub(crate) head_ref: String,
    pub(crate) base_ref: String,
    pub(crate) head_sha: String,
    pub(crate) base_sha: String,
}

/// Resolve pull request `number` through `gh pr view`. Cross-repository
/// pull requests are refused: their head commit is not in `origin`.
pub(crate) fn pull_request(repo: &Path, number: u64) -> Result<PullRequestRefs, String> {
    let output = Command::new("gh")
        .current_dir(repo)
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--json",
            "number,baseRefName,headRefName,baseRefOid,headRefOid,isCrossRepository",
        ])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                missing_gh(number)
            } else {
                format!("running gh: {e}")
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh pr view {number}: {}", stderr.trim()));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("reading pull request {number}: {e}"))?;
    if value
        .get("isCrossRepository")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return Err("cross-repository pull requests are not fetched by this command".to_string());
    }
    let field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("pull request {number} has no {key}"))
    };
    Ok(PullRequestRefs {
        number,
        head_ref: field("headRefName")?,
        base_ref: field("baseRefName")?,
        head_sha: field("headRefOid")?,
        base_sha: field("baseRefOid")?,
    })
}

pub(crate) fn missing_gh(number: u64) -> String {
    format!(
        "gh is not installed, so pull request {number} cannot be resolved.\n\
         Install it with `brew install gh`, or pass the commits directly:\n\
         topos pr-recap --base <base-sha> --head <head-sha>"
    )
}

/// Make `sha` available locally, fetching it from `origin` if needed.
pub(crate) fn ensure_commit(repo: &Path, sha: &str) -> Result<(), String> {
    if resolve_commit(repo, sha).is_ok() {
        return Ok(());
    }
    git(repo, &["fetch", "--no-tags", "origin", sha])
        .map(|_| ())
        .map_err(|_| format!("could not fetch {sha}. The commit may be from a fork."))
}

/// The full sha of the commit `rev` names in `repo`.
pub(crate) fn resolve_commit(repo: &Path, rev: &str) -> Result<String, String> {
    if !is_safe_rev(rev) {
        return Err(format!("refusing revision '{rev}'"));
    }
    let output = git(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{rev}^{{commit}}"),
        ],
    )
    .map_err(|_| format!("could not resolve '{rev}' to a commit"))?;
    Ok(output.trim().to_string())
}

/// Where `head` forked from `base`: the commit a change actually starts
/// from. A pull request's base ref has usually moved on since the fork;
/// reading the before-side there would charge the change with everything
/// merged into the base branch since.
pub(crate) fn merge_base(repo: &Path, base: &str, head: &str) -> Result<String, String> {
    let base = resolve_commit(repo, base)?;
    let head = resolve_commit(repo, head)?;
    let output = git(repo, &["merge-base", &base, &head])
        .map_err(|_| format!("{base} and {head} have no common ancestor"))?;
    Ok(output.trim().to_string())
}

fn is_safe_rev(rev: &str) -> bool {
    !rev.is_empty() && !rev.starts_with('-') && !rev.contains('\0')
}

pub(crate) fn git_root(start: &Path) -> Result<PathBuf, String> {
    let output = git_output(start, &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        return Err("not a git repository".to_string());
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

/// Run `git -C <repo> <args>` and return its stdout, untrimmed.
pub(crate) fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_output(repo, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {}: {}", args.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_output(repo: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_gh_names_the_workaround() {
        let message = missing_gh(12);
        assert!(message.contains("gh is not installed"));
        assert!(message.contains("brew install gh"));
        assert!(message.contains("--base <base-sha> --head <head-sha>"));
    }

    #[test]
    fn resolve_commit_refuses_an_option_like_revision() {
        let err = resolve_commit(Path::new("."), "--output=/tmp/x").unwrap_err();
        assert!(err.contains("refusing"));
    }
}
