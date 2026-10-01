//! Self-update: finding the published release, working out what is out of
//! date, and applying the upgrade the way each channel expects.
//!
//! Lives in the `topos-mcp` crate because both surfaces need it and the
//! dependency already runs this way — `topos/cli` depends on `topos-mcp`
//! (`topos/cli/Cargo.toml:18`), so the CLI's `topos update` and the MCP
//! server's update notice share one implementation with no new crate wiring.
//! `build_info.rs` set the precedent: self-identity questions belong next to
//! the binary that has the answers.
//!
//! What is deliberately *not* here: a downloader. The binary channel delegates
//! to `install.sh`, which already fetches, verifies the SHA-256 against the
//! release's `checksums.txt`, and moves the new binary into place atomically.
//! Reimplementing that in Rust would duplicate ~60 lines of reviewed shell and
//! add a second thing to get right about atomic replacement and checksums.

pub mod channel;
pub mod checksums;
pub mod installs;
pub mod notice;
pub mod release;

use std::path::Path;
use std::process::Command;

pub use channel::{Action, Channel};
pub use installs::Install;

/// The installer this build was published through. Used to print an exact
/// command rather than a reconstructed one.
pub const INSTALL_SCRIPT_URL: &str = "https://docs.krv.ai/topos/install.sh";

/// What is installed, what is published, and which of the two we are.
#[derive(Clone, Debug)]
pub struct Survey {
    /// The version of the running binary.
    pub current: String,
    /// The newest published version, or `None` when unreachable.
    pub latest: Option<String>,
    /// Every `topos` binary found, in `$PATH` order with the running one first.
    pub installs: Vec<Install>,
}

impl Survey {
    /// True when the published release is strictly newer than what is running.
    ///
    /// `None` when the release server could not be reached, which callers must
    /// not treat as "up to date" — an unreachable server is silence, not a
    /// verdict.
    pub fn update_available(&self) -> Option<bool> {
        let latest = self.latest.as_deref()?;
        Some(release::is_newer(latest, &self.current))
    }

    /// Installs that are behind `latest`, or all of them when `latest` is
    /// unknown — an update to an unknown version is still worth offering.
    pub fn outdated(&self) -> Vec<&Install> {
        match self.latest.as_deref() {
            Some(latest) => self
                .installs
                .iter()
                .filter(|install| {
                    install
                        .version
                        .as_deref()
                        .is_some_and(|version| release::is_newer(latest, version))
                })
                .collect(),
            None => self.installs.iter().collect(),
        }
    }

    /// More than one install, which is what makes an update look ineffective:
    /// the person upgrades one binary while a different one still shadows it.
    pub fn shadowed(&self) -> bool {
        self.installs.len() > 1
    }
}

/// Discover the installed binaries and ask what is published.
///
/// Deliberately does not consult the cache or honour the notice gate: this is
/// an explicit request, so it always goes to the network and always reports.
pub fn survey(home: &Path) -> Survey {
    Survey {
        current: env!("CARGO_PKG_VERSION").to_string(),
        latest: release::latest(),
        installs: installs::discover(home),
    }
}

/// Run a channel's upgrade. Only the channels that have a command to run.
///
/// `Action::Download` is the one variant with no work here: the download needs
/// a progress bar, and this crate carries no terminal UI. `topos/cli` owns that
/// and calls into it. Returning an error rather than silently succeeding keeps a
/// missing dispatch visible instead of reporting an update that changed
/// nothing.
pub fn apply(channel: Channel) -> Result<(), String> {
    apply_for(channel.action(), channel)
}

/// [`apply`], for a caller that has already resolved the action — so the CLI
/// can dispatch `Download` to its own downloader and everything else here
/// without asking each channel twice.
pub fn apply_for(action: Action, channel: Channel) -> Result<(), String> {
    let mut command = match action {
        Action::Download => {
            return Err(
                "the binary channel is downloaded in-process by the CLI, not here".to_string(),
            )
        }
        Action::Brew => {
            let mut command = Command::new("brew");
            command.args(["upgrade", "topos"]);
            command
        }
        Action::None => {
            return Err(format!(
                "topos cannot upgrade a {} install by itself — run this instead:\n  {}",
                channel.label(),
                channel.upgrade_command()
            ))
        }
    };
    let status = command
        .status()
        .map_err(|e| format!("cannot run `{}`: {e}", channel.upgrade_command()))?;
    if !status.success() {
        return Err(format!(
            "`{}` failed{} — nothing was changed",
            channel.upgrade_command(),
            match status.code() {
                Some(code) => format!(" with exit code {code}"),
                None => String::new(),
            }
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn install(path: &str, version: Option<&str>, channel: Channel) -> Install {
        Install {
            path: PathBuf::from(path),
            channel,
            version: version.map(str::to_string),
        }
    }

    fn survey(current: &str, latest: Option<&str>, installs: Vec<Install>) -> Survey {
        Survey {
            current: current.to_string(),
            latest: latest.map(str::to_string),
            installs,
        }
    }

    #[test]
    fn update_available_is_none_when_the_release_server_is_unreachable() {
        let survey = survey("0.7.0", None, vec![]);
        assert_eq!(
            survey.update_available(),
            None,
            "an unreachable server is silence, not 'up to date'"
        );
    }

    #[test]
    fn only_strictly_newer_releases_count_as_updates() {
        assert_eq!(
            survey("0.7.0", Some("0.7.1"), vec![]).update_available(),
            Some(true)
        );
        assert_eq!(
            survey("0.7.0", Some("0.7.0"), vec![]).update_available(),
            Some(false)
        );
        assert_eq!(
            survey("0.7.1", Some("0.7.0"), vec![]).update_available(),
            Some(false),
            "a build ahead of the release line is not offered a downgrade"
        );
    }

    #[test]
    fn outdated_ranks_per_install_rather_than_against_the_running_one() {
        let survey = survey(
            "0.7.0",
            Some("0.7.1"),
            vec![
                install("/usr/local/bin/topos", Some("0.7.1"), Channel::Binary),
                install("/opt/homebrew/bin/topos", Some("0.6.9"), Channel::Homebrew),
            ],
        );
        let outdated = survey.outdated();
        assert_eq!(outdated.len(), 1, "only the 0.6.9 brew install is behind");
        assert_eq!(outdated[0].channel, Channel::Homebrew);
        assert!(survey.shadowed());
    }

    #[test]
    fn an_unknown_latest_offers_every_install_rather_than_nothing() {
        let survey = survey(
            "0.7.0",
            None,
            vec![install("/a/topos", Some("0.7.0"), Channel::Binary)],
        );
        assert_eq!(survey.outdated().len(), 1);
    }

    #[test]
    fn a_channel_with_no_action_refuses_rather_than_guessing() {
        let error = apply(Channel::Source).unwrap_err();
        assert!(error.contains("source checkout"), "{error}");
        assert!(
            error.contains("git pull"),
            "must name the real command: {error}"
        );
    }
}
