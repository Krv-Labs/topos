//! `topos depgraph generate` — ensure `.gitnexus/` is present and fresh.

use std::path::PathBuf;

use clap::Args;
use console::Style;
use topos_engine::adapters::gitnexus::generate_depgraph;
use topos_mcp::evaluation::depgraph_status;
use topos_mcp::pr_recap::store::prepare_pr_stores;

use super::print_json;
use crate::commands::gh::{ensure_commit, git_root, merge_base, pull_request};
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

    let stores = prepare_pr_stores(&root, args.pr, &base, &head, &|| {})?;
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
