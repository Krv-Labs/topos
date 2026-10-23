//! Exact on-disk ownership for a managed pull-request store.
//!
//! A marker is evidence, never permission: every destructive operation still
//! validates the exact `<common>/topos-pr-<N>/{base,head}` layout, the live
//! Git registrations, and the contents of both worktrees. Legacy stores are
//! adopted only when all of that can be proved, and adoption writes the
//! marker before anything else is touched.

use std::path::{Path, PathBuf};

use super::git::{self, WorktreeRegistration};
use super::marker::{
    read_marker, validate_marker, write_marker, PrStoreMarker, DELETE_MARKER_FILE, MARKER_FILE,
};

/// The only top-level entries a managed store may own.
pub const KNOWN_ENTRIES: &[&str] = &["base", "head", "commits", MARKER_FILE, DELETE_MARKER_FILE];

/// The exact, validated paths of one PR store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreIdentity {
    pub common_dir: PathBuf,
    pub pr: u64,
    pub parent: PathBuf,
    pub base: PathBuf,
    pub head: PathBuf,
}

impl StoreIdentity {
    pub fn expected(common_dir: &Path, pr: u64) -> Self {
        let common_dir = git::normalize_path(common_dir);
        let parent = common_dir.join(format!("topos-pr-{pr}"));
        Self {
            base: parent.join("base"),
            head: parent.join("head"),
            parent,
            common_dir,
            pr,
        }
    }

    pub fn sides(&self) -> [(&'static str, PathBuf); 2] {
        [("base", self.base.clone()), ("head", self.head.clone())]
    }
}

/// Validate `parent` is exactly `<common>/topos-pr-<N>` and neither the slot
/// nor its sides are symlinks. Existing content is not inspected here.
pub fn identity(common_dir: &Path, pr: u64, parent: &Path) -> Result<StoreIdentity, String> {
    let expected = StoreIdentity::expected(common_dir, pr);
    reject_symlink(&expected.parent)?;
    reject_symlink(parent)?;
    let normalized = git::normalize_path(parent);
    if normalized != expected.parent {
        return Err(format!(
            "{} is not the expected PR store {}",
            parent.display(),
            expected.parent.display()
        ));
    }
    Ok(expected)
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("{} is a symlink", path.display()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("reading {}: {error}", path.display())),
    }
}

pub fn repo_for_common(common_dir: &Path) -> PathBuf {
    let common_dir = git::normalize_path(common_dir);
    match common_dir.parent() {
        Some(parent) if parent.join(".git") == common_dir => parent.to_path_buf(),
        _ => common_dir,
    }
}

/// The two commit lines a legacy store must carry. `elapsed_ms=` is accepted
/// as an optional third line, because every store written before ownership
/// markers existed has one.
pub fn read_commits(parent: &Path) -> Result<(String, String), String> {
    let path = parent.join("commits");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("no commits record".to_string());
        }
        Err(error) => return Err(format!("reading {}: {error}", path.display())),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("reading {}: {error}", path.display()))?;
    let mut lines = text.lines();
    let base = lines.next().unwrap_or_default().trim().to_string();
    let head = lines.next().unwrap_or_default().trim().to_string();
    let safe = |sha: &str| {
        (7..=64).contains(&sha.len()) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    if !safe(&base) || !safe(&head) {
        return Err(format!(
            "{} does not contain two hexadecimal commit ids",
            path.display()
        ));
    }
    for extra in lines {
        let extra = extra.trim();
        if extra.is_empty() {
            continue;
        }
        if !extra
            .strip_prefix("elapsed_ms=")
            .is_some_and(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(format!("{} has an unexpected third line", path.display()));
        }
    }
    Ok((base, head))
}

/// Read and validate the ownership marker for this exact store.
pub fn validate_store_marker(
    common_dir: &Path,
    pr: u64,
    parent: &Path,
) -> Result<PrStoreMarker, String> {
    let marker = read_marker(parent)?.ok_or_else(|| {
        format!(
            "{} has no ownership marker; it is not a managed PR store",
            parent.display()
        )
    })?;
    validate_marker(&marker, common_dir, pr)?;
    Ok(marker)
}

