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

/// Directory names skipped during traversal unconditionally (common venvs, caches, package stores).
const ALWAYS_SKIP_DIR_NAMES: &[&str] = &[
    ".git",
    ".gitnexus",
    ".hg",
    ".svn",
    ".venv",
    "venv",
    "venv.bak",
    "__pycache__",
    "__pypackages__",
    "node_modules",
    "target",
    ".next",
    ".turbo",
    "htmlcov",
    ".pytest_cache",
    ".mypy_cache",
    ".tox",
    ".ruff_cache",
    ".eggs",
    ".pixi",
];

/// Ambiguous directory names only skipped at the root/top level (build outputs, reports, root venvs).
/// Submodules such as `src/commands/coverage/` or `src/build/` are not skipped.
const ROOT_ONLY_SKIP_DIR_NAMES: &[&str] = &["coverage", "build", "out", "dist", "env"];

/// Unambiguous non-source directory names (test fixtures, snapshots, vendor).
const BUILTIN_IGNORE_DIR_NAMES: &[&str] = &["fixtures", "testdata", "vendor", "__snapshots__"];

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

/// Whether to avoid descending into `dir_path` during discovery (defaults to
/// top-level skip rules: only correct when `dir_path` is a direct child of the
/// scan/repo root; use `should_skip_dir_context` with an explicit flag in walks).
pub fn should_skip_dir(dir_path: &Path) -> bool {
    should_skip_dir_context(dir_path, true)
}

/// Whether to avoid descending into `dir_path` during discovery, distinguishing top-level from nested submodules.
pub fn should_skip_dir_context(dir_path: &Path, is_top_level: bool) -> bool {
    if let Some(name) = dir_path.file_name().and_then(|n| n.to_str()) {
        if ALWAYS_SKIP_DIR_NAMES.contains(&name) {
            return true;
        }
        if is_top_level && ROOT_ONLY_SKIP_DIR_NAMES.contains(&name) {
            return true;
        }
    }
    is_virtualenv_root(dir_path)
}

/// Check whether a path matches built-in non-source defaults (*.min.*, fixtures, vendor, testdata, __snapshots__).
///
/// Matches on the given path's own components. Callers walking an absolute
/// tree must pass a scan-root-relative path (see `build_path_skip_checker`),
/// otherwise a parent directory outside the scan (e.g. `/tmp/vendor/repo`)
/// would incorrectly ignore everything underneath it.
pub fn is_builtin_ignored_path(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if glob_match(name, "*.min.*") {
            return true;
        }
    }
    for component in path.components() {
        if let std::path::Component::Normal(c) = component {
            if let Some(s) = c.to_str() {
                if BUILTIN_IGNORE_DIR_NAMES.contains(&s) {
                    return true;
                }
            }
        }
    }
    false
}

