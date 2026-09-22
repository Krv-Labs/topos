//! `topos depgraph generate` — ensure `.gitnexus/` is present and fresh.

use std::path::{Path, PathBuf};

use clap::Args;
use console::Style;
use topos_engine::adapters::gitnexus::generate_depgraph;
use topos_mcp::evaluation::depgraph_status;

use super::print_json;
use crate::commands::gh::{ensure_commit, git, git_root, merge_base, pull_request, resolve_commit};
use crate::commands::render::{guide, guide_line, paint, RenderOptions};

#[derive(Args)]
pub struct GenerateArgs {
    /// Project directory to analyze (default: current directory).
    pub path: Option<PathBuf>,
    /// Regenerate even when the graph is already current.
    #[arg(long)]
    pub force: bool,
    /// Output the result as a single JSON object.
    #[arg(long)]
    pub json: bool,
}

pub fn run_generate(args: GenerateArgs) -> Result<(), String> {
    let target_dir = match args.path {
        Some(path) => path,
        None => std::env::current_dir().map_err(|e| format!("current directory: {e}"))?,
    };
    if !target_dir.is_dir() {
        return Err(format!("Not a directory: {}", target_dir.display()));
    }

    let target_file = target_dir.to_string_lossy().to_string();

    if !args.force {
        let status = depgraph_status(None, &target_dir, &target_file);
        if status.state == "present" {
            let message = "Dependency graph already current.".to_string();
            if args.json {
                print_json(&serde_json::json!({
                    "ok": true,
                    "generated": false,
                    "gitnexus_dir": status.gitnexus_dir,
                    "message": message,
                }))?;
            } else {
                let options = RenderOptions::stdout();
                println!(
                    "{}",
                    paint("◇  Dependency graph current", Style::new().bold(), options)
                );
                if let Some(dir) = status.gitnexus_dir {
                    println!("{}", guide_line(dir, Style::new().dim(), options));
                }
                println!("{}", guide('└', options));
                println!();
                println!(
                    "{}",
                    paint(
                        "Tip: run topos depgraph generate --force if results appear stale.",
                        Style::new().dim(),
                        options
                    )
                );
            }
            return Ok(());
        }
        if status.state == "schema_mismatch" {
            let message = status
                .detail
                .unwrap_or_else(|| "GitNexus store schema mismatch.".to_string());
            return Err(message);
        }
    }

    let result = generate_depgraph(&target_dir, args.json, None);
    finish_generate(args.json, result)
}

/// Re-export so callers (e.g. `pr-recap`) don't need to reach into
/// `topos_engine` directly to decide whether to attempt graph generation.
pub(crate) use topos_engine::adapters::gitnexus::gitnexus_available;

/// Paths to the worktrees (and their shared parent) backing a PR's coupling
/// graphs.
pub(crate) struct PrStores {
    pub(crate) parent: PathBuf,
    pub(crate) base: PathBuf,
    pub(crate) head: PathBuf,
}

/// Ensure both coupling stores exist for `base_sha`/`head_sha` under
/// `<repo_root>/.git/topos-pr-<pr>/`. Idempotent: if `commits` already
/// records these two shas and both `<side>/.gitnexus` dirs exist, return
/// immediately without regenerating. Otherwise move the worktrees onto the
/// requested commits, build both graphs in parallel, and write `commits`.
/// `commits` is removed before anything is rebuilt, so it only ever names
/// the commits the graphs on disk were built from. Never prints. Errors are
/// `String`.
pub(crate) fn prepare_pr_stores(
    repo_root: &Path,
    pr: u64,
    base_sha: &str,
    head_sha: &str,
) -> Result<PrStores, String> {
    let parent = repo_root.join(".git").join(format!("topos-pr-{}", pr));
    let base_tree = parent.join("base");
    let head_tree = parent.join("head");

    let commits_path = parent.join("commits");
    if let Ok(existing) = std::fs::read_to_string(&commits_path) {
        let mut lines = existing.lines();
        if lines.next() == Some(base_sha)
            && lines.next() == Some(head_sha)
            && base_tree.join(".gitnexus").exists()
            && head_tree.join(".gitnexus").exists()
        {
            return Ok(PrStores {
                parent,
                base: base_tree,
                head: head_tree,
            });
        }
    }

    match std::fs::remove_file(&commits_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("clearing graph commits: {e}")),
    }
    std::fs::create_dir_all(&parent).map_err(|e| format!("creating graph worktrees: {e}"))?;
    ensure_worktree(repo_root, &base_tree, base_sha)?;
    ensure_worktree(repo_root, &head_tree, head_sha)?;

    let base_job = std::thread::spawn({
        let path = base_tree.clone();
        move || generate_depgraph(&path, true, None)
    });
    let head_job = std::thread::spawn({
        let path = head_tree.clone();
        move || generate_depgraph(&path, true, None)
    });
    let base_result = base_job
        .join()
        .map_err(|_| "base graph generation failed".to_string())?;
    let head_result = head_job
        .join()
        .map_err(|_| "head graph generation failed".to_string())?;
    if !base_result.ok {
        return Err(format!("base graph: {}", base_result.message));
    }
    if !head_result.ok {
        return Err(format!("head graph: {}", head_result.message));
    }
    std::fs::write(&commits_path, format!("{base_sha}\n{head_sha}\n"))
        .map_err(|e| format!("recording graph commits: {e}"))?;

    Ok(PrStores {
        parent,
        base: base_tree,
        head: head_tree,
    })
}

