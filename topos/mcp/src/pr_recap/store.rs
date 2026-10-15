//! Where a pull request's coupling graphs live, and what is already built
//! there.
//!
//! Pull request N owns `<git-common-dir>/topos-pr-<N>/`: a `base/` and a
//! `head/` worktree, each with its `.gitnexus/` graph, and a `commits`
//! file. `commits` names the base and head commits those graphs were built
//! from, one per line, then `elapsed_ms=<n>`: how long that build took.
//! Readers take the first two lines only, so a file written before the
//! third line existed still reads. Everything here is plain file reads,
//! quick enough to run before deciding whether to ask about a build.

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use topos_engine::adapters::gitnexus::{generate_depgraph, GITNEXUS_FINGERPRINT_FILE};

use super::gh::{git, git_common_dir, resolve_commit};

/// Paths to the worktrees (and their shared parent) backing a PR's coupling
/// graphs.
pub struct PrStores {
    pub parent: PathBuf,
    pub base: PathBuf,
    pub head: PathBuf,
}

impl PrStores {
    /// Pull request `pr`'s stores for the repository at `repo_root`, in the
    /// `.git` directory its worktrees share.
    pub fn locate(repo_root: &Path, pr: u64) -> Result<Self, String> {
        Ok(Self::at(
            git_common_dir(repo_root)?.join(format!("topos-pr-{pr}")),
        ))
    }

    pub fn at(parent: PathBuf) -> Self {
        Self {
            base: parent.join("base"),
            head: parent.join("head"),
            parent,
        }
    }

    pub fn commits_path(&self) -> PathBuf {
        self.parent.join("commits")
    }
}

/// How much of a build the stores already hold for one base/head pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreState {
    /// Both graphs are built at these commits: nothing to do.
    Ready,
    /// The base graph is built at this base; the head needs building.
    BaseReusable,
    /// The head graph is built at this head; the base needs building.
    HeadReusable,
    /// Neither graph is built at these commits.
    Cold,
}

impl StoreState {
    pub fn one_side_reusable(self) -> bool {
        matches!(self, StoreState::BaseReusable | StoreState::HeadReusable)
    }
}

/// What `commits` records.
struct Commits {
    base: String,
    head: String,
    elapsed_ms: Option<u64>,
}

fn read_commits(path: &Path) -> Option<Commits> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let base = lines.next()?.to_string();
    let head = lines.next()?.to_string();
    let elapsed_ms = lines
        .next()
        .and_then(|line| line.strip_prefix("elapsed_ms="))
        .and_then(|value| value.trim().parse().ok());
    Some(Commits {
        base,
        head,
        elapsed_ms,
    })
}

/// Record that the graphs on disk were built from `base_sha`/`head_sha`,
/// in `elapsed_ms`.
pub fn write_commits(
    stores: &PrStores,
    base_sha: &str,
    head_sha: &str,
    elapsed_ms: u64,
) -> std::io::Result<()> {
    std::fs::write(
        stores.commits_path(),
        format!("{base_sha}\n{head_sha}\nelapsed_ms={elapsed_ms}\n"),
    )
}

/// What `stores` already holds for `base_sha`/`head_sha`. A side counts as
/// built only when `commits` names its commit and its graph is on disk;
/// `commits` is removed before any rebuild, so it never names a graph
/// that is half built.
pub fn pr_store_state(stores: &PrStores, base_sha: &str, head_sha: &str) -> StoreState {
    let Some(recorded) = read_commits(&stores.commits_path()) else {
        return StoreState::Cold;
    };
    let base = recorded.base == base_sha && stores.base.join(".gitnexus").exists();
    let head = recorded.head == head_sha && stores.head.join(".gitnexus").exists();
    match (base, head) {
        (true, true) => StoreState::Ready,
        (true, false) => StoreState::BaseReusable,
        (false, true) => StoreState::HeadReusable,
        (false, false) => StoreState::Cold,
    }
}

