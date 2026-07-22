//! Prune third-party and ignored paths when discovering source files.
//!
//! Walks a directory tree looking for source files by language extension,
//! for whole-project evaluation. Common noise — virtualenvs, VCS metadata,
//! caches, build output — is pruned by name; a project's own `.toposignore`
//! and (when the tree sits inside a git repo) `git check-ignore` prune the
//! rest.
//!
//! # Deviation from the Python original
//! - Python's `iter_source_files` is a generator (`Iterator[Path]`); this
//!   returns a `Vec<PathBuf>` instead. Both callers — [`collect_source_files`]
//!   here, and the `include_dirs=True` walk in `topos/mcp/evaluation.py`
//!   (out of scope for this port) — consume the whole thing, so laziness
//!   buys nothing and a plain `Vec` is simpler than a hand-rolled
//!   `Iterator` impl carrying the traversal stack.
//! - Python's `_is_file`/`_is_dir`/`_exists` wrap `Path.is_file()`/etc. in
//!   `try/except OSError: return False` to survive permission errors and
//!   the like. `std::path::Path::is_file`/`is_dir`/`exists` already behave
//!   exactly that way (they return `false` rather than propagating an
//!   error), so no wrapper is needed here.
//! - `.toposignore` pattern matching supports `*` and `?` wildcards, which
//!   covers every case in the Python test suite and every realistic
//!   ignore line (`*.log`, `build/`, ...). POSIX bracket expressions
//!   (`[abc]`, `[!abc]`) are matched as literal characters rather than
//!   character classes.
//!   ponytail: glob-lite matcher, not a full fnmatch/glob port — add
//!   `[...]` class support if a real `.toposignore` ever needs it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::process::run_with_timeout;

/// Directory names skipped during traversal (common venvs, caches, build outputs).
const SKIP_DIR_NAMES: &[&str] = &[
    ".git",
    ".gitnexus",
    ".hg",
    ".svn",
    ".venv",
    "venv",
    "venv.bak",
    "env",
    "__pycache__",
    "__pypackages__",
    "node_modules",
    "dist",
    "build",
    "out",
    "target",
    ".next",
    ".turbo",
    "coverage",
    "htmlcov",
    ".pytest_cache",
    ".mypy_cache",
    ".tox",
    ".ruff_cache",
    ".eggs",
    ".pixi",
];

const TOPOSIGNORE_NAME: &str = ".toposignore";

/// A composable "should this path be skipped?" predicate.
type PathFilter = Box<dyn Fn(&Path) -> bool>;

/// True when `dir_path` looks like a Python virtual environment root.
pub fn is_virtualenv_root(dir_path: &Path) -> bool {
    if dir_path.join("pyvenv.cfg").is_file() {
        return true;
    }
    let bin_dir = dir_path.join("bin");
    if bin_dir.join("python").exists() || bin_dir.join("python3").exists() {
        return true;
    }
    dir_path.join("Scripts").join("python.exe").is_file()
}

/// Whether to avoid descending into `dir_path` during discovery.
pub fn should_skip_dir(dir_path: &Path) -> bool {
    if let Some(name) = dir_path.file_name().and_then(|n| n.to_str()) {
        if SKIP_DIR_NAMES.contains(&name) {
            return true;
        }
    }
    is_virtualenv_root(dir_path)
}

/// Return the repository root containing `.git`, if any.
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let resolved = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    resolved
        .ancestors()
        .find(|candidate| candidate.join(".git").exists())
        .map(Path::to_path_buf)
}

fn load_ignore_patterns(ignore_file: &Path) -> Vec<String> {
    if !ignore_file.is_file() {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(ignore_file) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|raw| {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                None
            } else {
                Some(line.trim_end_matches('/').to_string())
            }
        })
        .collect()
}

/// Minimal shell-glob match (`*`, `?`) against a whole string, mirroring
/// Python's `fnmatch.fnmatch` for the patterns `.toposignore` actually
/// exercises (see the module-level "Deviation" note).
fn glob_match(name: &str, pattern: &str) -> bool {
    fn go(name: &[u8], pat: &[u8]) -> bool {
        match pat.first() {
            None => name.is_empty(),
            Some(b'*') => go(name, &pat[1..]) || (!name.is_empty() && go(&name[1..], pat)),
            Some(b'?') => !name.is_empty() && go(&name[1..], &pat[1..]),
            Some(&c) => name.first() == Some(&c) && go(&name[1..], &pat[1..]),
        }
    }
    go(name.as_bytes(), pattern.as_bytes())
}