#[derive(Args)]
pub struct GeneratePrArgs {
    /// Pull request to prepare. Both commits are indexed; the review is not printed.
    pub pr: u64,
}

pub fn run_generate_pr(args: GeneratePrArgs) -> Result<(), String> {
    let repo = std::env::current_dir().map_err(|e| format!("current directory: {e}"))?;
    let refs = pull_request(&repo, args.pr)?;
    let root = git_root(&repo)?;
    ensure_commit(&root, &refs.base_sha)?;
    ensure_commit(&root, &refs.head_sha)?;
    // Build at the fork point, as `pr-recap` does, so both commands share
    // the stores' `commits` key instead of each rebuilding over the other.
    let base = merge_base(&root, &refs.base_sha, &refs.head_sha)?;
    let head = refs.head_sha;

    let options = RenderOptions::stderr();
    eprintln!(
        "{}",
        paint(
            format!("◇  Preparing coupling graphs for #{}", args.pr),
            Style::new().bold(),
            options,
        )
    );
    eprintln!(
        "{}",
        guide_line("base and head, in parallel", Style::new().dim(), options)
    );

    let stores = prepare_pr_stores(&root, args.pr, &base, &head)?;
    let parent = stores.parent;

    let options = RenderOptions::stdout();
    println!(
        "{}",
        paint(
            format!("◇  Prepared coupling graphs for #{}", args.pr),
            Style::new().bold(),
            options,
        )
    );
    println!(
        "{}",
        guide_line(parent.display(), Style::new().dim(), options)
    );
    println!("{}", guide('└', options));
    println!();
    println!(
        "{}",
        paint(
            format!("Next: topos pr-recap {}", args.pr),
            Style::new().dim(),
            options,
        )
    );
    Ok(())
}

/// Check out `sha`, detached, at `path`: a worktree of `repo` that topos
/// owns. A worktree already on `sha` is left alone, graph included. One on
/// another commit is forced onto `sha` and stripped of every untracked and
/// ignored file — the old `.gitnexus/` with them — so the next graph can
/// only reflect `sha`. A directory git no longer tracks as a worktree (or a
/// registration whose directory was deleted) is rebuilt from scratch.
fn ensure_worktree(repo: &Path, path: &Path, sha: &str) -> Result<(), String> {
    let wanted = resolve_commit(repo, sha)?;
    if path.join(".git").exists() {
        if let Ok(current) = git(path, &["rev-parse", "HEAD"]) {
            if current.trim() == wanted {
                return Ok(());
            }
            git(
                path,
                &["checkout", "--quiet", "--force", "--detach", &wanted],
            )
            .map_err(|e| format!("could not check out {sha}: {e}"))?;
            git(path, &["clean", "-ffdxq"])?;
            return Ok(());
        }
    }
    if path.exists() {
        std::fs::remove_dir_all(path)
            .map_err(|e| format!("removing broken worktree {}: {e}", path.display()))?;
    }
    // Forget a registration whose directory is gone, or `add` refuses the path.
    git(repo, &["worktree", "prune"])?;
    git(
        repo,
        &[
            "worktree",
            "add",
            "--detach",
            &path.display().to_string(),
            &wanted,
        ],
    )
    .map(|_| ())
    .map_err(|e| format!("could not check out {sha}: {e}"))
}