/// What must be true before a legacy (pre-marker) store may be adopted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyStore {
    pub base: String,
    pub head: String,
}

/// Validate a legacy store without writing anything.
///
/// Adoption requires the expected two-worktree layout, a two-commit record,
/// and live detached registrations at exactly those commits. Anything
/// ambiguous or foreign is refused, not repaired.
pub fn legacy_store(
    common_dir: &Path,
    repo_root: &Path,
    pr: u64,
    parent: &Path,
) -> Result<LegacyStore, String> {
    let identity = identity(common_dir, pr, parent)?;
    if read_marker(parent)?.is_some() {
        return Err("store already has an ownership marker".to_string());
    }
    if parent.join(DELETE_MARKER_FILE).exists() {
        return Err("unmarked store is already being deleted".to_string());
    }
    let (base, head) = read_commits(parent)?;

    let read_dir = std::fs::read_dir(parent)
        .map_err(|error| format!("reading {}: {error}", parent.display()))?;
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(format!("{} contains a non-UTF-8 entry", parent.display()));
        };
        if !KNOWN_ENTRIES.contains(&name) {
            return Err(format!(
                "{} contains unexpected entry '{name}'",
                parent.display()
            ));
        }
    }
    for side in ["base", "head"] {
        let path = parent.join(side);
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("reading {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!("{} is not a real directory", path.display()));
        }
    }

    let actual_common = git::git_common_dir(repo_root)?;
    if git::normalize_path(&actual_common) != identity.common_dir {
        return Err(format!(
            "{} is not the common directory of {}",
            identity.common_dir.display(),
            repo_root.display()
        ));
    }
    let registrations = git::worktree_registrations(repo_root)
        .map_err(|error| format!("could not verify worktree registrations: {error}"))?;
    for registration in &registrations {
        let path = git::normalize_path(&registration.path);
        if path.starts_with(&identity.parent) && path != identity.base && path != identity.head {
            return Err(format!(
                "unexpected worktree registration {} inside the store",
                registration.path.display()
            ));
        }
    }
    for (side, sha) in [("base", &base), ("head", &head)] {
        let path = parent.join(side);
        let registration = registration_for(&registrations, &path).ok_or_else(|| {
            format!(
                "{} is not registered as a worktree; refusing to adopt",
                path.display()
            )
        })?;
        if registration.locked {
            return Err(format!("{} is a locked worktree", path.display()));
        }
        if registration.branch.is_some() || !registration.detached {
            return Err(format!(
                "{} is attached to a branch instead of a detached commit",
                path.display()
            ));
        }
        if !head_matches(registration.head.as_deref(), sha) {
            return Err(format!(
                "{} is registered at {} but commits records {sha}",
                path.display(),
                registration.head.as_deref().unwrap_or("no HEAD")
            ));
        }
        let admin = git::registration_admin_dir(common_dir, &path)?.ok_or_else(|| {
            format!(
                "{} has no matching Git admin directory; refusing to adopt",
                path.display()
            )
        })?;
        if !git::admin_gitdir_matches(&admin, &path) {
            return Err(format!(
                "{} is claimed by an admin directory that does not name it",
                path.display()
            ));
        }
    }
    Ok(LegacyStore { base, head })
}

fn head_matches(registered: Option<&str>, wanted: &str) -> bool {
    registered
        .is_some_and(|head| head == wanted || head.starts_with(wanted) || wanted.starts_with(head))
}

pub fn registration_for<'a>(
    registrations: &'a [WorktreeRegistration],
    path: &Path,
) -> Option<&'a WorktreeRegistration> {
    let wanted = git::normalize_path(path);
    registrations
        .iter()
        .find(|registration| git::normalize_path(&registration.path) == wanted)
}

