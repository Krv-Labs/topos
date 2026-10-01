//! Which distribution channel a `topos` binary came from, and how to upgrade it.
//!
//! Classification is by path shape only. Shelling out to `brew` to ask would
//! be slower and would make a read-only report require the package manager to
//! be installed and working — and the answer is fully determined by where the
//! file sits. `install.sh:152-184` already solved this the same way, and the
//! shapes below are kept in step with it.
//!
//! The consequence worth stating plainly: **Topos does not replace a Homebrew
//! or pip-installed binary with a downloaded tarball.** It runs that channel's
//! own upgrade command instead. Downloading over a Homebrew cellar file
//! produces a binary `brew` knows nothing about, which the next
//! `brew upgrade` then silently reverts.

use std::path::Path;

/// Where a `topos` binary came from, and therefore how it must be upgraded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// `brew install krv-labs/tap/topos`.
    Homebrew,
    /// The `install.sh` binary installer (`~/.local/bin` by default).
    Binary,
    /// `cargo install`.
    Cargo,
    /// A source checkout built with `cargo build --release`.
    Source,
    /// The pre-migration Python distribution (`topos-mcp` wheels).
    Python,
    /// A path matching no known channel.
    Unknown,
}

impl Channel {
    /// Short name for the `Source` row of the update card.
    pub fn label(self) -> &'static str {
        match self {
            Channel::Homebrew => "homebrew",
            Channel::Binary => "binary install",
            Channel::Cargo => "cargo",
            Channel::Source => "source checkout",
            Channel::Python => "python package",
            Channel::Unknown => "unknown",
        }
    }

    /// The command that upgrades this channel, as a person would run it.
    ///
    /// Shown verbatim in the card's `Command` row and in the plan of the
    /// destructive confirm, so it has to be a real command a copy-paste works.
    pub fn upgrade_command(self) -> &'static str {
        match self {
            Channel::Homebrew => "brew upgrade topos",
            Channel::Binary => {
                "TOPOS_UPDATE=1 curl -fsSL https://docs.krv.ai/topos/install.sh | bash"
            }
            Channel::Cargo => "cargo install topos --force",
            Channel::Source => "git pull && cargo build --release -p topos",
            Channel::Python => "uv pip install -U topos-mcp",
            Channel::Unknown => Channel::Binary.upgrade_command(),
        }
    }

    /// How `topos update` performs the upgrade itself.
    ///
    /// `None` means the channel is advised, not acted on: Topos has no way to
    /// upgrade a source checkout (it does not know the working tree) or a pip
    /// install (that is `uv`'s job), so the command is printed for the person
    /// to run.
    pub fn action(self) -> Action {
        match self {
            // Delegated to the installer, which already downloads, verifies the
            // SHA-256 against `checksums.txt`, and moves the new binary into
            // place atomically. Reimplementing that in Rust would duplicate
            // ~60 lines of already-reviewed shell for no gain.
            Channel::Binary => Action::InstallerScript,
            // Homebrew owns its cellar. `brew upgrade` is the only correct
            // move, and it is run on the user's explicit confirmation.
            Channel::Homebrew => Action::Brew,
            Channel::Cargo | Channel::Source | Channel::Python | Channel::Unknown => Action::None,
        }
    }
}

/// How a channel gets upgraded when `topos update` is confirmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Re-run `install.sh` with `TOPOS_UPDATE=1`.
    InstallerScript,
    /// Run `brew upgrade topos`.
    Brew,
    /// Print the command; do not run it.
    None,
}

/// Classify a `topos` executable by where it sits.
///
/// `home` is the user's home directory, taken explicitly so the classification
/// rules are testable without touching the process environment.
pub fn classify(path: &Path, home: &Path) -> Channel {
    let text = path.to_string_lossy().replace('\\', "/");

    if is_homebrew_path(&text) {
        return Channel::Homebrew;
    }
    if under(&text, &home.join(".cargo/bin").to_string_lossy()) {
        return Channel::Cargo;
    }
    // A source checkout builds into `target/release/`, and that path survives
    // a `cargo install --path`. It is the only reliable marker: the binary is
    // otherwise indistinguishable from an installed one.
    if text.contains("/target/release/") || text.contains("/target/debug/") {
        return Channel::Source;
    }
    if is_python_path(&text) {
        return Channel::Python;
    }
    Channel::Binary
}

