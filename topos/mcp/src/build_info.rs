//! Server build identity and stale-process detection.
//!
//! Dogfooding repeatedly stalled on a question the server could answer for
//! itself: *is this process running the binary I just built, and which of my
//! registered Topos servers am I even talking to?* Answering it by hand meant
//! `stat`, `pgrep`, `ps`, `file`, and `strings` on the executable. Everything
//! needed is available to the process at runtime, so it is reported instead.
//!
//! **The build timestamp is the executable's own mtime, read at runtime — not
//! a value stamped in by a `build.rs`.** A build script that embeds a
//! timestamp has to rerun on every build to stay accurate, which forces a
//! relink each time and slows the very loop this exists to speed up; worse, a
//! cached value can report a build time that never happened. The mtime is
//! measured when asked and cannot drift. `CARGO_PKG_VERSION` alone is useless
//! here: every build on a branch reports the same `0.4.0`.
//!
//! Staleness is deliberately computed rather than left to the reader:
//! `is_stale()` compares the executable's *current* mtime against this
//! process's start time, so replacing the binary under a running server (what
//! `maturin develop` does while an MCP host keeps the old process alive) is
//! reported as a fact, not as two numbers to subtract.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Snapshot taken once, as early as the process can manage.
#[derive(Debug, Clone)]
pub struct BuildIdentity {
    /// Cargo package version — the release line, constant per branch.
    pub version: &'static str,
    /// Resolved path of the running executable, when the OS will say.
    pub exe_path: Option<PathBuf>,
    /// Executable mtime at startup, in epoch seconds: the build time.
    pub built_at: Option<u64>,
    /// Process start, in epoch seconds.
    pub started_at: u64,
    pub pid: u32,
}

static IDENTITY: OnceLock<BuildIdentity> = OnceLock::new();

fn epoch_secs(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

fn exe_mtime(path: Option<&PathBuf>) -> Option<u64> {
    let path = path?;
    epoch_secs(std::fs::metadata(path).ok()?.modified().ok()?)
}

/// Capture (once) and return this process's build identity.
///
/// Call early — [`crate::server::serve`] does — so `started_at` reflects
/// process start rather than the first curious reader.
pub fn identity() -> &'static BuildIdentity {
    IDENTITY.get_or_init(|| {
        let exe_path = std::env::current_exe().ok();
        BuildIdentity {
            version: env!("CARGO_PKG_VERSION"),
            built_at: exe_mtime(exe_path.as_ref()),
            exe_path,
            started_at: epoch_secs(SystemTime::now()).unwrap_or(0),
            pid: std::process::id(),
        }
    })
}

/// Semver version carrying build metadata, e.g. `0.4.0+build.1784947860`.
///
/// Semver permits anything after `+`, so this stays a valid version while
/// making two builds of the same branch distinguishable. It rides on the
/// existing `serverInfo.version` field, which costs no agent context and is
/// what an MCP host renders in its server list — the one place a human
/// disambiguates a local build from a registry-installed one.
pub fn version_with_build() -> String {
    match identity().built_at {
        Some(built) => format!("{}+build.{built}", identity().version),
        None => identity().version.to_string(),
    }
}

/// Executable mtime *now*, which may differ from the startup snapshot.
fn current_build_time() -> Option<u64> {
    exe_mtime(identity().exe_path.as_ref())
}

/// Seconds by which the on-disk build outruns a process start, or `None`
/// when the process is current (or the build time is unreadable).
///
/// Pure, so the staleness rule is testable without racing a real rebuild.
fn stale_delta(built: Option<u64>, started: u64) -> Option<u64> {
    built.filter(|b| *b > started).map(|b| b - started)
}

/// True when the executable on disk was rebuilt after this process started.
///
/// The MCP host owns the process lifetime, so a rebuild does not replace a
/// running server: tool calls keep hitting the old code until someone
/// restarts it. That is the failure this detects.
pub fn is_stale() -> bool {
    stale_delta(current_build_time(), identity().started_at).is_some()
}