/// Mark a store as managed before a build touches it.
///
/// A missing slot and an empty slot are claimed directly. Existing content is
/// adopted only through [`legacy_store`]; anything else is refused so a
/// foreign directory is never labeled Topos-owned.
pub fn ensure_marker_for_build(
    common_dir: &Path,
    repo_root: &Path,
    pr: u64,
    parent: &Path,
) -> Result<PrStoreMarker, String> {
    let identity = identity(common_dir, pr, parent)?;
    if let Some(marker) = read_marker(parent)? {
        validate_marker(&marker, common_dir, pr)?;
        return Ok(marker);
    }
    match std::fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(format!("{} is not a real directory", parent.display()));
        }
        Ok(_) => {
            let mut entries = std::fs::read_dir(parent)
                .map_err(|error| format!("reading {}: {error}", parent.display()))?
                .flatten();
            if entries.next().is_some() {
                legacy_store(common_dir, repo_root, pr, parent).map_err(|error| {
                    format!("no ownership marker and not a valid legacy store: {error}")
                })?;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("creating {}: {error}", parent.display()))?;
        }
        Err(error) => return Err(format!("reading {}: {error}", parent.display())),
    }
    write_marker(
        parent,
        common_dir,
        repo_root,
        pr,
        Some("topos-mcp".to_string()),
    )
    .map_err(|error| {
        format!(
            "creating ownership marker in {}: {error}",
            identity.parent.display()
        )
    })
}

/// Adopt a fully validated legacy store, writing its marker before the caller
/// removes anything. Returns `None` when the store is not adoptable.
pub fn adopt_legacy(
    common_dir: &Path,
    repo_root: &Path,
    pr: u64,
    parent: &Path,
) -> Result<Option<PrStoreMarker>, String> {
    match legacy_store(common_dir, repo_root, pr, parent) {
        Ok(_) => write_marker(
            parent,
            common_dir,
            repo_root,
            pr,
            Some("topos-mcp".to_string()),
        )
        .map(Some),
        Err(_) => Ok(None),
    }
}

/// Why content cannot be force-removed or force-checked-out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentReport {
    pub blockers: Vec<String>,
    pub unknown_entries: Vec<String>,
    pub base_present: bool,
    pub head_present: bool,
}

impl ContentReport {
    pub fn safe(&self) -> bool {
        self.blockers.is_empty()
    }

    pub fn reason(&self) -> Option<String> {
        (!self.blockers.is_empty()).then(|| self.blockers.join("; "))
    }
}

/// Inspect a managed store before any force operation.
///
/// `.gitnexus/` is the only disposable content. Everything else that Git does
/// not consider clean blocks, as do attached or locked registrations,
/// unexpected registrations inside the slot, nested repositories, and a
/// current directory inside the store.
pub fn assess_content(
    common_dir: &Path,
    pr: u64,
    parent: &Path,
    repo_root: &Path,
) -> ContentReport {
    let cwd = std::env::current_dir().ok();
    assess_content_at(common_dir, pr, parent, repo_root, cwd.as_deref())
}

