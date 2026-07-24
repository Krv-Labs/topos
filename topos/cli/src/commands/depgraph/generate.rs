//! `topos depgraph generate` — ensure `.gitnexus/` is present and fresh.

use std::path::PathBuf;

use clap::Args;
use topos_engine::adapters::gitnexus::generate_depgraph;
use topos_mcp::evaluation::depgraph_status;

use super::print_json;

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
                println!("{message}");
                if let Some(dir) = status.gitnexus_dir {
                    println!("  .gitnexus: {dir}");
                }
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
    if args.json {
        print_json(&serde_json::json!({
            "ok": result.ok,
            "returncode": result.returncode,
            "generated": result.ok,
            "gitnexus_dir": result.gitnexus_path.as_ref().map(|p| p.display().to_string()),
            "message": result.message,
        }))?;
    }

    if result.ok {
        if !args.json {
            println!("{}", result.message);
            if let Some(dir) = result.gitnexus_path {
                println!("  .gitnexus: {}", dir.display());
            }
        }
        Ok(())
    } else {
        Err(result.message)
    }
}