fn matches_ignore_pattern(rel_posix: &str, pattern: &str) -> bool {
    if let Some(stripped) = pattern.strip_prefix('/') {
        let p = stripped.trim_start_matches('/');
        return glob_match(rel_posix, p) || rel_posix == p;
    }
    if pattern.contains('/') {
        return glob_match(rel_posix, pattern) || rel_posix.starts_with(&format!("{pattern}/"));
    }
    let name = rel_posix.rsplit('/').next().unwrap_or(rel_posix);
    if glob_match(name, pattern) {
        return true;
    }
    glob_match(rel_posix, pattern) || format!("/{rel_posix}/").contains(&format!("/{pattern}/"))
}

fn relative_posix(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

fn toposignore_checker(root: &Path) -> Option<PathFilter> {
    let patterns = load_ignore_patterns(&root.join(TOPOSIGNORE_NAME));
    if patterns.is_empty() {
        return None;
    }
    let root = root.to_path_buf();
    Some(Box::new(move |path: &Path| {
        let Some(rel_posix) = relative_posix(path, &root) else {
            return false;
        };
        patterns
            .iter()
            .any(|pat| matches_ignore_pattern(&rel_posix, pat))
    }))
}

fn git_available() -> bool {
    let mut cmd = Command::new("git");
    cmd.arg("--version");
    run_with_timeout(cmd, None, true, Some(Duration::from_secs(2)))
        .map(|out| out.status_code == Some(0))
        .unwrap_or(false)
}

fn git_check_ignore_checker(git_root: &Path) -> Option<PathFilter> {
    if !git_available() {
        return None;
    }
    let git_root = git_root.to_path_buf();
    Some(Box::new(move |path: &Path| {
        let Some(rel_posix) = relative_posix(path, &git_root) else {
            return false;
        };
        let mut cmd = Command::new("git");
        cmd.args(["-C"])
            .arg(&git_root)
            .args(["check-ignore", "-q", "--"])
            .arg(&rel_posix);
        run_with_timeout(cmd, None, true, Some(Duration::from_secs(1)))
            .map(|out| out.status_code == Some(0))
            .unwrap_or(false)
    }))
}

/// Compose hard-coded, `.toposignore`, and git-ignore checks for `scan_root`.
pub fn build_path_skip_checker(scan_root: &Path) -> PathFilter {
    let git_root = find_git_root(scan_root);
    let topos = toposignore_checker(scan_root);

    if let Some(git_root) = git_root {
        let git_check = git_check_ignore_checker(&git_root);
        if git_check.is_some() || topos.is_some() {
            return Box::new(move |path: &Path| {
                if git_check.as_ref().is_some_and(|check| check(path)) {
                    return true;
                }
                topos.as_ref().is_some_and(|check| check(path))
            });
        }
    }

    topos.unwrap_or_else(|| Box::new(|_path: &Path| false))
}

fn has_suffix(path: &Path, suffixes: &[&str]) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => suffixes.iter().any(|s| s.trim_start_matches('.') == ext),
        None => false,
    }
}

/// Split one directory's already-sorted children into matching files and
/// subdirectories to recurse into, applying the skip/ignore filters once.
fn scan_dir_children(
    entries: Vec<PathBuf>,
    suffixes: &[&str],
    ignored: &impl Fn(&Path) -> bool,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut subdirs = Vec::new();
    for entry in entries {
        if entry.is_dir() {
            if should_skip_dir(&entry) || ignored(&entry) {
                continue;
            }
            subdirs.push(entry);
        } else if entry.is_file() && has_suffix(&entry, suffixes) && !ignored(&entry) {
            files.push(entry);
        }
    }
    (files, subdirs)
}

/// Collect source files under `root`, pruning venvs and ignored directories.
///
/// With `include_dirs`, also collects each visited (non-skipped) directory,
/// after its own direct file children — callers that need a
/// directory-level signal (e.g. detecting a deletion via a stale
/// parent-directory mtime) get it from the same walk, checked only once
/// the files that would explain it more precisely have already been ruled
/// out.
pub fn iter_source_files(
    root: &Path,
    suffixes: &[&str],
    recursive: bool,
    is_ignored: Option<&PathFilter>,
    include_dirs: bool,
) -> Vec<PathBuf> {
    let ignored = |p: &Path| is_ignored.is_some_and(|f| f(p));
    let mut out = Vec::new();

    if root.is_file() {
        if has_suffix(root, suffixes) && !ignored(root) {
            out.push(root.to_path_buf());
        }
        return out;
    }
    if !root.is_dir() {
        return out;
    }

    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(read_dir) = std::fs::read_dir(&current) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = read_dir.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

        let (files, subdirs) = scan_dir_children(entries, suffixes, &ignored);
        out.extend(files);
        if include_dirs {
            out.push(current);
        }
        if recursive {
            stack.extend(subdirs);
        }
    }
    out
}

