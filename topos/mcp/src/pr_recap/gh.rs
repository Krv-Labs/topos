//! Shared `git` and `gh` plumbing for the commands that work on a pull
//! request (`topos pr-recap`, `topos depgraph generate-pr`).
//!
//! Every helper shells out and reports failures as a `String` ready to show
//! the user.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The two ends of a pull request, as `gh` reports them.
pub struct PullRequestRefs {
    pub number: u64,
    pub head_ref: String,
    pub base_ref: String,
    pub head_sha: String,
    pub base_sha: String,
    /// The head branch lives in a fork, so `origin` has the head commit
    /// only under `refs/pull/<number>/head`.
    pub cross_repository: bool,
}

/// Resolve pull request `number` through `gh pr view`.
pub fn pull_request(repo: &Path, number: u64) -> Result<PullRequestRefs, String> {
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
    let cross_repository = value
        .get("isCrossRepository")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
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
        cross_repository,
    })
}

/// Make both ends of `refs` available locally. A fork's head is fetched
/// through `origin`'s `refs/pull/<number>/head`, which GitHub keeps on the
/// base repository, so no remote for the fork is needed.
pub fn ensure_pull_request_commits(repo: &Path, refs: &PullRequestRefs) -> Result<(), String> {
    ensure_commit(repo, &refs.base_sha)?;
    if !refs.cross_repository {
        return ensure_commit(repo, &refs.head_sha);
    }
    if resolve_commit(repo, &refs.head_sha).is_ok() {
        return Ok(());
    }
    let pull_ref = format!("refs/pull/{}/head", refs.number);
    let fetched = git(repo, &["fetch", "--no-tags", "origin", &pull_ref]);
    if resolve_commit(repo, &refs.head_sha).is_ok() {
        return Ok(());
    }
    Err(match fetched {
        Err(cause) => unfetched_fork_head(refs, &pull_ref, &cause),
        Ok(_) => moved_fork_head(refs, &pull_ref),
    })
}

fn unfetched_fork_head(refs: &PullRequestRefs, pull_ref: &str, cause: &str) -> String {
    format!(
        "could not fetch the head of pull request {number} ({head}) from origin's {pull_ref}.\n\
         {cause}\n\
         The pull request comes from a fork, and `origin` must be the repository it targets.\n\
         Fetch the commit yourself, then pass the commits directly:\n\
         git fetch <base-remote> {pull_ref}\n\
         topos pr-recap --base {base} --head {head}",
        number = refs.number,
        head = refs.head_sha,
        base = refs.base_sha,
    )
}

fn moved_fork_head(refs: &PullRequestRefs, pull_ref: &str) -> String {
    format!(
        "pull request {number} changed while it was being read: origin's {pull_ref} no longer \
         holds its head {head}.\n\
         Run the command again.",
        number = refs.number,
        head = refs.head_sha,
    )
}

pub fn missing_gh(number: u64) -> String {
    format!(
        "gh is not installed, so pull request {number} cannot be resolved.\n\
         Install it with `brew install gh`, or pass the commits directly:\n\
         topos pr-recap --base <base-sha> --head <head-sha>"
    )
}

/// Make `sha` available locally, fetching it from `origin` if needed.
pub fn ensure_commit(repo: &Path, sha: &str) -> Result<(), String> {
    if resolve_commit(repo, sha).is_ok() {
        return Ok(());
    }
    git(repo, &["fetch", "--no-tags", "origin", sha])
        .map(|_| ())
        .map_err(|_| format!("could not fetch {sha}. The commit may be from a fork."))
}

