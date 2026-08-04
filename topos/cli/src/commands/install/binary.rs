//! Which absolute path to record as an MCP server's `command`.
//!
//! Every harness config topos writes names the topos executable by absolute
//! path. A bare `"topos"` is not an option: GUI-launched macOS apps inherit
//! only `/usr/bin:/bin:/usr/sbin:/sbin`, so Claude Desktop, Cursor and
//! Antigravity cannot spawn it on any install channel.
//!
//! Picking *which* absolute path is the whole problem. `current_exe()` reports
//! wherever the process happened to be launched from, and for a Homebrew
//! install that is the version-pinned Cellar path — recording it means the next
//! `brew upgrade` silently breaks every harness. So this module prefers the
//! `$PATH` spelling of the same physical file, which is the stable one, and
//! never canonicalizes a path it hands back.
//!
//! Two rules follow from that and are load-bearing throughout:
//!
//! * Comparison is by **file identity**, never by string. One physical file has
//!   several legitimate spellings, and a string compare would rewrite every
//!   config on every run.
//! * Identity is read with [`fs::metadata`], which follows symlinks, never
//!   `fs::symlink_metadata`, which would report a `$PATH` symlink's own inode
//!   and therefore never match its target.

use std::env;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};

/// File name of the topos executable on this platform.
#[cfg(not(windows))]
pub(crate) const EXE_NAME: &str = "topos";
#[cfg(windows)]
pub(crate) const EXE_NAME: &str = "topos.exe";

/// The absolute path to record as an MCP server's `command`.
///
/// GUI-launched macOS apps inherit only `/usr/bin:/bin:/usr/sbin:/sbin`, so a
/// bare `"topos"` cannot spawn from Claude Desktop, Cursor or Antigravity on
/// any install channel. An absolute path is the only portable answer.
pub(crate) fn resolve_binary_path() -> Result<PathBuf, String> {
    let exe = env::current_exe()
        .map_err(|e| format!("cannot determine the path of the running topos binary: {e}"))?;
    // Never fall back to a bare `"topos"`: a config entry that cannot spawn is
    // worse than a loud error, because it fails later and somewhere else.
    let exe = absolutize(exe);
    Ok(preferred_path_alias(&exe).unwrap_or(exe))
}

/// `path` made absolute, by joining the working directory rather than by
/// resolving it.
///
/// `_NSGetExecutablePath` is permitted to return a relative path, and a
/// relative `command` would resolve against whatever directory the harness
/// happens to spawn in. Joining keeps every symlink in the spelling intact;
/// canonicalizing here would reintroduce the Cellar-pinning bug.
fn absolutize(path: PathBuf) -> PathBuf {
    // A missing working directory leaves nothing better to say than the
    // original path; `drift` will flag it as non-absolute on the next run.
    match env::current_dir() {
        Ok(cwd) if !path.is_absolute() => cwd.join(path),
        _ => path,
    }
}

/// The `$PATH` entry naming the same physical file as `exe`, if there is one.
///
/// Prefers a `$PATH` alias over `current_exe()` because that alias is the
/// stable, upgrade-surviving spelling: on macOS `current_exe()` returns
/// whatever path the process was launched from, which for a Homebrew install
/// spawned through a recorded entry is the version-pinned Cellar path.
///
/// Runs on every platform. Gating it to Linux would leave the Homebrew pinning
/// bug reachable on exactly the machines that hit it.
fn preferred_path_alias(exe: &Path) -> Option<PathBuf> {
    // Linux reports a deleted executable as `/proc/self/exe (deleted)`; with no
    // readable identity to match against, there is no alias to prefer.
    fs::metadata(exe).ok()?;
    path_dirs()
        .into_iter()
        .filter_map(|dir| file_named_exe(&dir))
        // First match wins, deliberately: it is what a bare `topos` would
        // resolve to, so it is the spelling the user already means.
        .find(|candidate| same_file(candidate, exe))
}