fn assess_content_at(
    common_dir: &Path,
    pr: u64,
    parent: &Path,
    repo_root: &Path,
    cwd: Option<&Path>,
) -> ContentReport {
    let mut report = ContentReport::default();
    let identity = match identity(common_dir, pr, parent) {
        Ok(identity) => identity,
        Err(error) => {
            report.blockers.push(error);
            return report;
        }
    };

    if let Some(cwd) = cwd {
        let cwd = git::normalize_path(cwd);
        if cwd.starts_with(&identity.parent) {
            report
                .blockers
                .push("current directory is inside the PR store".to_string());
        }
    }

    if let Ok(read_dir) = std::fs::read_dir(parent) {
        let mut names: Vec<String> = read_dir
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .collect();
        names.sort();
        for name in names {
            if !KNOWN_ENTRIES.contains(&name.as_str()) {
                report.unknown_entries.push(name);
            }
        }
    } else if parent.exists() {
        report
            .blockers
            .push(format!("cannot read {}", parent.display()));
    }

    let registrations = match git::worktree_registrations(repo_root) {
        Ok(registrations) => registrations,
        Err(error) => {
            report
                .blockers
                .push(format!("could not verify worktree registrations: {error}"));
            Vec::new()
        }
    };
    for registration in &registrations {
        let path = git::normalize_path(&registration.path);
        if path.starts_with(&identity.parent) && path != identity.base && path != identity.head {
            report.blockers.push(format!(
                "unexpected worktree registration {} inside the store",
                registration.path.display()
            ));
        }
    }
    if let Ok(admins) = git::worktree_admin_dirs(common_dir) {
        for admin in admins {
            let Ok(claimed) = git::admin_gitdir_path(&admin) else {
                continue;
            };
            let Some(side) = claimed.parent() else {
                continue;
            };
            let side = git::normalize_path(side);
            if side.starts_with(&identity.parent) && side != identity.base && side != identity.head
            {
                report.blockers.push(format!(
                    "admin registration {} names unexpected worktree {}",
                    admin.display(),
                    side.display()
                ));
            }
        }
    } else {
        report.blockers.push(format!(
            "cannot read {}",
            common_dir.join("worktrees").display()
        ));
    }

    for (name, path) in [("base", &identity.base), ("head", &identity.head)] {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                report
                    .blockers
                    .push(format!("reading {}: {error}", path.display()));
                continue;
            }
        };
        if name == "base" {
            report.base_present = true;
        } else {
            report.head_present = true;
        }
        if metadata.file_type().is_symlink() {
            report
                .blockers
                .push(format!("{} is a symlink", path.display()));
            continue;
        }
        if !metadata.is_dir() {
            report
                .blockers
                .push(format!("{} is not a directory", path.display()));
            continue;
        }
        if registration_for(&registrations, path).is_none() {
            report.blockers.push(format!(
                "{} exists but is not a registered Git worktree",
                path.display()
            ));
        }
        match std::fs::symlink_metadata(path.join(".git")) {
            Ok(metadata) if metadata.file_type().is_symlink() => report
                .blockers
                .push(format!("{}/.git is a symlink", path.display())),
            Ok(metadata) if metadata.is_file() => {}
            Ok(metadata) if metadata.is_dir() => report.blockers.push(format!(
                "{}/.git is a nested repository, not a linked worktree",
                path.display()
            )),
            Ok(_) => report.blockers.push(format!(
                "{}/.git is not a Git metadata file",
                path.display()
            )),
            Err(error) => report
                .blockers
                .push(format!("{} is not a Git worktree: {error}", path.display())),
        }
        if let Err(error) = checkout_blockers(path) {
            report.blockers.push(error);
        }
        if let Some(nested) = nested_repository(path) {
            report.blockers.push(format!(
                "{} contains nested repository {}",
                path.display(),
                nested.display()
            ));
        }
    }
    for registration in &registrations {
        let path = git::normalize_path(&registration.path);
        if path == identity.base || path == identity.head {
            if registration.locked {
                report.blockers.push(format!(
                    "{} is a locked worktree",
                    registration.path.display()
                ));
            }
            if registration.branch.is_some() || !registration.detached {
                report.blockers.push(format!(
                    "{} is attached to a branch instead of a detached commit",
                    registration.path.display()
                ));
            }
        }
    }
    report.blockers.sort();
    report.blockers.dedup();
    report
}

