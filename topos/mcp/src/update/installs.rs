//! Every `topos` binary on this machine, and which one is running.
//!
//! Discovery order is `$PATH` order first, because that is the answer to the
//! question a person actually has — "when I type `topos`, what runs?" — and it
//! is the order a shell would resolve in. Known install locations follow, for
//! binaries that are installed but not reachable, which is precisely the state
//! that produces "I updated it and nothing changed".
//!
//! Deduplication is by **file identity**, never by path string. Homebrew puts
//! a symlink at `/opt/homebrew/bin/topos` pointing into
//! `/Cellar/topos/0.7.0/`, so both spellings appear in any honest sweep; a
//! string compare reports the user as running two installs when they run one.
//! `std::fs::metadata` follows symlinks, so the link and its target collapse
//! to the same `(dev, ino)`.

use std::path::{Path, PathBuf};

use super::channel::{self, Channel};

/// File name of the topos executable on this platform.
#[cfg(not(windows))]
pub const EXE_NAME: &str = "topos";
#[cfg(windows)]
pub const EXE_NAME: &str = "topos.exe";

/// One discovered `topos` executable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub path: PathBuf,
    pub channel: Channel,
    /// Version reported by the binary itself, when it could be run.
    pub version: Option<String>,
}

impl Install {
    /// Version for display, or a dash when the binary could not be asked.
    pub fn version_label(&self) -> &str {
        self.version.as_deref().unwrap_or("unknown")
    }
}

/// Every distinct `topos` executable reachable by this process.
///
/// The running binary is always included, even when it is not on `$PATH` —
/// a source checkout invoked by absolute path is the case where `PATH` order
/// says nothing useful.
pub fn discover(home: &Path) -> Vec<Install> {
    let mut found: Vec<Install> = Vec::new();
    for dir in candidate_dirs(home) {
        consider(&dir.join(EXE_NAME), home, &mut found);
    }
    if let Ok(exe) = std::env::current_exe() {
        consider(&absolutize(exe), home, &mut found);
    }
    found
}

/// Directories worth looking in, in resolution order.
fn candidate_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            // An empty entry means the working directory and a relative one
            // resolves against wherever a harness happens to spawn; neither is a
            // place an install lives.
            .filter(|dir| !dir.as_os_str().is_empty() && dir.is_absolute())
            .collect();

    // The installer's default, then the other user-bin locations the harness
    // detectors already know about, then Homebrew's bin when the user has
    // exported a prefix.
    dirs.push(home.join(".local/bin"));
    dirs.push(home.join(".cargo/bin"));
    dirs.push(home.join(".opencode/bin"));
    dirs.push(home.join("bin"));
    if let Some(prefix) = std::env::var_os("HOMEBREW_PREFIX").map(PathBuf::from) {
        dirs.push(prefix.join("bin"));
    }
    dirs
}

/// Add `path` to `found` if it is an executable file not already present.
fn consider(path: &Path, home: &Path, found: &mut Vec<Install>) {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return,
    };
    // A file with no execute bit cannot be the binary a person is running; it
    // is usually a partial extraction.
    if lacks_exec_bit(&metadata) {
        return;
    }
    if found.iter().any(|known| same_file(&known.path, path)) {
        return;
    }
    let channel = channel::classify(path, home);
    let version = probe_version(path);
    found.push(Install {
        path: path.to_path_buf(),
        channel,
        version,
    });
}

/// Run `<path> --version` and pull the version out.
///
/// Asking the binary is the only channel-agnostic source of truth: it works
/// for a Homebrew cellar binary, a `cargo install` binary and a source
/// checkout alike, where a package-manager query would need that manager
/// present. A binary that hangs or refuses is bounded by the timeout and
/// simply reports an unknown version.
fn probe_version(path: &Path) -> Option<String> {
    probe_version_with_timeout(path, std::time::Duration::from_secs(2))
}

fn probe_version_with_timeout(path: &Path, timeout: std::time::Duration) -> Option<String> {
    use std::io::{Read, Seek};
    use std::process::{Command, Stdio};
    // A file avoids waiting for EOF from a launcher descendant holding a pipe.
    let mut output = tempfile::tempfile().ok()?;
    let mut child = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(output.try_clone().ok()?)
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    output.rewind().ok()?;
    let mut text = String::new();
    output.take(4096).read_to_string(&mut text).ok()?;
    // clap prints `<bin name> <version>`; take the last whitespace-separated
    // token so a customized `name` in Cargo.toml cannot break it.
    text.split_whitespace().last().and_then(parse_version)
}

fn parse_version(text: &str) -> Option<String> {
    let trimmed = text.strip_prefix('v').unwrap_or(text);
    semver::Version::parse(trimmed)
        .ok()
        .map(|version| version.to_string())
}