/// How long the last pull request build under `store_root` (the directory
/// holding every `topos-pr-*`) took, in milliseconds.
///
/// The newest recorded `elapsed_ms` is the measure. Failing that, the
/// longest graph a fingerprint records, ×1.3 for the second side and the
/// checkout. Fingerprints are only the fallback: a gitnexus run that finds
/// its index current returns in about a second and rewrites them.
pub fn last_build_ms(store_root: &Path) -> Option<u64> {
    let stores: Vec<PrStores> = std::fs::read_dir(store_root)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("topos-pr-"))
        .map(|entry| PrStores::at(entry.path()))
        .collect();
    let recorded = stores
        .iter()
        .filter_map(|store| {
            let path = store.commits_path();
            let elapsed = read_commits(&path)?.elapsed_ms?;
            let written = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((written, elapsed))
        })
        .max_by_key(|(written, _)| *written)
        .map(|(_, elapsed)| elapsed);
    recorded.or_else(|| {
        stores
            .iter()
            .flat_map(|store| [&store.base, &store.head])
            .filter_map(|side| fingerprint_ms(side))
            .max()
            .map(|longest| longest * 13 / 10)
    })
}

/// How long the graph at `side` took to build, from its fingerprint.
fn fingerprint_ms(side: &Path) -> Option<u64> {
    let text =
        std::fs::read_to_string(side.join(".gitnexus").join(GITNEXUS_FINGERPRINT_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let seconds = value.get("finished_at")?.as_f64()? - value.get("generated_at")?.as_f64()?;
    (seconds > 0.0).then_some((seconds * 1000.0) as u64)
}

/// The wait to quote for a build from `state`, rounded to 5 s, or `None`
/// with no history to go on. A reusable side makes it about 0.8× as long.
pub fn build_estimate_ms(state: StoreState, last_build_ms: Option<u64>) -> Option<u64> {
    let ms = last_build_ms?;
    let ms = if state.one_side_reusable() {
        ms * 4 / 5
    } else {
        ms
    };
    Some(((ms + 2_500) / 5_000).max(1) * 5_000)
}

/// Check out `base_sha` and `head_sha` into `<git-common-dir>/topos-pr-<pr>/`
/// and build a `.gitnexus/` graph in each, in parallel.
///
/// If `commits` already names `base_sha` and `head_sha` and both `.gitnexus/`
/// directories exist, returns immediately. Otherwise `commits` is removed
/// before either worktree is touched so an interrupted build can never leave
/// a stale record claiming the new commits are indexed; it is rewritten only
/// after both graphs succeed, naming the exact commits those graphs were built
/// from. `on_side_done` runs from each side's build thread as that side
/// finishes, in whichever order they finish. Never prints. Errors are
/// `String`.
pub fn prepare_pr_stores(
    repo_root: &Path,
    pr: u64,
    base_sha: &str,
    head_sha: &str,
    on_side_done: &(dyn Fn() + Sync),
) -> Result<PrStores, String> {
    let stores = PrStores::locate(repo_root, pr)?;
    if pr_store_state(&stores, base_sha, head_sha) == StoreState::Ready {
        return Ok(stores);
    }

    let started = Instant::now();
    match std::fs::remove_file(stores.commits_path()) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("clearing graph commits: {e}")),
    }
    std::fs::create_dir_all(&stores.parent)
        .map_err(|e| format!("creating graph worktrees: {e}"))?;
    ensure_worktree(repo_root, &stores.base, base_sha)?;
    ensure_worktree(repo_root, &stores.head, head_sha)?;

    let (base_result, head_result) = std::thread::scope(|scope| {
        let [base_job, head_job] = [&stores.base, &stores.head].map(|path| {
            scope.spawn(move || {
                let result = generate_depgraph(path, true, None);
                on_side_done();
                result
            })
        });
        (base_job.join(), head_job.join())
    });
    let base_result = base_result.map_err(|_| "base graph generation failed".to_string())?;
    let head_result = head_result.map_err(|_| "head graph generation failed".to_string())?;
    if !base_result.ok {
        return Err(format!("base graph: {}", base_result.message));
    }
    if !head_result.ok {
        return Err(format!("head graph: {}", head_result.message));
    }
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    write_commits(&stores, base_sha, head_sha, elapsed_ms)
        .map_err(|e| format!("recording graph commits: {e}"))?;

    Ok(stores)
}

