//! Download the release binary, verify it, and replace it in place.
//!
//! The first cut of `topos update` delegated this to `install.sh`, on the
//! grounds that the installer already did checksum-and-atomic-replace
//! correctly. That was sound about correctness and wrong about experience:
//! the installer runs `curl --fail -sSL`, and `-s` discards curl's progress
//! meter with no `--progress-bar` to replace it. The only feedback was an
//! indeterminate braille spinner — no bytes, no percentage, no ETA — and three
//! phases produced *no output at all*: the version redirect, the checksums
//! fetch, and the SHA-256 of a multi-megabyte binary. Delegating inherited
//! that, so the download lives here now, on the same `indicatif` primitives
//! the rest of the CLI already draws with. `install.sh` is unchanged for
//! `curl | bash` users.
//!
//! Three orderings are load-bearing:
//!
//! * **Checksums before the binary.** The installer fetches the multi-megabyte
//!   asset first and the file that validates it afterwards, so a corrupted
//!   transfer is only discovered after the whole wait.
//! * **Verify, then rename.** Never write in place: the target may be the
//!   running executable, and a partial write there truncates a live process.
//! * **A bar with a real total.** The length comes from a `HEAD` first, so the
//!   bar can show a percentage and an ETA rather than spinning.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use console::Style;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use topos_mcp::update::checksums;

use crate::commands::render::{paint, RenderOptions};

/// Repository the releases live under. Matches `install.sh`'s `REPO`.
const REPO: &str = "Krv-Labs/topos";

/// Longest any single network step may take. A hung release server must not
/// look like a hung installer.
const NETWORK_TIMEOUT_SECS: u64 = 120;

/// How the transfer is presented.
///
/// One struct rather than two arguments threaded through every function,
/// because "is there a terminal to draw on" is decided once and every phase
/// has to agree with it — a bar that appears on one phase and not the next is
/// worse than no bar.
#[derive(Clone, Copy)]
pub struct Chrome {
    /// Colours and terminal width.
    pub opts: RenderOptions,
    /// False for a redirected run, where `\r` redraws become one line per
    /// frame in a log file.
    pub draw: bool,
}

impl Chrome {
    /// A terminal the user is watching.
    ///
    /// `stderr_is_terminal` is passed rather than derived from `opts.styled`,
    /// because `styled` also goes false under `NO_COLOR` and a user who has
    /// turned colour off still wants to see bytes arriving. The check every
    /// other interactive surface in the CLI uses is stderr being a TTY, since
    /// that is the stream the bar is drawn on.
    pub fn interactive(opts: RenderOptions, stderr_is_terminal: bool) -> Self {
        Self {
            opts,
            draw: stderr_is_terminal,
        }
    }
}

/// What an upgrade actually did, so the caller can say something true about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub path: PathBuf,
    pub version: String,
}

/// Replace the binary at `target` with `version` from the published release.
pub fn install(
    target: &Path,
    version: &str,
    platform: &str,
    display: Chrome,
) -> Result<Installed, String> {
    let asset = format!("topos-{platform}");
    let base = format!("https://github.com/{REPO}/releases/download/v{version}");

    let expected = expected_checksum(version, &asset, display)?;

    let dir = target
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", target.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let staging = staging_file(dir)?;

    let result = download(
        &format!("{base}/{asset}"),
        &staging,
        &expected,
        asset.as_str(),
        display,
    )
    .and_then(|()| {
        replace(&staging, target)?;
        Ok(Installed {
            path: target.to_path_buf(),
            version: version.to_string(),
        })
    });

    // The unique staging path is removed on drop, including on failure.
    result
}

fn staging_file(dir: &Path) -> Result<tempfile::TempPath, String> {
    tempfile::Builder::new()
        .prefix(".topos-download-")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map(|file| file.into_temp_path())
        .map_err(|e| format!("cannot stage download in {}: {e}", dir.display()))
}

/// Fetch `checksums.txt` and return the hash for `asset`.
pub fn expected_checksum(version: &str, asset: &str, display: Chrome) -> Result<String, String> {
    let url = format!("https://github.com/{REPO}/releases/download/v{version}/checksums.txt");
    let body = phase(display, "Checking release", |l| {
        let _ = l;
        fetch(&url)
    })?;
    checksums::expected(&body, asset).ok_or_else(|| {
        format!("the {version} release has no checksums.txt entry for {asset} — refusing to install an unverified binary")
    })
}

