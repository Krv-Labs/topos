//! The 24-hour throttle, the opt-out, and the one-line notices built on it.
//!
//! An unprompted update check is a network call in a program people run on
//! every keystroke, so it is governed by three independent brakes:
//!
//! * **A gate** — opt-out env var, CI, and (for the CLI) a terminal. An agent
//!   or a CI job has nobody to tell, and a hanging update check in a build is
//!   indistinguishable from a broken build.
//! * **A fetch throttle** — the network is touched at most once per
//!   [`THROTTLE_SECS`], which on the common path costs one `stat`.
//! * **A display throttle** — the *same* 24 hours, tracked separately.
//!
//! Fetch and display budgets are separate because they buy different things.
//! Fetching daily but showing hourly would nag; fetching hourly but showing
//! daily would hammer a URL nobody asked about. Both are daily, so the worst
//! case is one notice per day across every surface.
//!
//! **The MCP server reads this cache and never writes the fetch.** Blocking a
//! tool call on a network round-trip to decide whether to *mention* an update
//! is a bad trade for a server whose contract is to answer fast. The CLI does
//! the fetching, so anyone who only ever talks to the server over MCP simply
//! is not told — a missed notice, never a hung one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use super::release;

/// The cache file's name inside [`crate::paths::state_dir`].
const RECORD_FILE: &str = "update-check.json";

/// How long a fetch result stays fresh, and how long a notice stays quiet.
const THROTTLE_SECS: u64 = 24 * 60 * 60;

/// Opt-out, kept from the Python-era implementation so the name is not
/// invented twice.
const NO_NOTICES_ENV: &str = "TOPOS_NO_UPDATE_NOTICES";

/// What was last learned about the newest release.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// When the release server was last asked, in epoch seconds.
    #[serde(default)]
    pub checked_at: u64,
    /// When a notice was last shown, in epoch seconds. Distinct from
    /// `checked_at` so a fresh check does not reset the quiet period.
    #[serde(default)]
    pub notified_at: u64,
    /// Newest published version, or `None` when the last check failed.
    #[serde(default)]
    pub latest: Option<String>,
    /// The version that was running when the check ran.
    #[serde(default)]
    pub current: Option<String>,
    /// Every install found at check time, for the layout notice.
    #[serde(default)]
    pub installs: Vec<SeenInstall>,
}

/// One discovered binary, as remembered in the cache.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenInstall {
    pub path: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub channel: String,
}

fn cache_path(home: &Path) -> PathBuf {
    crate::paths::state_dir(home).join(RECORD_FILE)
}

/// Read the cache. A missing or corrupt file reads as an empty record, which
/// reads as "never checked" — the correct response to a truncated write.
pub fn load(home: &Path) -> Record {
    std::fs::read_to_string(cache_path(home))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Persist the record, replacing it atomically.
///
/// Rename rather than write-in-place so a reader — the other surface, running
/// concurrently in another process — never observes a half-written JSON file.
pub fn save(home: &Path, record: &Record) -> Result<(), String> {
    let path = cache_path(home);
    let dir = path.parent().unwrap_or(home);
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let temporary = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(record)
        .map_err(|e| format!("cannot serialize the update record: {e}"))?;
    std::fs::write(&temporary, body)
        .map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, &path).map_err(|e| format!("cannot update {}: {e}", path.display()))
}

/// Delete the cache file, so `topos uninstall` leaves nothing of ours behind.
///
/// Best-effort: a cache we cannot delete is a file, not a failure, and
/// uninstall is already committed to pruning the state directory by name.
pub fn remove_cache(home: &Path) {
    let path = cache_path(home);
    std::fs::remove_file(&path).ok();
}

/// Current time in epoch seconds, or `0` if the clock is before the epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Whether unprompted notices are permitted at all.
///
/// Deliberately does **not** consult terminal-ness: the MCP server has no
/// terminal by definition, and its notice is aimed at an agent, not a person.
pub fn allowed() -> bool {
    !flag_set(NO_NOTICES_ENV) && !ci()
}

/// Whether a human is at the terminal, for CLI-only notices.
///
/// All three streams, matching `interaction::Streams`: stderr carries the
/// notice, and a stdout or stdin that is redirected means a harness is
/// watching this process rather than a person.
pub fn interactive() -> bool {
    use std::io::IsTerminal;
    allowed()
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal()
}