fn tracked_graph_blockers(worktree: &Path) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["ls-files", "-z", "--", ".gitnexus"])
        .output()
        .map_err(|error| {
            format!(
                "listing tracked graph files in {}: {error}",
                worktree.display()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files in {}: {}",
            worktree.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if output.stdout.is_empty() {
        return Ok(());
    }
    let mut paths: Vec<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect();
    paths.sort();
    Err(format!(
        "{} has tracked content under .gitnexus/: {}",
        worktree.display(),
        paths.join(", ")
    ))
}

/// Reject a target commit whose tree tracks `.gitnexus/` content. Checking the
/// current index is not enough: a forced checkout could bring tracked graph
/// files into the worktree immediately before graph cleanup.
pub fn target_graph_blockers(repo: &Path, sha: &str) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-tree", "-r", "--name-only", "-z", sha, "--", ".gitnexus"])
        .output()
        .map_err(|error| format!("listing tracked graph files in {sha}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-tree {sha} in {}: {}",
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if output.stdout.is_empty() {
        return Ok(());
    }
    let mut paths: Vec<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect();
    paths.sort();
    Err(format!(
        "{sha} tracks content under .gitnexus/: {}",
        paths.join(", ")
    ))
}

/// Paths in `worktree` that Git would lose on a forced checkout. `.gitnexus/`
/// and untracked `.claude/skills/gitnexus/` files generated by older GitNexus
/// runs are deliberately ignored only when they are real directories and
/// untracked.
pub fn checkout_blockers(worktree: &Path) -> Result<(), String> {
    tracked_graph_blockers(worktree)?;
    validate_disposable_dir(&worktree.join(".gitnexus"), "generated graph directory")?;
    validate_disposable_gitnexus_skills(worktree)?;
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--ignored",
            "--untracked-files=all",
        ])
        .output()
        .map_err(|error| format!("running git status in {}: {error}", worktree.display()))?;
    if !output.status.success() {
        return Err(format!(
            "git status in {}: {}",
            worktree.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut blocked = Vec::new();
    let mut records = output.stdout.split(|byte| *byte == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 {
            continue;
        }
        let status = &record[..2];
        let path = &record[3..];
        if status.first() == Some(&b'R') || status.first() == Some(&b'C') {
            let _ = records.next();
        }
        if path == b".gitnexus" || path.starts_with(b".gitnexus/") {
            continue;
        }
        if status == b"??"
            && (path == b".claude/skills/gitnexus" || path.starts_with(b".claude/skills/gitnexus/"))
        {
            continue;
        }
        blocked.push(String::from_utf8_lossy(path).into_owned());
    }
    if blocked.is_empty() {
        Ok(())
    } else {
        blocked.sort();
        blocked.dedup();
        Err(format!(
            "{} has content outside .gitnexus/: {}",
            worktree.display(),
            blocked.join(", ")
        ))
    }
}

fn validate_disposable_dir(path: &Path, label: &str) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("{} is a symlink, not a {label}", path.display()))
        }
        Ok(metadata) if !metadata.is_dir() => Err(format!("{} is not a {label}", path.display())),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("reading {}: {error}", path.display())),
    }
}

fn validate_disposable_gitnexus_skills(worktree: &Path) -> Result<(), String> {
    for rel in [".claude", ".claude/skills", ".claude/skills/gitnexus"] {
        let path = worktree.join(rel);
        if !validate_disposable_dir(&path, "directory")? {
            break;
        }
    }
    Ok(())
}

fn tracked_gitnexus_skills_exist(worktree: &Path) -> Result<bool, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["ls-files", "-z", "--", ".claude/skills/gitnexus"])
        .output()
        .map_err(|error| {
            format!(
                "listing tracked GitNexus skill files in {}: {error}",
                worktree.display()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files in {}: {}",
            worktree.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(!output.stdout.is_empty())
}

fn remove_disposable_gitnexus_skills(worktree: &Path) -> Result<(), String> {
    let skills = worktree.join(".claude/skills/gitnexus");
    if std::fs::symlink_metadata(&skills)
        .err()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(());
    }
    validate_disposable_gitnexus_skills(worktree)?;
    if tracked_gitnexus_skills_exist(worktree)? {
        return Ok(());
    }
    std::fs::remove_dir_all(&skills)
        .map_err(|error| format!("removing {}: {error}", skills.display()))?;
    for rel in [".claude/skills", ".claude"] {
        let dir = worktree.join(rel);
        let is_empty = std::fs::read_dir(&dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if is_empty {
            let _ = std::fs::remove_dir(&dir);
        } else {
            break;
        }
    }
    Ok(())
}

fn nested_repository(worktree: &Path) -> Option<PathBuf> {
    let mut stack = vec![worktree.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name == ".git" || name == ".gitnexus" {
                continue;
            }
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(_) => continue,
            };
            if kind.is_symlink() || !kind.is_dir() {
                continue;
            }
            let path = entry.path();
            if path.join(".git").exists() {
                return Some(path);
            }
            stack.push(path);
        }
    }
    None
}