/// Human phrasing for a duration, avoiding a calendar dependency.
///
/// Only relative spans are rendered. For the question actually being asked —
/// "is this binary newer than this process?" — a delta beats an absolute
/// timestamp, and it needs no date arithmetic or extra crate.
fn humanize(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

/// One-line warning when the running code is out of date, else `None`.
///
/// Returned only in the stale case so a healthy response pays nothing.
pub fn stale_banner() -> Option<String> {
    stale_delta(current_build_time(), identity().started_at).map(render_stale_banner)
}

fn render_stale_banner(delta: u64) -> String {
    format!(
        "⚠️ **Stale MCP server** — the binary on disk was rebuilt {} after this \
         process started, so these results come from the older code. Restart the \
         Topos MCP server. (See `topos://build`.)",
        humanize(delta)
    )
}

/// Full identity report, served as the `topos://build` resource and by
/// `topos-mcp --version`.
pub fn render() -> String {
    let id = identity();
    let mut out = String::new();
    out.push_str("# Topos MCP build identity\n\n");
    out.push_str(&format!("- **version**: `{}`\n", version_with_build()));
    out.push_str(&format!(
        "- **executable**: `{}`\n",
        id.exe_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".into())
    ));
    match id.built_at {
        Some(b) => out.push_str(&format!("- **built at**: {b} (epoch seconds)\n")),
        None => out.push_str("- **built at**: <unreadable>\n"),
    }
    out.push_str(&format!(
        "- **process**: pid {} started {} (epoch seconds)\n",
        id.pid, id.started_at
    ));
    out.push_str(&format!(
        "- **file root**: `{}`\n",
        crate::security::resolve_file_root()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|e| format!("<unresolved: {e}>"))
    ));
    match (is_stale(), current_build_time()) {
        (true, Some(built)) => out.push_str(&format!(
            "- **stale**: **yes** — rebuilt {} after this process started; restart the server\n",
            humanize(built.saturating_sub(id.started_at))
        )),
        _ => out.push_str("- **stale**: no — this process is running the binary on disk\n"),
    }
    out.push_str(
        "\nA rebuild does not replace a running server: the MCP host owns the \
         process, so tool calls keep reaching the old code until it is restarted. \
         If several Topos servers are registered (a local build alongside a \
         registry-installed one), compare **executable** above against the one \
         you meant to exercise.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_carries_build_metadata_and_stays_semver() {
        let v = version_with_build();
        assert!(
            v.starts_with(env!("CARGO_PKG_VERSION")),
            "must extend the package version, got {v}"
        );
        if let Some((_, meta)) = v.split_once('+') {
            // Semver build metadata: dot-separated alphanumerics/hyphens.
            assert!(
                meta.split('.').all(|part| !part.is_empty()
                    && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')),
                "invalid semver build metadata in {v}"
            );
        }
    }

    #[test]
    fn identity_is_stable_across_calls() {
        let a = identity();
        let b = identity();
        assert_eq!(a.started_at, b.started_at);
        assert_eq!(a.pid, b.pid);
        assert_eq!(a.pid, std::process::id());
    }

    /// A freshly started process must never accuse itself of being stale —
    /// the test binary was built before this test ran.
    #[test]
    fn a_running_process_is_not_stale_against_its_own_executable() {
        assert!(!is_stale());
        assert!(stale_banner().is_none());
    }

    #[test]
    fn render_reports_identity_and_a_no_stale_verdict() {
        let out = render();
        assert!(out.contains("**version**"));
        assert!(out.contains("**executable**"));
        assert!(out.contains("**file root**"));
        assert!(out.contains("**stale**: no"));
    }

    /// The staleness rule, exercised without racing an actual rebuild.
    #[test]
    fn stale_delta_fires_only_when_the_build_outruns_the_process() {
        // Built before the process started: the normal, healthy case.
        assert_eq!(stale_delta(Some(1_000), 2_000), None);
        // Built at exactly the start instant — not stale; a rebuild has to
        // strictly follow the start to have been missed by this process.
        assert_eq!(stale_delta(Some(2_000), 2_000), None);
        // Rebuilt underneath a running server: this is the dogfooding trap.
        assert_eq!(stale_delta(Some(2_180), 2_000), Some(180));
        // Unreadable executable mtime must never assert staleness.
        assert_eq!(stale_delta(None, 2_000), None);
    }

    #[test]
    fn stale_banner_text_names_the_remedy() {
        let banner = render_stale_banner(180);
        assert!(banner.contains("Stale MCP server"));
        assert!(banner.contains("3m"), "should humanize the delta: {banner}");
        assert!(
            banner.contains("Restart"),
            "an agent needs the remedy, not just the diagnosis: {banner}"
        );
        assert!(banner.contains("topos://build"));
    }

    #[test]
    fn humanize_scales_units() {
        assert_eq!(humanize(45), "45s");
        assert_eq!(humanize(600), "10m");
        assert_eq!(humanize(7200), "2h");
        assert_eq!(humanize(172_800), "2d");
    }
}
