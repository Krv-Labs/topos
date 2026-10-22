//! `topos cache` — inspect the cross-repository inventory.
//!
//! Status and discovery are deliberately read-only by default. Discovery can
//! refresh `last_seen` unless `--dry-run` is passed; neither command adopts a
//! legacy PR store or authorizes deletion.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use console::Style;
use topos_mcp::cache::{self, CacheEntryView, DiscoverReport, StatusReport};

use super::install::report;
use super::render::{guide, paint, RenderOptions};

#[derive(Args)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub action: CacheAction,
}

#[derive(Subcommand)]
pub enum CacheAction {
    /// Show known cache locations and their structural state.
    Status(CacheStatusArgs),
    /// Discover existing PR and graph stores under one or more paths.
    Discover(CacheDiscoverArgs),
}

#[derive(Args)]
pub struct CacheStatusArgs {
    /// Include every known repository, not only the current one.
    #[arg(long)]
    pub global: bool,
    /// Recursively estimate bytes in managed directories (slower).
    #[arg(long)]
    pub sizes: bool,
    /// Emit the stable `topos.cache.v1` JSON document.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct CacheDiscoverArgs {
    /// Repository/worktree roots, or parent directories to scan one level.
    #[arg(required = true)]
    pub paths: Vec<PathBuf>,
    /// Show what would be registered without writing inventory.
    #[arg(long)]
    pub dry_run: bool,
    /// Emit the stable `topos.cache.v1` JSON document.
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: CacheArgs) -> Result<(), String> {
    match args.action {
        CacheAction::Status(args) => run_status(args),
        CacheAction::Discover(args) => run_discover(args),
    }
}

fn run_status(args: CacheStatusArgs) -> Result<(), String> {
    let report = cache::status(args.global, args.sizes)?;
    if args.json {
        return print_json(&report);
    }
    print_status_human(&report);
    Ok(())
}

fn run_discover(args: CacheDiscoverArgs) -> Result<(), String> {
    let report = cache::discover(&args.paths, args.dry_run)?;
    if args.json {
        return print_json(&report);
    }
    print_discover_human(&report);
    Ok(())
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn print_status_human(report: &StatusReport) {
    let opts = RenderOptions::stdout();
    report::header(
        &format!(
            "Topos Cache Status ({})",
            if report.scope == "global" {
                "global"
            } else {
                "project"
            }
        ),
        false,
        opts,
    );
    if report.entries.is_empty() {
        report::detail(
            &paint("○", Style::new().dim(), opts),
            "no known cache locations",
        );
    }
    for entry in &report.entries {
        print_entry(entry, opts);
    }
    for diagnostic in &report.diagnostics {
        report::note(diagnostic, opts);
    }
    let mut footer = format!(
        "{} cache location{}",
        report.totals.entries,
        if report.totals.entries == 1 { "" } else { "s" }
    );
    if report.totals.estimated_bytes > 0 || report.totals.unavailable_sizes > 0 {
        footer.push_str(&format!(
            " · ~{} estimated in managed directories",
            human_bytes(report.totals.estimated_bytes)
        ));
        if report.totals.unavailable_sizes > 0 {
            footer.push_str(&format!(
                " ({} unavailable)",
                report.totals.unavailable_sizes
            ));
        }
    }
    report::footer(&footer, opts);
}

fn print_discover_human(report: &DiscoverReport) {
    let opts = RenderOptions::stdout();
    report::header("Topos Cache Discovery", report.dry_run, opts);
    if report.entries.is_empty() {
        report::detail(
            &paint("○", Style::new().dim(), opts),
            "nothing found under the supplied paths",
        );
    }
    for entry in &report.entries {
        print_entry(entry, opts);
    }
    for diagnostic in &report.diagnostics {
        report::note(diagnostic, opts);
    }
    let footer = format!(
        "{} location{} · {} managed{}",
        report.totals.entries,
        if report.totals.entries == 1 { "" } else { "s" },
        report.totals.managed,
        if report.dry_run {
            " · inventory not written"
        } else {
            ""
        }
    );
    report::footer(&footer, opts);
}

fn print_entry(entry: &CacheEntryView, opts: RenderOptions) {
    let title = match (&entry.kind[..], entry.pr) {
        ("pr", Some(pr)) => format!("PR #{pr}"),
        (kind, _) => kind.to_string(),
    };
    println!(
        "{}  {}  {}  {}",
        guide('│', opts),
        paint(&title, Style::new().bold(), opts),
        paint(
            entry.state.as_str(),
            state_style(entry.state.as_str()),
            opts
        ),
        if entry.managed {
            paint("managed", Style::new().green(), opts)
        } else {
            paint("unmanaged", Style::new().dim(), opts)
        }
    );
    println!(
        "{}    {}",
        guide('│', opts),
        paint(&entry.path, Style::new().dim(), opts)
    );
    let mut facts = Vec::new();
    if let Some(used) = entry.last_used {
        facts.push(format!("last used {}", age_label(used)));
    }
    if let Some(built) = entry.last_built {
        facts.push(format!("last built {}", age_label(built)));
    }
    if let Some(seen) = entry.last_seen {
        facts.push(format!("last seen {}", age_label(seen)));
    }
    if let Some(bytes) = entry.estimated_bytes {
        facts.push(format!("~{}", human_bytes(bytes)));
    }
    if let Some(error) = &entry.size_error {
        facts.push(format!("size unavailable: {error}"));
    }
    if let Some(compat) = &entry.compatibility {
        let generator = compat.generator.as_deref().unwrap_or("generator unknown");
        facts.push(format!("layout {} · {generator}", compat.layout));
    }
    if !facts.is_empty() {
        println!(
            "{}    {}",
            guide('│', opts),
            paint(facts.join(" · "), Style::new().dim(), opts)
        );
    }
    for diagnostic in &entry.diagnostics {
        println!(
            "{}    {} {}",
            guide('│', opts),
            paint("!", Style::new().yellow(), opts),
            paint(diagnostic, Style::new().yellow(), opts)
        );
    }
}

fn state_style(state: &str) -> Style {
    match state {
        "ready" => Style::new().green(),
        "in-use" | "incomplete" => Style::new().color256(208),
        "legacy-ambiguous" => Style::new().yellow(),
        "orphaned" | "corrupt" => Style::new().red(),
        _ => Style::new().dim(),
    }
}

fn age_label(then: f64) -> String {
    let now = cache::now();
    let seconds = (now - then).max(0.0) as u64;
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3_599 => format!("{}m ago", seconds / 60),
        3_600..=86_399 => format!("{}h ago", seconds / 3_600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use topos_mcp::cache::StructuralState;

    #[test]
    fn states_have_distinct_readable_labels() {
        assert_eq!(StructuralState::Ready.as_str(), "ready");
        assert_eq!(StructuralState::InUse.as_str(), "in-use");
        assert_eq!(
            StructuralState::LegacyAmbiguous.as_str(),
            "legacy-ambiguous"
        );
    }

    #[test]
    fn byte_estimates_use_binary_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
    }
}