/// An env var set to anything but empty, `0`, or `false`.
fn flag_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| {
        let value = value.to_string_lossy();
        !(value.is_empty() || value == "0" || value.eq_ignore_ascii_case("false"))
    })
}

/// `CI` set to anything but empty, `0`, or `false` — the same rule
/// `interaction::PromptEnv` applies, so one CI detector serves both.
fn ci() -> bool {
    flag_set("CI") || std::env::var_os("GH_PROMPT_DISABLED").is_some_and(|v| !v.is_empty())
}

/// True when the release server has not been asked within the throttle.
pub fn stale(record: &Record, at: u64) -> bool {
    at.saturating_sub(record.checked_at) >= THROTTLE_SECS
}

/// Ask the release server if the throttle has expired, and record the answer.
///
/// Returns `None` when the gate is closed or the cached answer is still fresh,
/// so the common case costs one file read and nothing else.
pub fn refresh(home: &Path) -> Option<Record> {
    if !allowed() {
        return None;
    }
    let at = now();
    let mut record = load(home);
    if !stale(&record, at) {
        return None;
    }
    record.checked_at = at;
    record.latest = release::latest();
    record.current = Some(env!("CARGO_PKG_VERSION").to_string());
    record.installs = super::installs::discover(home)
        .iter()
        .map(|install| SeenInstall {
            path: install.path.display().to_string(),
            version: install.version.clone(),
            channel: install.channel.label().to_string(),
        })
        .collect();
    // A cache we cannot write is not worth failing a command over.
    save(home, &record).ok();
    Some(record)
}

/// The newest version this record says is available, when it is newer than
/// `current`.
fn available<'a>(record: &'a Record, current: &str) -> Option<&'a str> {
    let latest = record.latest.as_deref()?;
    release::is_newer(latest, current).then_some(latest)
}

/// True when the notice period has elapsed since the last one shown.
fn due(record: &Record, at: u64) -> bool {
    at.saturating_sub(record.notified_at) >= THROTTLE_SECS
}

/// Claim the right to notify, and mark it claimed.
///
/// Returns `true` at most once per [`THROTTLE_SECS`] across every process and
/// surface, and marks the record so a second surface stays quiet.
fn claim_notice(home: &Path) -> bool {
    let at = now();
    let mut record = load(home);
    if !due(&record, at) {
        return false;
    }
    record.notified_at = at;
    save(home, &record).is_ok()
}

/// The CLI notice, gated on there being a person to show it to.
pub fn cli_notice(home: &Path, current: &str) -> Option<String> {
    if !interactive() {
        return None;
    }
    cli_notice_unchecked(home, current)
}

/// The notice itself, without the terminal check.
///
/// Split out because the terminal check is the one thing an end-to-end test
/// cannot reach — `Stdio::null()` is not a terminal, and the e2e suite has no
/// pty — and the throttle is exactly the part worth testing. Callers that have
/// already established a person is watching may use this directly.
pub fn cli_notice_unchecked(home: &Path, current: &str) -> Option<String> {
    let record = load(home);
    let latest = available(&record, current);
    let shadowed = shadowed_installs(&record);
    if latest.is_none() && shadowed.is_empty() {
        return None;
    }
    if !claim_notice(home) {
        return None;
    }
    Some(render_cli_notice(latest, current, &shadowed))
}

/// Installs that are not the one in use, which is what makes an update look
/// like it did nothing.
fn shadowed_installs(record: &Record) -> Vec<&SeenInstall> {
    if record.installs.len() < 2 {
        return Vec::new();
    }
    record.installs.iter().skip(1).collect()
}

/// The notice body, split out so it is testable without a filesystem.
fn render_cli_notice(latest: Option<&str>, current: &str, shadowed: &[&SeenInstall]) -> String {
    let mut lines = Vec::new();
    if let Some(latest) = latest {
        lines.push(format!(
            "! topos {latest} is available (running {current}) · run `topos update`"
        ));
    }
    if !shadowed.is_empty() {
        lines.push(format!(
            "! {} topos installs found · PATH order decides which runs",
            shadowed.len() + 1
        ));
        for install in shadowed {
            lines.push(format!(
                "!   {}  {}  {}",
                install.path,
                install.version.as_deref().unwrap_or("unknown"),
                install.channel
            ));
        }
    }
    lines.join("\n")
}