/// Check out `sha`, detached, at `path`: a worktree of `repo` that topos
/// owns. A worktree already on `sha` is left alone, graph included. One on
/// another commit is forced onto `sha` and stripped of every untracked and
/// ignored file — the old `.gitnexus/` with them — so the next graph can
/// only reflect `sha`. A directory git no longer tracks as a worktree (or a
/// registration whose directory was deleted) is rebuilt from scratch.
pub fn ensure_worktree(repo: &Path, path: &Path, sha: &str) -> Result<(), String> {
    let wanted = resolve_commit(repo, sha)?;
    if path.join(".git").exists() {
        if let Ok(current) = git(path, &["rev-parse", "HEAD"]) {
            if current.trim() == wanted {
                return Ok(());
            }
            git(
                path,
                &["checkout", "--quiet", "--force", "--detach", &wanted],
            )
            .map_err(|e| format!("could not check out {sha}: {e}"))?;
            git(path, &["clean", "-ffdxq"])?;
            return Ok(());
        }
    }
    if path.exists() {
        std::fs::remove_dir_all(path)
            .map_err(|e| format!("removing broken worktree {}: {e}", path.display()))?;
    }
    // Forget a registration whose directory is gone, or `add` refuses the path.
    git(repo, &["worktree", "prune"])?;
    git(
        repo,
        &[
            "worktree",
            "add",
            "--detach",
            &path.display().to_string(),
            &wanted,
        ],
    )
    .map(|_| ())
    .map_err(|e| format!("could not check out {sha}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stores() -> (tempfile::TempDir, PrStores) {
        let dir = tempfile::tempdir().expect("tempdir");
        let stores = PrStores::at(dir.path().join("topos-pr-7"));
        std::fs::create_dir_all(&stores.parent).unwrap();
        (dir, stores)
    }

    fn built(side: &Path) {
        std::fs::create_dir_all(side.join(".gitnexus")).unwrap();
    }

    #[test]
    fn the_state_reads_commits_and_both_graphs() {
        let (_keep, stores) = stores();
        assert_eq!(pr_store_state(&stores, "b", "h"), StoreState::Cold);

        built(&stores.base);
        built(&stores.head);
        write_commits(&stores, "b", "h", 26_500).unwrap();
        assert_eq!(pr_store_state(&stores, "b", "h"), StoreState::Ready);
        assert_eq!(pr_store_state(&stores, "b", "h2"), StoreState::BaseReusable);
        assert_eq!(pr_store_state(&stores, "b2", "h"), StoreState::HeadReusable);
        assert_eq!(pr_store_state(&stores, "b2", "h2"), StoreState::Cold);

        std::fs::remove_dir_all(stores.head.join(".gitnexus")).unwrap();
        assert_eq!(
            pr_store_state(&stores, "b", "h"),
            StoreState::BaseReusable,
            "a recorded commit without its graph is not built"
        );
    }

    #[test]
    fn a_two_line_commits_file_still_reads() {
        let (_keep, stores) = stores();
        built(&stores.base);
        built(&stores.head);
        std::fs::write(stores.commits_path(), "b\nh\n").unwrap();
        assert_eq!(pr_store_state(&stores, "b", "h"), StoreState::Ready);
        let commits = read_commits(&stores.commits_path()).unwrap();
        assert_eq!(commits.elapsed_ms, None);

        write_commits(&stores, "b", "h", 1_234).unwrap();
        let text = std::fs::read_to_string(stores.commits_path()).unwrap();
        assert_eq!(text, "b\nh\nelapsed_ms=1234\n");
        let mut lines = text.lines();
        assert_eq!(
            (lines.next(), lines.next()),
            (Some("b"), Some("h")),
            "the first two lines stay the commits"
        );
        assert_eq!(pr_store_state(&stores, "b", "h"), StoreState::Ready);
    }

    #[test]
    fn the_newest_recorded_build_wins() {
        let dir = tempfile::tempdir().expect("tempdir");
        let old = PrStores::at(dir.path().join("topos-pr-1"));
        let new = PrStores::at(dir.path().join("topos-pr-2"));
        for store in [&old, &new] {
            std::fs::create_dir_all(&store.parent).unwrap();
        }
        write_commits(&old, "b", "h", 40_000).unwrap();
        let past = SystemTime::now() - std::time::Duration::from_secs(3_600);
        std::fs::File::options()
            .write(true)
            .open(old.commits_path())
            .unwrap()
            .set_modified(past)
            .unwrap();
        write_commits(&new, "b", "h", 21_000).unwrap();
        assert_eq!(last_build_ms(dir.path()), Some(21_000));
    }

    #[test]
    fn fingerprints_are_the_fallback_and_nothing_is_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(last_build_ms(dir.path()), None);
        assert_eq!(last_build_ms(&dir.path().join("missing")), None);

        let store = PrStores::at(dir.path().join("topos-pr-3"));
        let fingerprint = |side: &Path, seconds: f64| {
            built(side);
            std::fs::write(
                side.join(".gitnexus").join(GITNEXUS_FINGERPRINT_FILE),
                format!(
                    r#"{{"generated_at": 100.0, "finished_at": {}}}"#,
                    100.0 + seconds
                ),
            )
            .unwrap();
        };
        fingerprint(&store.base, 1.0);
        fingerprint(&store.head, 20.0);
        // A two-line commits file records no time: the fingerprints decide.
        std::fs::write(store.commits_path(), "b\nh\n").unwrap();
        assert_eq!(last_build_ms(dir.path()), Some(26_000));

        write_commits(&store, "b", "h", 9_000).unwrap();
        assert_eq!(last_build_ms(dir.path()), Some(9_000));
    }

    #[test]
    fn estimates_round_to_five_seconds() {
        assert_eq!(build_estimate_ms(StoreState::Cold, None), None);
        assert_eq!(
            build_estimate_ms(StoreState::Cold, Some(26_500)),
            Some(25_000)
        );
        assert_eq!(
            build_estimate_ms(StoreState::Cold, Some(27_500)),
            Some(30_000)
        );
        assert_eq!(
            build_estimate_ms(StoreState::BaseReusable, Some(26_500)),
            Some(20_000),
            "0.8× of 26.5 s is 21.2 s"
        );
        assert_eq!(
            build_estimate_ms(StoreState::HeadReusable, Some(26_500)),
            Some(20_000)
        );
        assert_eq!(
            build_estimate_ms(StoreState::Cold, Some(900)),
            Some(5_000),
            "never quotes 0 s"
        );
    }

    /// A repo with two commits touching `src/a.py`; returns the shas too.
    fn two_commit_repo() -> (tempfile::TempDir, PathBuf, String, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().to_path_buf();
        git(&repo, &["init", "-q"]).expect("init");
        git(&repo, &["config", "user.email", "depgraph@example.com"]).unwrap();
        git(&repo, &["config", "user.name", "Depgraph"]).unwrap();
        let commit = |body: &str, message: &str| {
            std::fs::create_dir_all(repo.join("src")).unwrap();
            std::fs::write(repo.join("src/a.py"), body).unwrap();
            git(&repo, &["add", "."]).unwrap();
            git(&repo, &["commit", "-qm", message]).unwrap();
            resolve_commit(&repo, "HEAD").unwrap()
        };
        let first = commit("x = 1\n", "first");
        let second = commit("x = 2\n", "second");
        (dir, repo, first, second)
    }

    fn head_of(path: &Path) -> String {
        resolve_commit(path, "HEAD").unwrap()
    }

    #[test]
    fn ensure_worktree_moves_a_stale_checkout_and_drops_its_graph() {
        let (_keep, repo, first, second) = two_commit_repo();
        let tree = repo.join(".git/topos-pr-1/head");
        ensure_worktree(&repo, &tree, &first).unwrap();
        assert_eq!(head_of(&tree), first);
        std::fs::create_dir_all(tree.join(".gitnexus")).unwrap();
        std::fs::write(tree.join("src/stray.py"), "y = 1\n").unwrap();
        std::fs::write(tree.join("src/a.py"), "x = 99\n").unwrap();

        ensure_worktree(&repo, &tree, &second).unwrap();

        assert_eq!(head_of(&tree), second);
        assert_eq!(
            std::fs::read_to_string(tree.join("src/a.py")).unwrap(),
            "x = 2\n"
        );
        assert!(!tree.join("src/stray.py").exists());
        assert!(!tree.join(".gitnexus").exists());
    }

    #[test]
    fn ensure_worktree_keeps_the_graph_of_a_current_checkout() {
        let (_keep, repo, first, _) = two_commit_repo();
        let tree = repo.join(".git/topos-pr-1/base");
        ensure_worktree(&repo, &tree, &first).unwrap();
        std::fs::create_dir_all(tree.join(".gitnexus")).unwrap();

        ensure_worktree(&repo, &tree, &first).unwrap();

        assert_eq!(head_of(&tree), first);
        assert!(tree.join(".gitnexus").exists());
    }

    #[test]
    fn ensure_worktree_recovers_a_deleted_worktree_directory() {
        let (_keep, repo, first, second) = two_commit_repo();
        let tree = repo.join(".git/topos-pr-1/head");
        ensure_worktree(&repo, &tree, &first).unwrap();
        std::fs::remove_dir_all(&tree).unwrap();

        ensure_worktree(&repo, &tree, &second).unwrap();

        assert_eq!(head_of(&tree), second);
    }

    #[test]
    fn ensure_worktree_replaces_a_directory_git_does_not_know() {
        let (_keep, repo, first, _) = two_commit_repo();
        let tree = repo.join(".git/topos-pr-1/base");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join("leftover.py"), "z = 1\n").unwrap();

        ensure_worktree(&repo, &tree, &first).unwrap();

        assert_eq!(head_of(&tree), first);
        assert!(!tree.join("leftover.py").exists());
    }

    #[test]
    fn prepare_pr_stores_forgets_old_commits_before_rebuilding() {
        let (_keep, repo, first, second) = two_commit_repo();
        let parent = repo.join(".git/topos-pr-1");
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::write(parent.join("commits"), format!("{first}\n{first}\n")).unwrap();

        // An unresolvable head fails before any graph is built; the stale
        // record must already be gone so no later run trusts it.
        let err = prepare_pr_stores(&repo, 1, &second, "0000000", &|| {}).err();

        assert!(err.is_some_and(|e| e.contains("could not resolve")));
        assert!(!parent.join("commits").exists());
    }

    /// Run from a linked worktree, whose `.git` is a file, the stores land
    /// in the main repository's `.git` directory and check out there.
    #[test]
    fn a_linked_worktree_keeps_its_stores_in_the_common_dir() {
        let (_keep, repo, first, _) = two_commit_repo();
        let elsewhere = tempfile::tempdir().expect("tempdir");
        let linked = elsewhere.path().join("linked");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                &linked.display().to_string(),
            ],
        )
        .unwrap();

        let stores = PrStores::locate(&linked, 1).unwrap();
        let canonical = |path: &Path| std::fs::canonicalize(path).unwrap();
        assert_eq!(
            canonical(stores.parent.parent().unwrap()),
            canonical(&repo.join(".git"))
        );
        ensure_worktree(&linked, &stores.base, &first).unwrap();
        assert_eq!(head_of(&stores.base), first);
    }
}
