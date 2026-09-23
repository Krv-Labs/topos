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
use std::time::SystemTime;

use topos_engine::adapters::gitnexus::GITNEXUS_FINGERPRINT_FILE;

use crate::commands::gh::git_common_dir;

/// Paths to the worktrees (and their shared parent) backing a PR's coupling
/// graphs.
pub(crate) struct PrStores {
    pub(crate) parent: PathBuf,
    pub(crate) base: PathBuf,
    pub(crate) head: PathBuf,
}

impl PrStores {
    /// Pull request `pr`'s stores for the repository at `repo_root`, in the
    /// `.git` directory its worktrees share.
    pub(crate) fn locate(repo_root: &Path, pr: u64) -> Result<Self, String> {
        Ok(Self::at(
            git_common_dir(repo_root)?.join(format!("topos-pr-{pr}")),
        ))
    }

    pub(crate) fn at(parent: PathBuf) -> Self {
        Self {
            base: parent.join("base"),
            head: parent.join("head"),
            parent,
        }
    }

    pub(crate) fn commits_path(&self) -> PathBuf {
        self.parent.join("commits")
    }
}

/// How much of a build the stores already hold for one base/head pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoreState {
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
    pub(crate) fn one_side_reusable(self) -> bool {
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
pub(crate) fn write_commits(
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
pub(crate) fn pr_store_state(stores: &PrStores, base_sha: &str, head_sha: &str) -> StoreState {
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
pub(crate) fn last_build_ms(store_root: &Path) -> Option<u64> {
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
pub(crate) fn build_estimate_ms(state: StoreState, last_build_ms: Option<u64>) -> Option<u64> {
    let ms = last_build_ms?;
    let ms = if state.one_side_reusable() {
        ms * 4 / 5
    } else {
        ms
    };
    Some(((ms + 2_500) / 5_000).max(1) * 5_000)
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
}