fn finish_generate(
    json: bool,
    result: topos_engine::adapters::gitnexus::DepgraphGenerationResult,
) -> Result<(), String> {
    if result.ok {
        if !json {
            let options = RenderOptions::stdout();
            println!(
                "{}",
                paint(
                    "◇  Dependency graph generated",
                    Style::new().bold(),
                    options
                )
            );
            if let Some(dir) = result.gitnexus_path {
                println!("{}", guide_line(dir.display(), Style::new().dim(), options));
            }
            println!("{}", guide('└', options));
        }
        Ok(())
    } else {
        Err(result.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repo with two commits touching `src/a.py`; returns the shas too.
    fn two_commit_repo() -> (tempfile::TempDir, PathBuf, String, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().to_path_buf();
        git(&repo, &["init", "-q"]).expect("init");
        git(&repo, &["config", "user.email", "depgraph@example.com"]).unwrap();
        git(&repo, &["config", "user.name", "Depgraph"]).unwrap();
        let commit = |body: &str, message: &str| {
            std::fs::create_dir_all(repo.join("src")).unwrap();
            std::fs::write(repo.join("src/a.py"), body).unwrap();
            git(&repo, &["add", "."]).unwrap();
            git(&repo, &["commit", "-qm", message]).unwrap();
            resolve_commit(&repo, "HEAD").unwrap()
        };
        let first = commit("x = 1\n", "first");
        let second = commit("x = 2\n", "second");
        (dir, repo, first, second)
    }

    fn head_of(path: &Path) -> String {
        resolve_commit(path, "HEAD").unwrap()
    }

    #[test]
    fn ensure_worktree_moves_a_stale_checkout_and_drops_its_graph() {
        let (_keep, repo, first, second) = two_commit_repo();
        let tree = repo.join(".git/topos-pr-1/head");
        ensure_worktree(&repo, &tree, &first).unwrap();
        assert_eq!(head_of(&tree), first);
        std::fs::create_dir_all(tree.join(".gitnexus")).unwrap();
        std::fs::write(tree.join("src/stray.py"), "y = 1\n").unwrap();
        std::fs::write(tree.join("src/a.py"), "x = 99\n").unwrap();

        ensure_worktree(&repo, &tree, &second).unwrap();

        assert_eq!(head_of(&tree), second);
        assert_eq!(
            std::fs::read_to_string(tree.join("src/a.py")).unwrap(),
            "x = 2\n"
        );
        assert!(!tree.join("src/stray.py").exists());
        assert!(!tree.join(".gitnexus").exists());
    }

    #[test]
    fn ensure_worktree_keeps_the_graph_of_a_current_checkout() {
        let (_keep, repo, first, _) = two_commit_repo();
        let tree = repo.join(".git/topos-pr-1/base");
        ensure_worktree(&repo, &tree, &first).unwrap();
        std::fs::create_dir_all(tree.join(".gitnexus")).unwrap();

        ensure_worktree(&repo, &tree, &first).unwrap();

        assert_eq!(head_of(&tree), first);
        assert!(tree.join(".gitnexus").exists());
    }

    #[test]
    fn ensure_worktree_recovers_a_deleted_worktree_directory() {
        let (_keep, repo, first, second) = two_commit_repo();
        let tree = repo.join(".git/topos-pr-1/head");
        ensure_worktree(&repo, &tree, &first).unwrap();
        std::fs::remove_dir_all(&tree).unwrap();

        ensure_worktree(&repo, &tree, &second).unwrap();

        assert_eq!(head_of(&tree), second);
    }

    #[test]
    fn ensure_worktree_replaces_a_directory_git_does_not_know() {
        let (_keep, repo, first, _) = two_commit_repo();
        let tree = repo.join(".git/topos-pr-1/base");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join("leftover.py"), "z = 1\n").unwrap();

        ensure_worktree(&repo, &tree, &first).unwrap();

        assert_eq!(head_of(&tree), first);
        assert!(!tree.join("leftover.py").exists());
    }

    #[test]
    fn prepare_pr_stores_forgets_old_commits_before_rebuilding() {
        let (_keep, repo, first, second) = two_commit_repo();
        let parent = repo.join(".git/topos-pr-1");
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::write(parent.join("commits"), format!("{first}\n{first}\n")).unwrap();

        // An unresolvable head fails before any graph is built; the stale
        // record must already be gone so no later run trusts it.
        let err = prepare_pr_stores(&repo, 1, &second, "0000000").err();

        assert!(err.is_some_and(|e| e.contains("could not resolve")));
        assert!(!parent.join("commits").exists());
    }
}
