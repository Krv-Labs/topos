//! What the newest published release is, and whether it is newer than us.
//!
//! The version is read from a redirect rather than the GitHub API: a
//! `HEAD`-equivalent `curl` against `/releases/latest` reports the final URL,
//! whose last segment is the tag. That needs no token, no API version header,
//! and no rate-limit budget, which matters because an update check is
//! unprompted network traffic from a tool people run on every keystroke. It is
//! the same mechanism `install.sh:483-500` uses, kept in step deliberately so
//! the two can never disagree about what "latest" means.
//!
//! Only the tag comes back — no changelog, no asset list, no publish date.
//! That is enough to answer "is there something newer", which is the only
//! question asked.

use std::process::Command;

use semver::Version;

const REPO: &str = "Krv-Labs/topos";

/// Longest a version lookup may take before it is abandoned.
///
/// Deliberately short. This runs on a human's command line, sometimes on a
/// flaky hotel network, and an update notice that delays a command by ten
/// seconds is worse than no notice at all.
const LOOKUP_TIMEOUT_SECS: u64 = 5;

/// The newest published tag, without a leading `v`, or `None` if it cannot be
/// determined.
///
/// `None` is the normal outcome on a machine with no network, no `curl`, or
/// a proxy that intercepts the request. Every caller treats it as "say
/// nothing" — an unreachable release server is not an error worth surfacing.
pub fn latest() -> Option<String> {
    let url = format!("https://github.com/{REPO}/releases/latest");
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            &LOOKUP_TIMEOUT_SECS.to_string(),
            "--output",
            "/dev/null",
            "--write-out",
            "%{url_effective}",
            &url,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    tag_from_effective_url(&String::from_utf8_lossy(&output.stdout))
}

/// Extract the version from a `/releases/tag/<tag>` redirect target.
///
/// Returns `None` unless the URL really is a release redirect: an intercepting
/// proxy can answer with an HTML login page at the same path, and treating
/// `<title>Sign in</title>` as a version would be worse than reporting nothing.
fn tag_from_effective_url(url: &str) -> Option<String> {
    let path = url
        .split_once("/releases/tag/")?
        .1
        .split(['?', '#'])
        .next()?
        .trim_end_matches('/');
    parse(path).map(|version| version.to_string())
}

/// Parse a tag into a comparable version, tolerating the `v` prefix.
fn parse(tag: &str) -> Option<Version> {
    let trimmed = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(trimmed).ok()
}

/// True when `latest` is a strictly newer release than `current`.
///
/// Both sides are parsed, so an unparseable version on either end is
/// "not newer" rather than a panic or a bogus offer. `0.7.1-rc.1` correctly
/// counts as newer than `0.7.0`: a release candidate is the current head of
/// the line, and the semver crate — unlike the hand-rolled comparator in
/// `gitnexus.rs` — gets that ordering right.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse(latest), parse(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// The release channel identifier for this build, used in reports.
pub fn platform() -> String {
    // `OS` is already the release asset's spelling for the two platforms a
    // binary is published for; the match only documents that.
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_redirect_yields_the_tag_without_its_v() {
        assert_eq!(
            tag_from_effective_url("https://github.com/Krv-Labs/topos/releases/tag/v0.7.1")
                .as_deref(),
            Some("0.7.1")
        );
        // Query strings and fragments must not become part of the version.
        assert_eq!(
            tag_from_effective_url("https://github.com/Krv-Labs/topos/releases/tag/v0.7.1?x=1")
                .as_deref(),
            Some("0.7.1")
        );
        assert_eq!(
            tag_from_effective_url("https://github.com/Krv-Labs/topos/releases/tag/0.7.1/")
                .as_deref(),
            Some("0.7.1")
        );
    }

    /// A captive portal or a proxy login page must not be mistaken for a tag.
    #[test]
    fn anything_that_is_not_a_release_redirect_yields_nothing() {
        for url in [
            "https://github.com/Krv-Labs/topos",
            "https://github.com/login?return_to=%2Freleases%2Ftag%2Fv9.9.9",
            "https://github.com/Krv-Labs/topos/releases",
            "",
        ] {
            assert_eq!(tag_from_effective_url(url), None, "{url}");
        }
    }

    #[test]
    fn precedence_matches_semver_including_prereleases() {
        assert!(is_newer("0.7.1", "0.7.0"));
        assert!(is_newer("0.8.0", "0.7.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(
            is_newer("0.10.0", "0.9.0"),
            "a two-digit minor is numeric, not lexical"
        );
        assert!(!is_newer("0.9.0", "0.10.0"));
        assert!(
            is_newer("v0.7.1", "0.7.0"),
            "tags carry a v; versions do not"
        );
        // The case the hand-rolled gitnexus comparator gets wrong.
        assert!(is_newer("0.7.1-rc.1", "0.7.0"));

        assert!(!is_newer("0.7.0", "0.7.0"), "same version is not newer");
        assert!(!is_newer("0.6.9", "0.7.0"), "an older release is not newer");
        assert!(
            !is_newer("0.7.0", "0.7.1-rc.1"),
            "a release is not newer than its own candidate"
        );
    }

    #[test]
    fn unparseable_versions_are_never_offered_as_an_update() {
        assert!(!is_newer("not-a-version", "0.7.0"));
        assert!(!is_newer("0.7.1", "nightly"));
        assert!(!is_newer("", ""));
    }

    #[test]
    fn the_platform_label_names_a_real_release_asset() {
        let platform = platform();
        assert!(
            ["linux-amd64", "linux-arm64", "macos-arm64"].contains(&platform.as_str()),
            "release.yml only builds these three; got {platform}"
        );
    }
}