/// Remove only a worktree's generated `.gitnexus/` directory (and any
/// untracked `.claude/skills/gitnexus/` directory injected by older GitNexus
/// runs), never following a symlink planted in its place. A file or symlink
/// with that name is user content, not disposable graph data.
pub fn remove_disposable_graph(worktree: &Path) -> Result<(), String> {
    let path = worktree.join(".gitnexus");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                std::fs::remove_dir_all(&path)
                    .map_err(|error| format!("removing {}: {error}", path.display()))?;
            } else {
                return Err(format!(
                    "{} is not a generated graph directory; refusing to remove it",
                    path.display()
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("reading {}: {error}", path.display())),
    }
    remove_disposable_gitnexus_skills(worktree)
}

/// Remove a stale Git admin record only when it provably belongs to `side`.
/// The caller must hold the repository worktree lock.
pub fn remove_stale_registration(common_dir: &Path, side: &Path) -> Result<bool, String> {
    let Some(admin) = git::registration_admin_dir(common_dir, side)? else {
        return Ok(false);
    };
    let worktrees = git::normalize_path(&common_dir.join("worktrees"));
    let admin_parent = admin.parent().map(git::normalize_path).unwrap_or_default();
    if admin_parent != worktrees {
        return Err(format!(
            "refusing to remove admin directory outside {}",
            worktrees.display()
        ));
    }
    if !git::admin_gitdir_matches(&admin, side) {
        return Err(format!(
            "{} is not owned by the registration in {}",
            side.display(),
            admin.display()
        ));
    }
    std::fs::remove_dir_all(&admin)
        .map_err(|error| format!("removing stale registration {}: {error}", admin.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn git_ok(repo: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn repo() -> (tempfile::TempDir, PathBuf, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_path_buf();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.email", "cache@example.com"]);
        git(&repo, &["config", "user.name", "Cache"]);
        let commit = |body: &str| {
            std::fs::write(repo.join("a.txt"), body).unwrap();
            git(&repo, &["add", "."]);
            git(&repo, &["commit", "-qm", body]);
            git(&repo, &["rev-parse", "HEAD"])
        };
        let base = commit("base");
        let head = commit("head");
        (dir, repo, base, head)
    }

    fn common(repo: &Path) -> PathBuf {
        git::git_common_dir(repo).unwrap()
    }

    fn worktree(repo: &Path, path: &Path, sha: &str) {
        git(
            repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                &path.display().to_string(),
                sha,
            ],
        );
    }

    fn legacy(repo: &Path, pr: u64, base: &str, head: &str) -> StoreIdentity {
        let identity = StoreIdentity::expected(&common(repo), pr);
        std::fs::create_dir_all(&identity.parent).unwrap();
        worktree(repo, &identity.base, base);
        worktree(repo, &identity.head, head);
        std::fs::write(identity.parent.join("commits"), format!("{base}\n{head}\n")).unwrap();
        identity
    }

    #[test]
    fn identity_rejects_redirected_and_foreign_slots() {
        let (_keep, repo, _, _) = repo();
        let common = common(&repo);
        let expected = StoreIdentity::expected(&common, 4);
        std::fs::create_dir_all(&expected.parent).unwrap();
        identity(&common, 4, &expected.parent).unwrap();
        assert!(identity(&common, 5, &expected.parent).is_err());

        #[cfg(unix)]
        {
            let elsewhere = repo.join("elsewhere");
            std::fs::create_dir_all(&elsewhere).unwrap();
            let slot = common.join("topos-pr-9");
            std::os::unix::fs::symlink(&elsewhere, &slot).unwrap();
            assert!(identity(&common, 9, &slot).is_err());
        }
    }

    #[test]
    fn legacy_adoption_requires_live_detached_worktrees_and_writes_the_marker_first() {
        let (_keep, repo, base, head) = repo();
        let identity = legacy(&repo, 7, &base, &head);
        let common = common(&repo);
        let candidate = legacy_store(&common, &repo, 7, &identity.parent).unwrap();
        assert_eq!(candidate.base, base);
        let marker = adopt_legacy(&common, &repo, 7, &identity.parent)
            .unwrap()
            .expect("adoptable");
        assert_eq!(marker.pr, 7);
        assert!(identity.parent.join(MARKER_FILE).is_file());
        assert!(read_marker(&identity.parent).unwrap().is_some());
    }

    #[test]
    fn attached_locked_or_unregistered_worktrees_are_refused() {
        let (_keep, repo, base, head) = repo();
        let identity = legacy(&repo, 8, &base, &head);
        let common = common(&repo);
        git(
            &repo,
            &["worktree", "lock", &identity.base.display().to_string()],
        );
        assert!(legacy_store(&common, &repo, 8, &identity.parent).is_err());
        git(
            &repo,
            &["worktree", "unlock", &identity.base.display().to_string()],
        );

        std::fs::remove_dir_all(&identity.head).unwrap();
        assert!(legacy_store(&common, &repo, 8, &identity.parent).is_err());
    }

    #[test]
    fn malformed_commits_and_unexpected_entries_are_never_adopted() {
        let (_keep, repo, base, head) = repo();
        let identity = legacy(&repo, 9, &base, &head);
        let common = common(&repo);
        std::fs::write(
            identity.parent.join("commits"),
            format!("{base}\n{head}\nnot-elapsed\n"),
        )
        .unwrap();
        assert!(legacy_store(&common, &repo, 9, &identity.parent).is_err());

        std::fs::write(identity.parent.join("commits"), format!("{base}\n{head}\n")).unwrap();
        std::fs::write(identity.parent.join("user-notes.txt"), "keep").unwrap();
        assert!(legacy_store(&common, &repo, 9, &identity.parent).is_err());
        assert!(identity.parent.join("user-notes.txt").exists());
    }

    #[test]
    fn content_outside_the_graph_allowlist_blocks() {
        let (_keep, repo, base, head) = repo();
        let identity = legacy(&repo, 10, &base, &head);
        let common = common(&repo);
        ensure_marker_for_build(&common, &repo, 10, &identity.parent).unwrap();
        std::fs::create_dir_all(identity.base.join(".gitnexus")).unwrap();
        let report = assess_content_at(&common, 10, &identity.parent, &repo, Some(&repo));
        assert!(report.safe(), "{:?}", report.blockers);

        std::fs::write(identity.base.join("stray.txt"), "user").unwrap();
        let report = assess_content_at(&common, 10, &identity.parent, &repo, Some(&repo));
        assert!(!report.safe());
        assert!(report.reason().unwrap().contains("stray.txt"));
        std::fs::remove_file(identity.base.join("stray.txt")).unwrap();

        let nested = identity.base.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(git_ok(&nested, &["init", "-q"]));
        let report = assess_content_at(&common, 10, &identity.parent, &repo, Some(&repo));
        assert!(!report.safe());
        assert!(report.reason().unwrap().contains("nested repository"));
    }

    #[test]
    fn a_current_directory_inside_the_store_or_an_unexpected_registration_blocks() {
        let (_keep, repo, base, head) = repo();
        let identity = legacy(&repo, 11, &base, &head);
        let common = common(&repo);
        ensure_marker_for_build(&common, &repo, 11, &identity.parent).unwrap();
        let report = assess_content_at(&common, 11, &identity.parent, &repo, Some(&identity.base));
        assert!(!report.safe());
        assert!(report.reason().unwrap().contains("current directory"));

        let extra = identity.parent.join("extra");
        worktree(&repo, &extra, &base);
        let report = assess_content_at(&common, 11, &identity.parent, &repo, Some(&repo));
        assert!(!report.safe());
        assert!(report.reason().unwrap().contains("unexpected worktree"));
    }

    #[test]
    fn only_the_graph_directory_is_disposable() {
        let (_keep, repo, _, _) = repo();
        let side = repo.join("side");
        std::fs::create_dir_all(side.join(".gitnexus")).unwrap();
        std::fs::write(side.join(".gitnexus/lbug"), b"index").unwrap();
        std::fs::write(side.join("keep.txt"), "keep").unwrap();
        remove_disposable_graph(&side).unwrap();
        assert!(!side.join(".gitnexus").exists());
        assert!(side.join("keep.txt").exists());
    }

    #[test]
    fn a_file_named_gitnexus_is_user_content() {
        let (_keep, repo, _, _) = repo();
        let side = repo.join("side");
        std::fs::create_dir_all(&side).unwrap();
        std::fs::write(side.join(".gitnexus"), "not a generated graph").unwrap();

        let error = remove_disposable_graph(&side).unwrap_err();
        assert!(error.contains("not a generated graph directory"), "{error}");
        assert_eq!(
            std::fs::read_to_string(side.join(".gitnexus")).unwrap(),
            "not a generated graph"
        );

        std::fs::remove_file(side.join(".gitnexus")).unwrap();
        std::fs::remove_dir(&side).unwrap();
        std::fs::write(repo.join(".gitnexus"), "not a generated graph").unwrap();
        let error = checkout_blockers(&repo).unwrap_err();
        assert!(error.contains("not a generated graph directory"), "{error}");
        assert!(repo.join(".gitnexus").exists());
    }

    #[test]
    fn tracked_graph_content_is_not_disposable() {
        let (_keep, repo, base, _) = repo();
        std::fs::create_dir_all(repo.join(".gitnexus")).unwrap();
        std::fs::write(repo.join(".gitnexus/note.txt"), "tracked").unwrap();
        git(&repo, &["add", ".gitnexus/note.txt"]);
        git(&repo, &["commit", "-qm", "track graph note"]);
        let tracked = git(&repo, &["rev-parse", "HEAD"]);

        let error = target_graph_blockers(&repo, &tracked).unwrap_err();
        assert!(error.contains("tracks content under .gitnexus"), "{error}");
        assert!(target_graph_blockers(&repo, &base).is_ok());
        let error = checkout_blockers(&repo).unwrap_err();
        assert!(error.contains("tracked content under .gitnexus"), "{error}");
        assert_eq!(
            std::fs::read_to_string(repo.join(".gitnexus/note.txt")).unwrap(),
            "tracked"
        );
    }

    #[test]
    fn untracked_gitnexus_skills_are_disposable_and_cleaned_up() {
        let (_keep, repo, base, head) = repo();
        let identity = legacy(&repo, 12, &base, &head);
        let common = common(&repo);
        ensure_marker_for_build(&common, &repo, 12, &identity.parent).unwrap();
        let skill = identity
            .base
            .join(".claude/skills/gitnexus/gitnexus-debugging/SKILL.md");
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        std::fs::write(&skill, "generated skill").unwrap();

        let report = assess_content_at(&common, 12, &identity.parent, &repo, Some(&repo));
        assert!(report.safe(), "{:?}", report.blockers);

        remove_disposable_graph(&identity.base).unwrap();
        assert!(!identity.base.join(".claude/skills/gitnexus").exists());
        assert!(!identity.base.join(".claude").exists());

        // Non-gitnexus skills under .claude/skills must still block and be preserved.
        let custom_skill = identity.base.join(".claude/skills/custom/SKILL.md");
        std::fs::create_dir_all(custom_skill.parent().unwrap()).unwrap();
        std::fs::write(&custom_skill, "user skill").unwrap();
        let report = assess_content_at(&common, 12, &identity.parent, &repo, Some(&repo));
        assert!(!report.safe());
        assert!(report.reason().unwrap().contains(".claude/skills/custom"));
    }

    #[test]
    fn tracked_or_symlinked_gitnexus_skills_are_not_disposable() {
        let (_keep, repo, _, _) = repo();
        let skill = repo.join(".claude/skills/gitnexus/gitnexus-cli/SKILL.md");
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        std::fs::write(&skill, "tracked skill").unwrap();
        git(
            &repo,
            &["add", ".claude/skills/gitnexus/gitnexus-cli/SKILL.md"],
        );
        git(&repo, &["commit", "-qm", "track gitnexus skill"]);

        // Modifying a tracked file under .claude/skills/gitnexus must block.
        std::fs::write(&skill, "modified skill").unwrap();
        let error = checkout_blockers(&repo).unwrap_err();
        assert!(
            error.contains(".claude/skills/gitnexus/gitnexus-cli/SKILL.md"),
            "{error}"
        );
        // remove_disposable_graph must not delete tracked .claude/skills/gitnexus files.
        remove_disposable_graph(&repo).unwrap();
        assert!(skill.exists());
    }
}