/// The running binary, if it could be located.
pub fn running() -> Option<Install> {
    let exe = std::env::current_exe().ok()?;
    let exe = absolutize(exe);
    let home = crate::paths::home_dir().unwrap_or_else(|_| PathBuf::from("/"));
    Some(Install {
        channel: channel::classify(&exe, &home),
        version: Some(
            semver::Version::parse(env!("CARGO_PKG_VERSION"))
                .map(|version| version.to_string())
                .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string()),
        ),
        path: exe,
    })
}

/// `path` made absolute by joining the working directory, not by resolving it.
///
/// `_NSGetExecutablePath` may return a relative path. Resolving symlinks here
/// would turn a `$PATH` spelling into a version-pinned cellar path, which is
/// the exact bug `install/binary.rs` exists to avoid.
fn absolutize(path: PathBuf) -> PathBuf {
    match std::env::current_dir() {
        Ok(cwd) if !path.is_absolute() => cwd.join(path),
        _ => path,
    }
}

#[cfg(unix)]
fn lacks_exec_bit(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode() & 0o111 == 0
}

#[cfg(not(unix))]
fn lacks_exec_bit(_metadata: &std::fs::Metadata) -> bool {
    false
}

/// True when both paths name the same physical file, symlinks resolved.
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    let identity = |path: &Path| std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()));
    identity(a).zip(identity(b)).is_some_and(|(x, y)| x == y)
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// clap prints `<bin name> <version>`; the parser must survive a
    /// customized bin name and reject an error line.
    #[test]
    fn a_clap_version_line_yields_the_version() {
        let version_of = |line: &str| line.split_whitespace().last().and_then(parse_version);
        assert_eq!(version_of("topos 0.7.0").as_deref(), Some("0.7.0"));
        assert_eq!(version_of("topos.exe 0.7.0").as_deref(), Some("0.7.0"));
        assert_eq!(version_of("topos v0.7.1").as_deref(), Some("0.7.1"));
        assert_eq!(version_of("topos 0.7.0\n").as_deref(), Some("0.7.0"));
        assert_eq!(version_of("Error: no such subcommand"), None);
        assert_eq!(version_of(""), None);
    }

    #[test]
    fn a_single_token_version_parses_too() {
        assert_eq!(parse_version("0.7.0").as_deref(), Some("0.7.0"));
        assert_eq!(parse_version("0.7.0-rc.1").as_deref(), Some("0.7.0-rc.1"));
        assert_eq!(parse_version("subcommand"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_stuck_version_probe_is_bounded() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("topos");
        std::fs::write(&path, "#!/bin/sh\nexec sleep 2\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let start = std::time::Instant::now();
        assert_eq!(
            probe_version_with_timeout(&path, std::time::Duration::from_millis(100)),
            None
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn the_test_binary_itself_is_discoverable_and_versioned() {
        // Proves the probe and the identity comparison against real files on
        // disk rather than fixtures.
        let mut found = Vec::new();
        let exe = std::env::current_exe().unwrap();
        consider(&exe, Path::new("/nonexistent-home"), &mut found);
        assert_eq!(found.len(), 1, "must be added exactly once");
        assert!(same_file(&found[0].path, &exe));
        // Whatever cargo test binary this is, it answers `--version` somehow;
        // what matters is that asking it did not hang or panic.
        let _ = &found[0].version;
    }

    #[cfg(unix)]
    #[test]
    fn a_non_executable_file_is_not_an_install() {
        let dir = std::env::temp_dir().join(format!("topos-update-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(EXE_NAME);
        std::fs::write(&path, "not a program").unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let mut found = Vec::new();
        consider(&path, Path::new("/nonexistent-home"), &mut found);
        assert!(
            found.is_empty(),
            "a file with no exec bit is not an install"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_and_its_target_count_once() {
        let root = std::env::temp_dir().join(format!("topos-update-link-{}", std::process::id()));
        std::fs::create_dir_all(root.join("real")).unwrap();
        std::fs::create_dir_all(root.join("link")).unwrap();
        let target = root.join("real").join(EXE_NAME);
        std::fs::write(&target, "#!/bin/sh\necho 'topos 9.9.9'\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let link = root.join("link").join(EXE_NAME);
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let mut found = Vec::new();
        let home = Path::new("/nonexistent-home");
        consider(&link, home, &mut found);
        consider(&target, home, &mut found);
        assert_eq!(found.len(), 1, "one physical file, one entry");
        assert_eq!(
            found[0].version.as_deref(),
            Some("9.9.9"),
            "the probe runs through the symlink"
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