/// The full sha of the commit `rev` names in `repo`.
pub fn resolve_commit(repo: &Path, rev: &str) -> Result<String, String> {
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
pub fn merge_base(repo: &Path, base: &str, head: &str) -> Result<String, String> {
    let base = resolve_commit(repo, base)?;
    let head = resolve_commit(repo, head)?;
    let output = git(repo, &["merge-base", &base, &head]).map_err(|_| {
        if is_shallow(repo) {
            shallow_clone(&base, &head)
        } else {
            format!("{base} and {head} have no common ancestor")
        }
    })?;
    Ok(output.trim().to_string())
}

fn is_shallow(repo: &Path) -> bool {
    git(repo, &["rev-parse", "--is-shallow-repository"]).is_ok_and(|out| out.trim() == "true")
}

fn shallow_clone(base: &str, head: &str) -> String {
    format!(
        "this clone is shallow, so git cannot tell where {head} forked from {base}.\n\
         Fetch the full history: set `fetch-depth: 0` on actions/checkout, or run\n\
         git fetch --unshallow origin"
    )
}

fn is_safe_rev(rev: &str) -> bool {
    !rev.is_empty() && !rev.starts_with('-') && !rev.contains('\0')
}

pub fn git_root(start: &Path) -> Result<PathBuf, String> {
    let output = git_output(start, &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        return Err("not a git repository".to_string());
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

/// The `.git` directory every worktree of `repo` shares, as an absolute
/// path. In a linked worktree `<root>/.git` is a file pointing elsewhere,
/// so it cannot be joined onto.
pub fn git_common_dir(repo: &Path) -> Result<PathBuf, String> {
    let output = git(repo, &["rev-parse", "--git-common-dir"])?;
    // git prints it relative to `-C` when it can (`.git` in the main
    // worktree) and absolute otherwise.
    Ok(repo.join(output.trim()))
}

/// Run `git -C <repo> <args>` and return its stdout, untrimmed.
pub fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
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
    fn an_unfetched_fork_head_names_the_ref_and_the_workaround() {
        let refs = PullRequestRefs {
            number: 12,
            head_ref: "feature".to_string(),
            base_ref: "main".to_string(),
            head_sha: "h".repeat(40),
            base_sha: "b".repeat(40),
            cross_repository: true,
        };
        let message = unfetched_fork_head(&refs, "refs/pull/12/head", "git fetch: no such ref");
        for expected in [
            "pull request 12",
            "origin's refs/pull/12/head",
            "git fetch: no such ref",
            "git fetch <base-remote> refs/pull/12/head",
            &format!("--base {} --head {}", refs.base_sha, refs.head_sha),
        ] {
            assert!(message.contains(expected), "{expected:?} in:\n{message}");
        }
    }

    #[test]
    fn a_moved_fork_head_says_to_rerun_not_to_pass_the_old_commit() {
        let refs = PullRequestRefs {
            number: 12,
            head_ref: "feature".to_string(),
            base_ref: "main".to_string(),
            head_sha: "h".repeat(40),
            base_sha: "b".repeat(40),
            cross_repository: true,
        };
        let message = moved_fork_head(&refs, "refs/pull/12/head");
        assert!(message.contains("pull request 12 changed while it was being read"));
        assert!(message.contains("Run the command again"));
        assert!(!message.contains("--base"), "{message}");
    }

    #[test]
    fn a_shallow_clone_names_both_ways_to_get_the_history() {
        let message = shallow_clone("bbb", "hhh");
        for expected in [
            "this clone is shallow",
            "hhh forked from bbb",
            "`fetch-depth: 0` on actions/checkout",
            "git fetch --unshallow origin",
        ] {
            assert!(message.contains(expected), "{expected:?} in:\n{message}");
        }
    }

    #[test]
    fn resolve_commit_refuses_an_option_like_revision() {
        let err = resolve_commit(Path::new("."), "--output=/tmp/x").unwrap_err();
        assert!(err.contains("refusing"));
    }

    /// A linked worktree's `.git` is a file; its stores still belong under
    /// the main repository's `.git` directory.
    #[test]
    fn the_common_dir_of_a_linked_worktree_is_the_main_git_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("main");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]).unwrap();
        git(&repo, &["config", "user.email", "gh@example.com"]).unwrap();
        git(&repo, &["config", "user.name", "Gh"]).unwrap();
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        git(&repo, &["add", "."]).unwrap();
        git(&repo, &["commit", "-qm", "first"]).unwrap();
        let linked = dir.path().join("linked");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                &linked.display().to_string(),
            ],
        )
        .unwrap();
        assert!(linked.join(".git").is_file());

        let canonical = |path: PathBuf| std::fs::canonicalize(path).unwrap();
        let main_git = canonical(repo.join(".git"));
        let from_main = git_common_dir(&repo).unwrap();
        let from_linked = git_common_dir(&linked).unwrap();
        assert!(from_main.is_absolute() && from_linked.is_absolute());
        assert_eq!(canonical(from_main), main_git);
        assert_eq!(canonical(from_linked), main_git);
    }
}