/// Stream `url` into `path` behind a progress bar, then verify the hash.
fn download(
    url: &str,
    path: &Path,
    expected: &str,
    asset: &str,
    display: Chrome,
) -> Result<(), String> {
    // A `HEAD` costs one round trip and buys a real percentage and ETA.
    let total = content_length(url);

    let file =
        std::fs::File::create(path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    let mut child = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            &NETWORK_TIMEOUT_SECS.to_string(),
        ])
        .arg(url)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run curl: {e}"))?;

    let mut reader = child.stdout.take().ok_or("curl produced no output")?;

    let bar = progress_bar(total, display);
    let mut buffer = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    let read_result = loop {
        match reader.read(&mut buffer) {
            Ok(0) => break Ok(()),
            Ok(count) => {
                if let Err(e) = writer.write_all(&buffer[..count]) {
                    break Err(format!("cannot write {}: {e}", path.display()));
                }
                received += count as u64;
                bar.set_position(received);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => break Err(format!("download interrupted: {e}")),
        }
    };
    let transfer_result = read_result.and_then(|()| {
        writer
            .flush()
            .map_err(|e| format!("cannot write {}: {e}", path.display()))
    });
    bar.finish_and_clear();
    if let Err(error) = transfer_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    // Drain before waiting: a curl blocked on a full pipe would deadlock here.
    let status = child
        .wait()
        .map_err(|e| format!("cannot wait for curl: {e}"))?;
    if !status.success() {
        return Err(describe_failure(url, status));
    }

    // Named, because "Verifying" with no subject gives the user nothing to
    // connect the wait to — and this is the step that has to be trusted.
    let actual = phase(display, &format!("Verifying {asset}"), |_| {
        checksums::sha256_file(path)
    })?;
    if actual != expected {
        return Err(format!(
            "checksum mismatch for {asset}\n  expected  {expected}\n  \
             actual    {actual}\n  The download was corrupted. Retry, or report both hashes."
        ));
    }
    Ok(())
}

/// Total bytes, or `None` when the server will not say — which yields a
/// byte-count-only bar rather than a wrong percentage.
fn content_length(url: &str) -> Option<u64> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--location",
            "--head",
            "--max-time",
            "20",
        ])
        .arg(url)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // `--head` prints the response headers, so the last `content-length` is
    // the real one after a redirect rather than the 302's.
    let header = String::from_utf8_lossy(&output.stdout);
    header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .rfind(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
}

/// curl's exit code is the only useful diagnostic for a 404 or a TLS failure.
/// `install.sh` discarded it entirely (`-s` without `-S`).
fn describe_failure(url: &str, status: std::process::ExitStatus) -> String {
    let code = status.code().map(|c| c.to_string()).unwrap_or_default();
    format!("download failed (curl exit {code}) for {url}")
}

/// Move the verified file over the target, restoring the execute bit.
fn replace(staged: &Path, target: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("cannot make {} executable: {e}", staged.display()))?;
    }
    std::fs::rename(staged, target).map_err(|e| {
        format!(
            "cannot replace {}: {e}\n  Another topos may be running from it.",
            target.display()
        )
    })
}

/// The bar template for a transfer of known or unknown length.
///
/// Split out so the choice is assertable: `{percent}` is only honest when a
/// total is known, and rendering `0%` while 4 MB streams in is worse than
/// showing bytes and a rate.
fn template_for(total: Option<u64>) -> &'static str {
    match total {
        Some(_) => {
            // `{bar}` alone draws no percentage — indicatif only fills a bar's
            // interior from `{wide_bar}` unless asked — so it is named
            // explicitly here.
            "  Downloading {bar:20.cyan/dim} {percent:>3}%  {bytes}/{total_bytes} \
             {binary_bytes_per_sec}  {eta}"
        }
        None => "  Downloading {spinner:.cyan} {bytes} {binary_bytes_per_sec}",
    }
}

fn progress_bar(total: Option<u64>, display: Chrome) -> ProgressBar {
    if !display.draw {
        return ProgressBar::hidden();
    }
    let bar = match total {
        Some(bytes) => ProgressBar::new(bytes),
        None => ProgressBar::new_spinner(),
    };
    // stderr, like every other progress surface in the CLI, so stdout stays
    // clean for `--json`.
    bar.set_draw_target(ProgressDrawTarget::stderr());
    bar.set_style(
        ProgressStyle::with_template(template_for(total))
            .expect("static progress template")
            .progress_chars("█▓░"),
    );
    bar.enable_steady_tick(std::time::Duration::from_millis(100));
    bar
}