/// Check whether the first ~10 lines of a file contain a `@generated` or `Code generated by ... DO NOT EDIT` marker.
pub fn has_generated_header(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut buffer = [0u8; 2048];
    let Ok(bytes_read) = (&file).take(2048).read(&mut buffer) else {
        return false;
    };
    if bytes_read == 0 {
        return false;
    }
    let text = String::from_utf8_lossy(&buffer[..bytes_read]);
    let header = text
        .lines()
        .take(10)
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    // Normalised copy so `do-not-edit`, `do_not_edit`, and `don't edit`
    // all match the `do not edit` family below (same for `auto-generated`).
    let normalized = header.replace(['-', '_'], " ").replace('\'', "");

    if header.contains("@generated")
        || header.contains("@auto-generated")
        || header.contains("@autogenerated")
        || header.contains("<auto-generated")
        || normalized.contains("@auto generated")
    {
        return true;
    }
    if (header.contains("code generated")
        || header.contains("generated by")
        || header.contains("automatically generated")
        || header.contains("autogenerated")
        || normalized.contains("auto generated")
        || normalized.contains("generated code")
        || normalized.contains("generated file"))
        && (normalized.contains("do not edit")
            || normalized.contains("do not modify")
            || normalized.contains("dont edit")
            || normalized.contains("do not change"))
    {
        return true;
    }
    false
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

fn git_available() -> bool {
    let mut cmd = Command::new("git");
    cmd.arg("--version");
    run_with_timeout(cmd, None, true, Some(Duration::from_secs(2)))
        .map(|out| out.status_code == Some(0))
        .unwrap_or(false)
}

fn git_is_ignored(git_root: &Path, rel_git_posix: &str) -> bool {
    if rel_git_posix.is_empty() {
        return false;
    }
    let mut cmd = Command::new("git");
    cmd.args(["-C"])
        .arg(git_root)
        .args(["check-ignore", "-q", "--"])
        .arg(rel_git_posix);
    run_with_timeout(cmd, None, true, Some(Duration::from_secs(1)))
        .map(|out| out.status_code == Some(0))
        .unwrap_or(false)
}

/// Compose built-in, git-ignore, and `.toposignore` checks for `scan_root`.
///
/// Built-ins are evaluated against the scan-root-relative path so a parent
/// directory outside the scan (e.g. a checkout at `/tmp/vendor/repo`) never
/// triggers a match. `.toposignore` is loaded from both `scan_root` and the
/// enclosing git root (when different), so `topos evaluate benchmarks/ -r`
/// still honours the repo-root file. Git-ignore is evaluated against the
/// git-root-relative path derived from the scan root, so relative invocations
/// like `topos evaluate .` behave the same as absolute ones.
pub fn build_path_skip_checker(scan_root: &Path) -> PathFilter {
    let git_root = find_git_root(scan_root);
    let scan_patterns = load_ignore_patterns(&scan_root.join(TOPOSIGNORE_NAME));
    let scan_root_buf = scan_root.to_path_buf();
    let scan_root_canon = scan_root
        .canonicalize()
        .unwrap_or_else(|_| scan_root_buf.clone());
    let git_patterns = git_root
        .as_ref()
        .filter(|gr| **gr != scan_root_canon)
        .map(|gr| load_ignore_patterns(&gr.join(TOPOSIGNORE_NAME)))
        .unwrap_or_default();
    // `scan_root` expressed relative to the git root ("" when they are equal,
    // `None` when the scan sits outside any git repo). Lets us derive a
    // git-relative path from a scan-relative one without canonicalizing every
    // walked file.
    let scan_rel_to_git: Option<PathBuf> = git_root.as_ref().and_then(|gr| {
        scan_root_canon
            .strip_prefix(gr)
            .ok()
            .map(|p| p.to_path_buf())
    });
    let git_usable = git_root.is_some() && git_available();
    let git_root_buf = git_root.clone();

    Box::new(move |path: &Path| {
        let rel_to_scan: PathBuf = match path.strip_prefix(&scan_root_buf) {
            Ok(rel) => rel.to_path_buf(),
            Err(_) => match path.canonicalize().ok().and_then(|abs| {
                abs.strip_prefix(&scan_root_canon)
                    .ok()
                    .map(|p| p.to_path_buf())
            }) {
                Some(rel) => rel,
                None => return false,
            },
        };
        if is_builtin_ignored_path(&rel_to_scan) {
            return true;
        }
        let rel_scan_posix = rel_to_scan.to_string_lossy().replace('\\', "/");
        if !rel_scan_posix.is_empty()
            && scan_patterns
                .iter()
                .any(|pat| matches_ignore_pattern(&rel_scan_posix, pat))
        {
            return true;
        }
        let Some(prefix) = scan_rel_to_git.as_ref() else {
            return false;
        };
        let prefix_posix = prefix.to_string_lossy().replace('\\', "/");
        let rel_git_posix = if rel_scan_posix.is_empty() {
            prefix_posix
        } else if prefix_posix.is_empty() {
            rel_scan_posix.clone()
        } else {
            format!("{prefix_posix}/{rel_scan_posix}")
        };
        if !rel_git_posix.is_empty()
            && git_patterns
                .iter()
                .any(|pat| matches_ignore_pattern(&rel_git_posix, pat))
        {
            return true;
        }
        if git_usable {
            if let Some(gr) = git_root_buf.as_ref() {
                if git_is_ignored(gr, &rel_git_posix) {
                    return true;
                }
            }
        }
        false
    })
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
    is_top_level: bool,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut subdirs = Vec::new();
    for entry in entries {
        if entry.is_dir() {
            if should_skip_dir_context(&entry, is_top_level) || ignored(&entry) {
                continue;
            }
            subdirs.push(entry);
        } else if entry.is_file()
            && has_suffix(&entry, suffixes)
            && !ignored(&entry)
            && !has_generated_header(&entry)
        {
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
        // Explicit file argument: suffix match alone wins, consistent with
        // `collect_source_files`. Ignore filters (builtin/git/toposignore) and
        // generated headers only apply to directory discovery.
        if has_suffix(root, suffixes) {
            out.push(root.to_path_buf());
        }
        return out;
    }
    if !root.is_dir() {
        return out;
    }

    // `ROOT_ONLY_SKIP_DIR_NAMES` (build/dist/coverage/...) are repo-top-level
    // outputs, not nested source modules like `src/build`. Anchor "top level"
    // to the git root when known so `topos evaluate src -r` still scans
    // `src/build`; without git info fall back to the scan root.
    let git_root = find_git_root(root);
    let root_canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root_is_repo_top = git_root.as_ref().is_some_and(|g| *g == root_canonical);
    let apply_root_only_at_scan_top = git_root.is_none() || root_is_repo_top;
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(read_dir) = std::fs::read_dir(&current) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = read_dir.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

        let is_top_level = current == root && apply_root_only_at_scan_top;
        let (files, subdirs) = scan_dir_children(entries, suffixes, &ignored, is_top_level);
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
            // Explicit file: suffix match alone wins (overrides builtin,
            // git/toposignore, and generated-header filters, which only
            // apply to directory discovery). Matches the `iter_source_files`
            // single-file branch.
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

    #[test]
    fn should_skip_dir_scopes_ambiguous_names_to_top_level() {
        assert!(should_skip_dir_context(Path::new("coverage"), true));
        assert!(should_skip_dir_context(Path::new("build"), true));
        assert!(should_skip_dir_context(Path::new("out"), true));
        assert!(should_skip_dir_context(Path::new("dist"), true));
        assert!(should_skip_dir_context(Path::new("env"), true));

        assert!(!should_skip_dir_context(
            Path::new("src/commands/coverage"),
            false
        ));
        assert!(!should_skip_dir_context(Path::new("src/build"), false));
        assert!(!should_skip_dir_context(Path::new("pkg/out"), false));
        assert!(!should_skip_dir_context(Path::new("pkg/dist"), false));
        assert!(!should_skip_dir_context(Path::new("pkg/env"), false));

        assert!(should_skip_dir_context(
            Path::new("src/commands/node_modules"),
            false
        ));
        assert!(should_skip_dir_context(Path::new("pkg/.venv"), false));
    }

    #[test]
    fn collect_source_files_scans_nested_coverage_module_and_skips_root_coverage() {
        let tmp = unique_tmp_dir("nested_coverage");
        // Root coverage dir (should be skipped)
        let root_cov = tmp.join("coverage");
        std::fs::create_dir_all(&root_cov).unwrap();
        std::fs::write(root_cov.join("index.html"), "<h1>report</h1>").unwrap();
        std::fs::write(root_cov.join("report.rs"), "fn report() {}").unwrap();

        // Nested coverage module (should be scanned)
        let nested_cov = tmp.join("src").join("commands").join("coverage");
        std::fs::create_dir_all(&nested_cov).unwrap();
        std::fs::write(nested_cov.join("publish.rs"), "pub fn publish() {}").unwrap();
        std::fs::write(nested_cov.join("complete.rs"), "pub fn complete() {}").unwrap();
        std::fs::write(
            tmp.join("src").join("commands").join("coverage.rs"),
            "pub mod publish;",
        )
        .unwrap();

        let files = collect_source_files(&[tmp.as_path()], &[".rs"], true);
        let names: BTreeSet<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert!(names.contains("publish.rs"));
        assert!(names.contains("complete.rs"));
        assert!(names.contains("coverage.rs"));
        assert!(!names.contains("report.rs"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn collect_source_files_scans_nested_build_and_env_modules() {
        let tmp = unique_tmp_dir("nested_build_env");
        let root_build = tmp.join("build");
        std::fs::create_dir_all(&root_build).unwrap();
        std::fs::write(root_build.join("out.rs"), "fn out() {}").unwrap();

        let src_build = tmp.join("src").join("build");
        std::fs::create_dir_all(&src_build).unwrap();
        std::fs::write(src_build.join("builder.rs"), "pub fn build() {}").unwrap();

        let pkg_env = tmp.join("pkg").join("env");
        std::fs::create_dir_all(&pkg_env).unwrap();
        std::fs::write(pkg_env.join("config.py"), "ENV = 'dev'").unwrap();

        let files = collect_source_files(&[tmp.as_path()], &[".rs", ".py"], true);
        let names: BTreeSet<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert!(names.contains("builder.rs"));
        assert!(names.contains("config.py"));
        assert!(!names.contains("out.rs"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn is_builtin_ignored_path_recognizes_fixtures_vendor_snapshots_and_minified() {
        assert!(is_builtin_ignored_path(Path::new("app.min.js")));
        assert!(is_builtin_ignored_path(Path::new("dist/bundle.min.ts")));
        assert!(is_builtin_ignored_path(Path::new(
            "tests/fixtures/sample.rs"
        )));
        assert!(is_builtin_ignored_path(Path::new("vendor/lib.py")));
        assert!(is_builtin_ignored_path(Path::new("pkg/testdata/data.go")));
        assert!(is_builtin_ignored_path(Path::new(
            "tests/__snapshots__/out.rs"
        )));
        assert!(!is_builtin_ignored_path(Path::new("src/main.rs")));
        assert!(!is_builtin_ignored_path(Path::new(
            "src/commands/coverage.rs"
        )));
    }

    #[test]
    fn has_generated_header_detects_various_markers() {
        let tmp = unique_tmp_dir("gen_headers");
        let f1 = tmp.join("f1.rs");
        std::fs::write(&f1, "// @generated\nfn hello() {}\n").unwrap();
        assert!(has_generated_header(&f1));

        let f2 = tmp.join("f2.go");
        std::fs::write(
            &f2,
            "// Code generated by protoc-gen-go. DO NOT EDIT.\npackage main\n",
        )
        .unwrap();
        assert!(has_generated_header(&f2));

        let f3 = tmp.join("f3.ts");
        std::fs::write(&f3, "/* @auto-generated by codegen */\nconst x = 1;\n").unwrap();
        assert!(has_generated_header(&f3));

        let f4 = tmp.join("f4.py");
        std::fs::write(
            &f4,
            "# This file was automatically generated by tool.\n# Do not modify.\nx = 1\n",
        )
        .unwrap();
        assert!(has_generated_header(&f4));

        let normal = tmp.join("normal.rs");
        std::fs::write(&normal, "// Normal hand-written code\nfn main() {}\n").unwrap();
        assert!(!has_generated_header(&normal));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn collect_source_files_skips_minified_fixtures_and_generated_headers() {
        let tmp = unique_tmp_dir("builtin_ignores");
        let src = tmp.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("normal.rs"), "fn main() {}\n").unwrap();
        std::fs::write(src.join("app.min.js"), "var x=1;").unwrap();
        std::fs::write(src.join("gen.rs"), "// @generated\npub fn gen() {}\n").unwrap();

        let fixtures = tmp.join("tests").join("fixtures");
        std::fs::create_dir_all(&fixtures).unwrap();
        std::fs::write(fixtures.join("fixture.rs"), "fn fix() {}\n").unwrap();

        let files = collect_source_files(&[tmp.as_path()], &[".rs", ".js"], true);
        assert_eq!(
            files
                .iter()
                .map(|p| p.file_name().unwrap().to_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["normal.rs"]
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn subdir_scan_keeps_nested_build_when_git_root_known() {
        // `src/build` is a nested source module, not repo-top-level output,
        // even when the scan starts at `src/`.
        let tmp = unique_tmp_dir("subdir_build_git");
        std::fs::create_dir_all(tmp.join(".git")).unwrap();
        let src_build = tmp.join("src").join("build");
        std::fs::create_dir_all(&src_build).unwrap();
        std::fs::write(src_build.join("builder.rs"), "pub fn build() {}").unwrap();

        let files = collect_source_files(&[tmp.join("src").as_path()], &[".rs"], true);
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(names.contains(&"builder.rs"), "{names:?}");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn builtin_ignores_are_scan_relative_not_absolute() {
        // A parent directory outside the scan named `vendor` must not ignore
        // everything underneath it.
        let tmp = unique_tmp_dir("parent_vendor");
        let repo = tmp.join("vendor").join("repo");
        let src = repo.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();

        let checker = build_path_skip_checker(&repo);
        assert!(!checker(&src.join("main.rs")));
        let files = collect_source_files(&[repo.as_path()], &[".rs"], true);
        assert_eq!(files.len(), 1);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn has_generated_header_detects_bare_auto_generated_variants() {
        let tmp = unique_tmp_dir("gen_variants");
        let cases = [
            ("// auto-generated file - do not edit\nfn x() {}\n", true),
            ("// GENERATED CODE - DO NOT EDIT\nfn x() {}\n", true),
            ("// Code generated by tool, do-not-edit\nfn x() {}\n", true),
            ("// Normal hand-written code\nfn main() {}\n", false),
        ];
        for (i, (contents, expected)) in cases.iter().enumerate() {
            let path = tmp.join(format!("v{i}.rs"));
            std::fs::write(&path, contents).unwrap();
            assert_eq!(has_generated_header(&path), *expected, "{contents}");
        }
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn explicit_file_overrides_generated_and_builtin_filters() {
        let tmp = unique_tmp_dir("explicit_override");
        let gen = tmp.join("gen.rs");
        std::fs::write(&gen, "// @generated\npub fn gen() {}\n").unwrap();

        let files = collect_source_files(&[gen.as_path()], &[".rs"], false);
        assert_eq!(files, vec![gen.clone()]);

        let single = iter_source_files(&gen, &[".rs"], false, None, false);
        assert_eq!(single, vec![gen]);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn toposignore_from_git_root_applies_to_subdir_scans() {
        let tmp = unique_tmp_dir("git_toposignore");
        std::fs::create_dir_all(tmp.join(".git")).unwrap();
        std::fs::write(tmp.join(".toposignore"), "benchmarks/\n").unwrap();
        let bench = tmp.join("benchmarks");
        std::fs::create_dir_all(&bench).unwrap();
        std::fs::write(bench.join("foo.rs"), "fn foo() {}\n").unwrap();

        let files = collect_source_files(&[bench.as_path()], &[".rs"], true);
        assert!(files.is_empty(), "{files:?}");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