/// The `$PATH` entries worth searching, in order.
///
/// An empty entry means "the working directory" to POSIX, which would yield a
/// relative recorded path; a non-absolute entry has the same defect. Both are
/// dropped rather than resolved.
fn path_dirs() -> Vec<PathBuf> {
    // Bound to a local because `split_paths` borrows the whole `$PATH` value.
    let value = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&value)
        .filter(|dir| !dir.as_os_str().is_empty())
        .filter(|dir| dir.is_absolute())
        .collect()
}

/// `dir/topos`, but only when it is a regular file.
///
/// The `is_file()` gate is what skips a *directory* named `topos` and a
/// *broken symlink* named `topos`, neither of which a shell would ever execute.
fn file_named_exe(dir: &Path) -> Option<PathBuf> {
    let candidate = dir.join(EXE_NAME);
    fs::metadata(&candidate)
        .is_ok_and(|meta| meta.is_file())
        .then_some(candidate)
}

/// Why a recorded `command` no longer refers to `binary`, or `None` when it
/// still does.
///
/// Identity-based, never a string compare: [`preferred_path_alias`] introduces
/// spelling variance for one physical file, so a string compare would rewrite
/// every config on every run.
pub(crate) fn drift(recorded: &str, binary: &Path) -> Option<String> {
    let path = Path::new(recorded);
    shape_drift(recorded, path).or_else(|| identity_drift(recorded, path, binary))
}

/// Drift the recorded path shows on its own, before any comparison: it cannot
/// spawn at all.
///
/// One rule per function, ordered most-fundamental first, so the reported
/// reason names the actual defect rather than a downstream symptom. The earlier
/// draft's bare `"topos"` fails the first rule, which is how an install written
/// by that draft self-heals on the next run.
fn shape_drift(recorded: &str, path: &Path) -> Option<String> {
    not_absolute(recorded, path)
        .or_else(|| does_not_exist(recorded, path))
        .or_else(|| not_a_file(recorded, path))
        .or_else(|| not_executable(recorded, path))
}

/// A relative `command` resolves against whatever directory the harness spawns
/// in, which is never the one the user had in mind.
fn not_absolute(recorded: &str, path: &Path) -> Option<String> {
    (!path.is_absolute()).then(|| format!("`{recorded}` is not an absolute path"))
}

/// Typically an uninstalled channel: the config outlived the executable.
fn does_not_exist(recorded: &str, path: &Path) -> Option<String> {
    fs::metadata(path)
        .is_err()
        .then(|| format!("`{recorded}` no longer exists"))
}

/// A directory, or a device node — something that exists but cannot be spawned.
fn not_a_file(recorded: &str, path: &Path) -> Option<String> {
    file_meta(path)
        .is_some_and(|meta| !meta.is_file())
        .then(|| format!("`{recorded}` is not a file"))
}

/// A file whose execute bit was lost, usually to an archive extraction.
fn not_executable(recorded: &str, path: &Path) -> Option<String> {
    file_meta(path)
        .is_some_and(|meta| lacks_exec_bit(&meta))
        .then(|| format!("`{recorded}` is not executable"))
}

/// Metadata for a rule that has already established the path is readable.
///
/// Re-reading it per rule keeps each rule independent and total; the cost is
/// one `stat` per harness, once per command.
fn file_meta(path: &Path) -> Option<Metadata> {
    fs::metadata(path).ok()
}

/// Drift only visible by comparison: the recorded path spawns something, but
/// not this topos.
///
/// Typically a second install channel — a Homebrew entry left behind after a
/// `cargo install`, or the reverse.
fn identity_drift(recorded: &str, path: &Path, binary: &Path) -> Option<String> {
    (!same_file(path, binary)).then(|| {
        format!(
            "`{recorded}` is a different binary than the running topos at {}",
            binary.display()
        )
    })
}