/// Homebrew's cellar and opt layout, recognized without invoking `brew`.
///
/// Mirrors `install.sh:152-184`, including its deliberately hardcoded prefix
/// list: `HOMEBREW_PREFIX` is only consulted when set, so a machine that never
/// exported it still classifies correctly.
fn is_homebrew_path(text: &str) -> bool {
    if text.contains("/Cellar/") || text.contains("/opt/topos/") {
        return true;
    }
    if let Some(prefix) =
        std::env::var_os("HOMEBREW_PREFIX").map(|p| p.to_string_lossy().into_owned())
    {
        if under(text, &prefix) {
            return true;
        }
    }
    ["/opt/homebrew", "/usr/local", "/home/linuxbrew/.linuxbrew"]
        .iter()
        .any(|prefix| {
            text.starts_with(&format!("{prefix}/Cellar/"))
                || text == format!("{prefix}/bin/topos")
                || text.starts_with(&format!("{prefix}/opt/topos/"))
        })
}

/// The Python-era distribution, which shipped a `topos` console script.
fn is_python_path(text: &str) -> bool {
    [
        "/site-packages/",
        "/dist-packages/",
        "/pipx/",
        "/pypackages/",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

/// True when `text` is `base` itself or lies beneath it.
fn under(text: &str, base: &str) -> bool {
    let base = base.trim_end_matches('/');
    !base.is_empty() && (text == base || text.starts_with(&format!("{base}/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> &'static Path {
        Path::new("/home/dev")
    }

    #[test]
    fn homebrew_is_recognized_from_its_cellar_and_prefixes() {
        let brew = [
            "/opt/homebrew/Cellar/topos/0.7.0/bin/topos",
            "/opt/homebrew/opt/topos/bin/topos",
            "/opt/homebrew/bin/topos",
            "/usr/local/bin/topos",
            "/home/linuxbrew/.linuxbrew/bin/topos",
        ];
        for path in brew {
            assert_eq!(
                classify(Path::new(path), home()),
                Channel::Homebrew,
                "{path} should be Homebrew"
            );
        }
    }

    #[test]
    fn the_default_binary_install_is_not_mistaken_for_any_other_channel() {
        for path in [
            "/home/dev/.local/bin/topos",
            "/usr/local/bin/topos-not-topos",
        ] {
            let channel = classify(Path::new(path), home());
            assert_ne!(channel, Channel::Unknown, "{path}");
        }
        assert_eq!(
            classify(Path::new("/home/dev/.local/bin/topos"), home()),
            Channel::Binary
        );
    }

    #[test]
    fn cargo_source_and_python_installs_are_told_apart() {
        assert_eq!(
            classify(Path::new("/home/dev/.cargo/bin/topos"), home()),
            Channel::Cargo
        );
        assert_eq!(
            classify(Path::new("/repos/topos/target/release/topos"), home()),
            Channel::Source
        );
        assert_eq!(
            classify(
                Path::new("/home/dev/.local/pipx/venvs/topos-mcp/bin/topos"),
                home()
            ),
            Channel::Python
        );
    }

    #[test]
    fn only_brew_and_the_installer_are_acted_on() {
        assert_eq!(Channel::Binary.action(), Action::InstallerScript);
        assert_eq!(Channel::Homebrew.action(), Action::Brew);
        for channel in [
            Channel::Cargo,
            Channel::Source,
            Channel::Python,
            Channel::Unknown,
        ] {
            assert_eq!(
                channel.action(),
                Action::None,
                "{} must be advised, not upgraded in place",
                channel.label()
            );
        }
    }

    #[test]
    fn every_channel_names_a_command_a_person_could_paste() {
        for channel in [
            Channel::Homebrew,
            Channel::Binary,
            Channel::Cargo,
            Channel::Source,
            Channel::Python,
            Channel::Unknown,
        ] {
            let command = channel.upgrade_command();
            assert!(!command.is_empty());
            assert!(
                !command.contains('\n'),
                "{} produced a multi-line command",
                channel.label()
            );
        }
        assert_eq!(
            Channel::Unknown.upgrade_command(),
            Channel::Binary.upgrade_command(),
            "an unrecognized path falls back to the installer rather than a dead end"
        );
    }

    /// `under` must not treat `/usr/local` as containing `/usr/localother`.
    #[test]
    fn prefix_matching_respects_path_boundaries() {
        assert!(under("/usr/local/bin/topos", "/usr/local"));
        assert!(under("/usr/local", "/usr/local"));
        assert!(!under("/usr/localother/bin/topos", "/usr/local"));
        assert!(!under("/usr/local/bin/topos", ""));
    }
}