/// Print `label`, run `work`, then replace the line with the outcome.
fn phase<T>(
    display: Chrome,
    label: &str,
    work: impl FnOnce(&str) -> Result<T, String>,
) -> Result<T, String> {
    if !display.draw {
        return work(label);
    }
    let opts = display.opts;
    let bar = ProgressBar::new_spinner();
    bar.set_style(
        ProgressStyle::with_template("  {spinner:.cyan} {msg}")
            .expect("static progress template")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    bar.set_message(label.to_string());
    bar.set_draw_target(ProgressDrawTarget::stderr());
    bar.enable_steady_tick(std::time::Duration::from_millis(80));

    let result = work(label);

    bar.finish_and_clear();
    match &result {
        Ok(_) => println!("  {} {label}", paint("✓", Style::new().green(), opts)),
        Err(_) => println!("  {} {label}", paint("✕", Style::new().red(), opts)),
    }
    result
}

/// Run a curl that returns stdout as text, with a deadline.
fn fetch(url: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            &NETWORK_TIMEOUT_SECS.to_string(),
        ])
        .arg(url)
        .output()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if !output.status.success() {
        return Err(describe_failure(url, output.status));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("{url} returned invalid UTF-8: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome() -> Chrome {
        Chrome {
            opts: RenderOptions {
                styled: false,
                width: 80,
            },
            draw: false,
        }
    }

    #[test]
    fn concurrent_staging_files_do_not_share_bytes_or_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let first = staging_file(dir.path()).unwrap();
        let second = staging_file(dir.path()).unwrap();
        assert_ne!(first.to_path_buf(), second.to_path_buf());
        std::fs::write(&first, b"verified").unwrap();
        std::fs::write(&second, b"other").unwrap();
        drop(second);
        assert_eq!(std::fs::read(&first).unwrap(), b"verified");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        drop(first);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    /// `draw: false` must return the worker's value untouched, so the whole
    /// pipeline is exercisable without a pty — which is the only way a
    /// redirected run can be asserted on at all.
    #[test]
    fn a_redirected_run_reports_a_phase_outcome_without_drawing() {
        let result = phase(chrome(), "Checking release", |_| Ok::<_, String>(7));
        assert_eq!(result, Ok(7));
    }

    #[test]
    fn a_phase_error_is_propagated_rather_than_swallowed() {
        let result = phase(chrome(), "Checking release", |_| {
            Err::<(), _>("checksum mismatch".to_string())
        });
        assert_eq!(result, Err("checksum mismatch".to_string()));
    }

    #[test]
    fn a_bar_without_a_total_shows_no_percentage() {
        let known = template_for(Some(1024));
        // Matched without the closing brace: the template may carry a format
        // spec, as `{percent:>3}` does.
        assert!(known.contains("{percent"), "{known}");
        assert!(known.contains("{total_bytes"), "{known}");
        assert!(known.contains("{eta"), "{known}");

        let unknown = template_for(None);
        assert!(
            !unknown.contains("{percent") && !unknown.contains("{total_bytes"),
            "an unknown total must not render a percentage it cannot know: {unknown}"
        );
        // Still a bar of progress, just not a quantified one.
        assert!(unknown.contains("{bytes}"), "{unknown}");
        assert!(unknown.contains("{binary_bytes_per_sec}"), "{unknown}");
    }

    /// Both templates must be valid to indicatif, or a download panics on the
    /// very path that is supposed to be helping.
    #[test]
    fn every_template_is_one_indicatif_accepts() {
        for total in [Some(1u64), None] {
            let template = template_for(total);
            ProgressStyle::with_template(template)
                .unwrap_or_else(|e| panic!("{template:?} rejected: {e}"));
        }
    }

    #[test]
    fn a_curl_failure_names_the_url_it_failed_on() {
        let status = std::process::Command::new("sh")
            .args(["-c", "exit 22"])
            .status()
            .unwrap();
        let message = describe_failure("https://example.invalid/topos-linux-amd64", status);
        assert!(message.contains("22"), "{message}");
        assert!(
            message.contains("topos-linux-amd64"),
            "the asset that failed has to be identifiable: {message}"
        );
    }
}