/// True when the file carries no execute bit for anybody.
///
/// Unix only; elsewhere executability is not a mode bit and a file that exists
/// is assumed spawnable.
#[cfg(unix)]
fn lacks_exec_bit(meta: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    meta.mode() & 0o111 == 0
}

#[cfg(not(unix))]
fn lacks_exec_bit(_meta: &Metadata) -> bool {
    false
}

/// True when both paths name the same physical file.
///
/// Unix compares `(dev, ino)`; Windows compares `fs::canonicalize` results
/// (`file_index` is nightly-only, rust#63010). Reads both sides with
/// `fs::metadata`, so a symlink and its target compare equal — that is the
/// point.
///
/// Total by construction: callers pass whatever string a user's config holds,
/// including `"uvx"` and `""`, so an unreadable side is simply "not the same
/// file" rather than an error.
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    file_identity(a)
        .zip(file_identity(b))
        .is_some_and(|(left, right)| left == right)
}

/// A value that is equal for two paths exactly when they name one physical
/// file, or `None` when the path names nothing readable.
///
/// `fs::metadata` follows symlinks on purpose. `symlink_metadata` on a Homebrew
/// `$PATH` entry yields the link's own inode rather than the executable's, so
/// the alias search would never find a match and every Homebrew install would
/// record its Cellar path.
#[cfg(unix)]
fn file_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    fs::metadata(path).ok().map(|meta| (meta.dev(), meta.ino()))
}