/// Latch so one server process injects the banner into one tool result, not
/// all fifty-nine.
static EMITTED: AtomicBool = AtomicBool::new(false);

/// The MCP notice: markdown for the agent that will relay it to the user.
///
/// Reads the cache only — see the module doc for why this never fetches.
pub fn mcp_banner(current: &str) -> Option<String> {
    if !allowed() {
        return None;
    }
    if EMITTED.swap(true, Ordering::SeqCst) {
        return None;
    }
    let Ok(home) = crate::paths::home_dir() else {
        return None;
    };
    let record = load(&home);
    let latest = available(&record, current)?;
    // No claim: the CLI and the server share one daily budget, and whichever
    // gets there first has already told the user.
    Some(render_mcp_banner(latest, current))
}

fn render_mcp_banner(latest: &str, current: &str) -> String {
    format!(
        "⚠️ **Topos {latest} is available** — this server is running {current}. \
         Tell the user once and do not repeat it on every turn; `topos update` \
         shows what it would change before applying anything."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(checked_at: u64, notified_at: u64, latest: &str) -> Record {
        Record {
            checked_at,
            notified_at,
            latest: Some(latest.to_string()),
            current: Some("0.7.0".to_string()),
            installs: Vec::new(),
        }
    }

    #[test]
    fn the_fetch_throttle_allows_one_network_call_a_day() {
        let at = 1_000_000;
        let fresh = record(at, 0, "0.7.0");
        assert!(!stale(&fresh, at));
        assert!(!stale(&fresh, at + THROTTLE_SECS - 1));
        assert!(stale(&fresh, at + THROTTLE_SECS));

        // A record that predates the epoch, or a zeroed one, must not be
        // treated as permanently fresh.
        assert!(stale(&Record::default(), at));
    }

    #[test]
    fn the_display_throttle_is_independent_of_the_fetch_throttle() {
        let at = 1_000_000;
        // Checked long ago but notified just now: quiet, despite being stale.
        let checked_only = record(at - THROTTLE_SECS, at, "0.7.1");
        assert!(stale(&checked_only, at), "the fetch should be allowed");
        assert!(!due(&checked_only, at), "but the notice should stay quiet");
        assert!(!claim_notice_on(&checked_only, at));
    }

    /// `claim_notice` against an in-memory record, so the filesystem stays
    /// out of the throttle rule.
    fn claim_notice_on(record: &Record, at: u64) -> bool {
        due(record, at)
    }

    #[test]
    fn a_notice_names_the_new_version_the_running_one_and_the_command() {
        let notice = render_cli_notice(Some("0.7.1"), "0.7.0", &[]);
        assert_eq!(
            notice,
            "! topos 0.7.1 is available (running 0.7.0) · run `topos update`"
        );
        assert!(
            !notice.contains('❯'),
            "a passive notice must never look like an interactive prompt"
        );
    }

    #[test]
    fn no_new_version_means_no_notice_at_all() {
        assert_eq!(render_cli_notice(None, "0.7.0", &[]), "");
    }

    #[test]
    fn a_shadowed_install_is_reported_with_its_path_version_and_channel() {
        let installs = [
            SeenInstall {
                path: "/opt/homebrew/bin/topos".into(),
                version: Some("0.6.9".into()),
                channel: "homebrew".into(),
            },
            SeenInstall {
                path: "/home/dev/.local/bin/topos".into(),
                version: Some("0.7.0".into()),
                channel: "binary install".into(),
            },
        ];
        let shadowed: Vec<&SeenInstall> = installs.iter().skip(1).collect();
        let notice = render_cli_notice(None, "0.7.0", &shadowed);
        assert!(notice.contains("! 2 topos installs found"), "{notice}");
        assert!(notice.contains("PATH order decides"), "{notice}");
        assert!(
            notice.contains("/home/dev/.local/bin/topos  0.7.0  binary install"),
            "{notice}"
        );
        // The install in use is the one `discover` listed first, so it is the
        // one not reported as shadowed.
        assert!(
            !notice.contains("/opt/homebrew/bin/topos"),
            "the running install is not a stray: {notice}"
        );
    }

    #[test]
    fn a_single_install_is_never_a_layout_problem() {
        let record = Record {
            installs: vec![SeenInstall {
                path: "/home/dev/.local/bin/topos".into(),
                version: Some("0.7.0".into()),
                channel: "binary install".into(),
            }],
            ..Record::default()
        };
        assert!(shadowed_installs(&record).is_empty());
    }

    #[test]
    fn the_mcp_banner_tells_the_agent_to_say_it_once() {
        let banner = render_mcp_banner("0.7.1", "0.7.0");
        assert!(banner.contains("0.7.1"), "{banner}");
        assert!(banner.contains("running 0.7.0"), "{banner}");
        assert!(
            banner.contains("do not repeat it"),
            "an agent told the same thing each turn is worse than silence: {banner}"
        );
    }

    /// The throttle end to end against a real cache file: once, then quiet.
    ///
    /// Driven through [`cli_notice_unchecked`] rather than [`cli_notice`],
    /// because `cargo test` has no terminal and the terminal check is the one
    /// thing an in-process test cannot reach. Everything below it — the record,
    /// the claim, the file write — is the real code.
    #[test]
    fn a_notice_is_claimed_once_per_throttle_window() {
        let home = std::env::temp_dir().join(format!("topos-claim-{}", std::process::id()));
        save(
            &home,
            &Record {
                checked_at: now(),
                notified_at: 0,
                latest: Some("9.9.9".into()),
                current: Some("0.7.0".into()),
                installs: Vec::new(),
            },
        )
        .unwrap();

        let first = cli_notice_unchecked(&home, "0.7.0");
        assert!(first.is_some(), "the first run should notify");
        assert!(first.unwrap().contains("9.9.9"));

        // Immediately after, in a second process-equivalent call.
        assert_eq!(
            cli_notice_unchecked(&home, "0.7.0"),
            None,
            "the notice repeated inside the throttle window"
        );

        // And the claim is persisted, not just in-process.
        assert!(
            load(&home).notified_at > 0,
            "the claim must survive the process"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// Being up to date is not news, and must not consume the daily budget.
    #[test]
    fn an_up_to_date_check_does_not_claim_the_notice() {
        let home = std::env::temp_dir().join(format!("topos-current-{}", std::process::id()));
        save(
            &home,
            &Record {
                checked_at: now(),
                notified_at: 0,
                latest: Some("0.7.0".into()),
                current: Some("0.7.0".into()),
                installs: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(cli_notice_unchecked(&home, "0.7.0"), None);
        assert_eq!(
            load(&home).notified_at,
            0,
            "silence must not burn the once-a-day notice"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_corrupt_cache_reads_as_never_checked() {
        let home = std::env::temp_dir().join(format!("topos-notice-{}", std::process::id()));
        let dir = crate::paths::state_dir(&home);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(RECORD_FILE), "{ this is not json").unwrap();
        assert_eq!(load(&home), Record::default());
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn the_record_round_trips_through_the_cache_file() {
        let home = std::env::temp_dir().join(format!("topos-notice-rt-{}", std::process::id()));
        let mut original = record(100, 200, "0.7.1");
        original.installs.push(SeenInstall {
            path: "/a/topos".into(),
            version: Some("0.7.0".into()),
            channel: "binary install".into(),
        });
        save(&home, &original).unwrap();
        let read = load(&home);
        assert_eq!(read.checked_at, 100);
        assert_eq!(read.notified_at, 200);
        assert_eq!(read.latest.as_deref(), Some("0.7.1"));
        assert_eq!(read.installs.len(), 1);
        assert_eq!(read.installs[0].path, "/a/topos");
        std::fs::remove_dir_all(&home).ok();
    }

    /// Every env-reading assertion in one test: `set_var` is process-global
    /// and `cargo test` is threaded.
    #[test]
    fn the_gate_honours_the_opt_out_and_ci() {
        std::env::set_var(NO_NOTICES_ENV, "1");
        assert!(!allowed(), "{NO_NOTICES_ENV} must close the gate");
        std::env::remove_var(NO_NOTICES_ENV);

        std::env::set_var("CI", "true");
        assert!(!allowed(), "CI must close the gate");
        std::env::set_var("CI", "false");
        assert!(allowed(), "CI=false is not CI");
        std::env::remove_var("CI");
    }
}
