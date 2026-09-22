//! `topos depgraph generate` — ensure `.gitnexus/` is present and fresh.

use std::path::{Path, PathBuf};

use clap::Args;
use console::Style;
use topos_engine::adapters::gitnexus::generate_depgraph;
use topos_mcp::evaluation::depgraph_status;

use super::print_json;
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
/// immediately without regenerating. Otherwise create the worktrees, build
/// both graphs in parallel, and write `commits`. Never prints. Errors are
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
    let output = std::process::Command::new("gh")
        .current_dir(&repo)
        .args([
            "pr",
            "view",
            &args.pr.to_string(),
            "--json",
            "baseRefOid,headRefOid,isCrossRepository",
        ])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "gh is not installed, so pull request {} cannot be resolved.\n\
                     Install it with `brew install gh`.",
                    args.pr
                )
            } else {
                format!("running gh: {e}")
            }
        })?;
    if !output.status.success() {
        return Err(format!(
            "gh pr view {}: {}",
            args.pr,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("reading pull request {}: {e}", args.pr))?;
    if value
        .get("isCrossRepository")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return Err("cross-repository pull requests are not fetched by this command".to_string());
    }
    let sha = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("pull request {} has no {key}", args.pr))
    };
    let base = sha("baseRefOid")?;
    let head = sha("headRefOid")?;
    let root = git_root(&repo)?;

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

pub(crate) fn git_root(start: &std::path::Path) -> Result<PathBuf, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(start)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("running git: {e}"))?;
    if !output.status.success() {
        return Err("not a git repository".to_string());
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

fn ensure_worktree(
    repo: &std::path::Path,
    path: &std::path::Path,
    sha: &str,
) -> Result<(), String> {
    if path.join(".git").exists() {
        return Ok(());
    }
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "worktree",
            "add",
            "--detach",
            &path.display().to_string(),
            sha,
        ])
        .output()
        .map_err(|e| format!("running git: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "could not check out {sha}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
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