/// The Windows counterpart. `canonicalize` is used **only** here, for
/// comparison; its output is never recorded, because a resolved path defeats
/// the upgrade-survival this module exists to protect.
#[cfg(not(unix))]
fn file_identity(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("topos-binary-{label}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A real, executable stand-in for the topos binary, in its own directory
    /// so it can be placed on a synthetic `$PATH`.
    fn executable_in(parent: &Path, dir_name: &str) -> PathBuf {
        let dir = parent.join(dir_name);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(EXE_NAME);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        set_mode(&path, 0o755);
        path
    }

    fn set_mode(path: &Path, mode: u32) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = (path, mode);
    }

    #[cfg(unix)]
    fn link_dir(parent: &Path, dir_name: &str, target: &Path) -> PathBuf {
        let dir = parent.join(dir_name);
        fs::create_dir_all(&dir).unwrap();
        let link = dir.join(EXE_NAME);
        std::os::unix::fs::symlink(target, &link).unwrap();
        link
    }

    /// Every `$PATH`-mutating assertion lives in this one test on purpose:
    /// `cargo test` is threaded and `set_var` is process-global, so splitting
    /// them would make the suite race against itself.
    #[cfg(unix)]
    #[test]
    fn path_alias_search_matches_what_a_shell_would_run() {
        let root = scratch("alias");
        let real = executable_in(&root, "real");
        let link = link_dir(&root, "lnkbin", &real);
        let broken = link_dir(&root, "brokenbin", &root.join("nowhere"));
        let dir_named_exe = root.join("dirbin").join(EXE_NAME);
        fs::create_dir_all(&dir_named_exe).unwrap();
        let bare = root.join("barebin");
        fs::create_dir_all(&bare).unwrap();

        let saved_path = env::var_os("PATH");
        let saved_cwd = env::current_dir().ok();

        // A symlink on `$PATH` is the same file as its target. This is the
        // assertion that fails under `symlink_metadata`.
        set_path(&[link.parent().unwrap()]);
        assert_eq!(preferred_path_alias(&real), Some(link.clone()));

        // A broken symlink named `topos` is skipped, not preferred.
        set_path(&[broken.parent().unwrap(), link.parent().unwrap()]);
        assert_eq!(preferred_path_alias(&real), Some(link.clone()));

        // A directory named `topos` is skipped.
        set_path(&[&root.join("dirbin"), link.parent().unwrap()]);
        assert_eq!(preferred_path_alias(&real), Some(link.clone()));

        // First match wins — it is what a bare `topos` resolves to.
        set_path(&[link.parent().unwrap(), real.parent().unwrap()]);
        assert_eq!(preferred_path_alias(&real), Some(link.clone()));

        // An empty `$PATH` entry means "cwd" to POSIX. Even standing in a
        // directory that holds the executable, it must never produce a
        // relative recorded path.
        env::set_current_dir(real.parent().unwrap()).unwrap();
        env::set_var("PATH", "/nonexistent-a::/nonexistent-b");
        let from_empty_entry = preferred_path_alias(&real);
        assert!(
            from_empty_entry.is_none(),
            "empty $PATH entry yielded {from_empty_entry:?}"
        );
        if let Some(cwd) = saved_cwd {
            env::set_current_dir(cwd).unwrap();
        }

        // Not on `$PATH`: the caller keeps its input verbatim rather than a
        // resolved one. `link` is a symlink, so canonicalizing would visibly
        // change it — which is exactly what must not happen.
        set_path(&[&bare]);
        assert_eq!(preferred_path_alias(&link), None);
        assert_ne!(link, fs::canonicalize(&link).unwrap(), "vacuous assertion");
        let recorded = preferred_path_alias(&link).unwrap_or_else(|| link.clone());
        assert_eq!(recorded, link);

        restore_path(saved_path);
        fs::remove_dir_all(root).ok();
    }

    fn set_path(dirs: &[&Path]) {
        env::set_var("PATH", env::join_paths(dirs).unwrap());
    }

    fn restore_path(saved: Option<std::ffi::OsString>) {
        match saved {
            Some(value) => env::set_var("PATH", value),
            None => env::remove_var("PATH"),
        }
    }

    /// The reason `drift` compares identity rather than strings: one physical
    /// file legitimately has several spellings, and a string compare would
    /// rewrite every config on every run.
    #[cfg(unix)]
    #[test]
    fn two_spellings_of_one_file_do_not_drift() {
        let root = scratch("spellings");
        let real = executable_in(&root, "bin");
        let link = link_dir(&root, "lnkbin", &real);

        assert_ne!(real, link);
        assert_eq!(drift(&link.display().to_string(), &real), None);
        assert_eq!(drift(&real.display().to_string(), &link), None);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_command_that_cannot_spawn_this_topos_drifts_with_a_reason() {
        let root = scratch("drift");
        let real = executable_in(&root, "bin");
        let other = executable_in(&root, "other");

        // The earlier draft's bare `topos`: relative, so it self-heals on the
        // next install.
        let relative = drift("topos", &real).unwrap();
        assert!(relative.contains("not an absolute path"), "{relative}");

        let gone = root.join("bin").join("removed");
        let missing = drift(&gone.display().to_string(), &real).unwrap();
        assert!(missing.contains("no longer exists"), "{missing}");

        let as_dir = drift(&root.join("bin").display().to_string(), &real).unwrap();
        assert!(as_dir.contains("is not a file"), "{as_dir}");

        let different = drift(&other.display().to_string(), &real).unwrap();
        assert!(different.contains("different binary"), "{different}");
        assert!(
            different.contains(&real.display().to_string()),
            "{different}"
        );

        fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_file_without_an_execute_bit_drifts() {
        let root = scratch("mode");
        let real = executable_in(&root, "bin");
        set_mode(&real, 0o644);

        let reason = drift(&real.display().to_string(), &real).unwrap();
        assert!(reason.contains("is not executable"), "{reason}");
        fs::remove_dir_all(root).ok();
    }

    /// `same_file` is handed raw config strings by `artifact.rs`, so an
    /// unreadable side must answer "no", never panic.
    #[test]
    fn same_file_is_total_over_unreadable_paths() {
        let root = scratch("total");
        let real = executable_in(&root, "bin");

        assert!(same_file(&real, &real));
        assert!(!same_file(Path::new("uvx"), &real));
        assert!(!same_file(Path::new(""), &real));
        assert!(!same_file(&real, &root.join("bin").join("absent")));
        fs::remove_dir_all(root).ok();
    }
}