/// Collect source files from explicit paths (files or directories).
pub fn collect_source_files<P: AsRef<Path>>(
    paths: &[P],
    suffixes: &[&str],
    recursive: bool,
) -> Vec<PathBuf> {
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();

    for path_arg in paths {
        let path = path_arg.as_ref();
        if path.is_file() {
            if has_suffix(path, suffixes) {
                files.insert(path.to_path_buf());
            }
            continue;
        }
        if !path.is_dir() {
            continue;
        }

        let is_ignored = build_path_skip_checker(path);
        files.extend(iter_source_files(
            path,
            suffixes,
            recursive,
            Some(&is_ignored),
            false,
        ));
    }

    files.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_tmp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos_discovery_test_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn should_skip_dir_recognizes_common_venv_names() {
        assert!(should_skip_dir(Path::new("/proj/.venv")));
        assert!(should_skip_dir(Path::new("/proj/venv")));
        assert!(should_skip_dir(Path::new("/proj/env")));
    }

    #[test]
    fn is_virtualenv_root_false_for_unrelated_dir() {
        assert!(!is_virtualenv_root(Path::new("/fake/does/not/exist")));
    }

    #[test]
    fn collect_source_files_skips_dot_venv() {
        let tmp = unique_tmp_dir("skip_venv");
        std::fs::write(tmp.join("app.py"), "x = 1\n").unwrap();
        let venv = tmp.join(".venv").join("lib");
        std::fs::create_dir_all(&venv).unwrap();
        std::fs::write(venv.join("site.py"), "print('dep')\n").unwrap();

        let files = collect_source_files(&[tmp.as_path()], &[".py"], true);
        assert_eq!(
            files
                .iter()
                .map(|p| p.file_name().unwrap().to_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["app.py"]
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn collect_source_files_skips_venv_with_pyvenv_cfg() {
        let tmp = unique_tmp_dir("skip_pyvenv_cfg");
        std::fs::write(tmp.join("main.py"), "").unwrap();
        let custom = tmp.join("myenv");
        std::fs::create_dir_all(custom.join("lib")).unwrap();
        std::fs::write(custom.join("pyvenv.cfg"), "[venv]\n").unwrap();
        std::fs::write(custom.join("lib").join("dep.py"), "").unwrap();

        let files = collect_source_files(&[tmp.as_path()], &[".py"], true);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap().to_str().unwrap(), "main.py");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn collect_source_files_respects_toposignore() {
        let tmp = unique_tmp_dir("toposignore");
        std::fs::write(tmp.join("keep.py"), "").unwrap();
        let scratch = tmp.join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(scratch.join("skip.py"), "").unwrap();
        std::fs::write(tmp.join(".toposignore"), "scratch/\n").unwrap();

        let files = collect_source_files(&[tmp.as_path()], &[".py"], true);
        assert_eq!(
            files
                .iter()
                .map(|p| p.file_name().unwrap().to_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["keep.py"]
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn collect_source_files_non_recursive() {
        let tmp = unique_tmp_dir("non_recursive");
        let src = tmp.join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.py"), "").unwrap();
        std::fs::write(src.join("sub").join("b.py"), "").unwrap();

        let files = collect_source_files(&[src.as_path()], &[".py"], false);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name().unwrap().to_str().unwrap(), "a.py");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[cfg(unix)]
    #[test]
    fn collect_source_files_skips_unreadable_child_dir() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = unique_tmp_dir("unreadable_child");
        std::fs::write(tmp.join("keep.py"), "").unwrap();
        let blocked = tmp.join("blocked");
        std::fs::create_dir_all(&blocked).unwrap();
        std::fs::write(blocked.join("hidden.py"), "").unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000)).unwrap();

        let files = collect_source_files(&[tmp.as_path()], &[".py"], true);

        // Restore permissions before cleanup, else remove_dir_all fails.
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            files
                .iter()
                .map(|p| p.file_name().unwrap().to_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["keep.py"]
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn glob_match_supports_star_and_question_mark() {
        assert!(glob_match("main.py", "*.py"));
        assert!(!glob_match("main.py", "*.js"));
        assert!(glob_match("a.py", "?.py"));
        assert!(!glob_match("ab.py", "?.py"));
    }
}
