//! The `.topos.toml` a range is judged by.
//!
//! A change cannot be trusted to bring its own rules: a waiver, a looser
//! gate, `fail_on = "never"` or an allowlist entry added in the range
//! would clear the very regression it arrives with. So the policy, the
//! waivers and the allowlist are read at the base revision, and whatever
//! the range itself does to the file is listed as not applied.

use std::path::{Path, PathBuf};

use topos_engine::adapters::discovery::{
    is_ignored_by_patterns, parse_ignore_patterns, text_has_generated_header,
};
use topos_engine::config::{parse_topos_config, ToposConfig};

use topos_engine::graphs::ast::languages::language_for_path;

use super::git::{file_change, show_file, skip_reason, DiffEntry};
use super::model::{FileChange, SkipKind, SkippedFile};
use super::{priority_name, resolve_priority};

const CONFIG_FILE: &str = ".topos.toml";
const IGNORE_FILE: &str = ".toposignore";

/// The head that means "the files on disk", as `resolve_range` spells it.
pub const WORKTREE: &str = ":worktree";

/// The project config for one run, and what the range did to it.
pub struct BasePolicy {
    /// The config at the base revision; the defaults when it has none.
    pub topos: ToposConfig,
    /// The file `topos` was read from, when the base revision has one.
    pub source: Option<PathBuf>,
    /// The `.toposignore` patterns at the base revision.
    pub ignore: Vec<String>,
    /// Every way the head's config and `.toposignore` differ from the ones
    /// at the fork point: edits this range makes, none of which judged it.
    pub unapplied: Vec<String>,
}

/// Read the config that judges `fork..head` from `base`, the revision the
/// change merges into. `fork` is their merge-base, so an edit that landed
/// on the base branch after the fork is not reported as this change's.
/// `head` is a revision, or `:worktree` for the files on disk.
pub fn base_policy(root: &Path, base: &str, fork: &str, head: &str) -> BasePolicy {
    let at = |rev: &str, file: &str| {
        if rev == WORKTREE {
            std::fs::read_to_string(root.join(file)).ok()
        } else {
            show_file(root, rev, file).ok()
        }
    };
    let parse = |text: Option<String>| {
        parse_topos_config(&text.unwrap_or_default(), Some(root.to_path_buf()))
    };
    let ignore = |rev: &str| parse_ignore_patterns(&at(rev, IGNORE_FILE).unwrap_or_default());
    let judged = at(base, CONFIG_FILE);
    let mut unapplied = edits(&parse(at(fork, CONFIG_FILE)), &parse(at(head, CONFIG_FILE)));
    listed(
        &mut unapplied,
        ".toposignore pattern",
        &ignore(fork),
        &ignore(head),
        String::clone,
    );
    BasePolicy {
        source: judged.is_some().then(|| root.join(CONFIG_FILE)),
        topos: parse(judged),
        ignore: ignore(base),
        unapplied,
    }
}

/// Decides, for each changed file, whether it is scored, and against
/// what. The rules a path alone cannot settle are read at the base
/// revision, so a change cannot bring its own: the head's `.toposignore`
/// is not read, and a generated-code header counts only when the file
/// carries it on both sides.
pub struct Exclusions<'a> {
    pub repo: &'a Path,
    /// The revision the before side is read at.
    pub base: &'a str,
    /// The head revision, or [`WORKTREE`] for the files on disk.
    pub head: &'a str,
    /// The base revision's `.toposignore` patterns.
    pub ignore: &'a [String],
}

impl Exclusions<'_> {
    /// The entry to score, or the record of why it is skipped.
    ///
    /// An existing file whose base side was never reviewed (it sat at a
    /// skipped path, or was generated code and has since lost its header)
    /// comes back as an added file: unreviewed content is no baseline, so
    /// it must clear the new-file gates on its own.
    pub fn review(&self, entry: DiffEntry) -> Result<DiffEntry, SkippedFile> {
        if let Some(reason) = self.path_skip(&entry.path) {
            return Err(skipped(entry.path, reason));
        }
        if file_change(&entry.status) == FileChange::Added {
            return Ok(entry);
        }
        let base_path = entry.old_path.as_deref().unwrap_or(&entry.path);
        let generated = self.generated(self.base, base_path);
        if generated && self.generated(self.head, &entry.path) {
            return Err(skipped(
                entry.path,
                "generated-code header at base and head".to_string(),
            ));
        }
        let moved_from_skipped = entry
            .old_path
            .as_deref()
            .is_some_and(|old| self.path_skip(old).is_some());
        Ok(if generated || moved_from_skipped {
            DiffEntry {
                status: "A".to_string(),
                old_path: None,
                ..entry
            }
        } else {
            entry
        })
    }

    /// Why a file at `path` is not scored, by its path: the recap's skip
    /// rules, then the base `.toposignore`. Reads no file contents, so it
    /// is cheap enough for an estimate; [`Exclusions::review`] adds the
    /// generated-header check that decides what is actually scored.
    pub fn path_skip(&self, path: &str) -> Option<String> {
        let at_path = DiffEntry {
            status: "M".to_string(),
            path: path.to_string(),
            old_path: None,
        };
        skip_reason(&at_path).or_else(|| {
            is_ignored_by_patterns(path, self.ignore)
                .then(|| "ignored by .toposignore at base".to_string())
        })
    }

    fn generated(&self, rev: &str, path: &str) -> bool {
        let text = if rev == WORKTREE {
            std::fs::read_to_string(self.repo.join(path)).ok()
        } else {
            show_file(self.repo, rev, path).ok()
        };
        text.is_some_and(|text| text_has_generated_header(&text))
    }
}

/// Extensions of source code in languages Topos does not score. A changed
/// file with one of these is code nobody reviewed, not a doc or a data
/// file, and is reported as such.
const UNSCORED_SOURCE_EXTENSIONS: &[&str] = &[
    "jsx", "mts", "cts", "pyi", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "java", "kt", "kts",
    "swift", "rb", "php", "cs", "scala", "m", "mm", "sh", "bash", "zsh", "vue", "svelte", "lua",
    "pl", "dart", "ex", "exs", "erl", "hs", "clj", "groovy", "sql",
];

const LANGUAGE_NOT_SCORED: &str = "source file in a language Topos does not score";

fn skipped(path: String, reason: String) -> SkippedFile {
    let scored_language = language_for_path(&path).is_some();
    let unscored_language = !scored_language
        && Path::new(&path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                UNSCORED_SOURCE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
            });
    let (kind, reason) = match (scored_language, unscored_language) {
        (true, _) => (SkipKind::PathOrPolicy, reason),
        // A vendored file in an unscored language is skipped for its path.
        (false, true) if reason == UNSUPPORTED_FILE => {
            (SkipKind::LanguageNotScored, LANGUAGE_NOT_SCORED.to_string())
        }
        (false, true) => (SkipKind::PathOrPolicy, reason),
        (false, false) => (SkipKind::NotSource, reason),
    };
    SkippedFile {
        source: kind != SkipKind::NotSource,
        kind,
        lines_changed: 0,
        path,
        reason,
    }
}

/// What `skip_reason` says about a file in no supported language.
const UNSUPPORTED_FILE: &str = "not a supported source file";

/// What `head` changes about how a range is judged, one phrase each.
fn edits(fork: &ToposConfig, head: &ToposConfig) -> Vec<String> {
    let mut out = Vec::new();
    let settings = fork.pr_recap.settings();
    for (was, now) in settings.iter().zip(head.pr_recap.settings()) {
        if was.value != now.value {
            out.push(format!("{}: {} → {}", now.path(), was.value, now.value));
        }
    }
    listed(
        &mut out,
        "waiver",
        &fork.pr_recap.waivers,
        &head.pr_recap.waivers,
        |waiver| format!("{} on {}", waiver.gate(), waiver.path()),
    );
    listed(
        &mut out,
        "allowlist entry",
        &fork.allow,
        &head.allow,
        |entry| format!("{} in {}", entry.pattern, entry.scope),
    );
    let priority = |config| resolve_priority(None, config).map(priority_name);
    if let (Ok(was), Ok(now)) = (priority(fork), priority(head)) {
        if was != now {
            out.push(format!("priority: {was} → {now}"));
        }
    }
    out
}

fn listed<T: PartialEq>(
    out: &mut Vec<String>,
    kind: &str,
    was: &[T],
    now: &[T],
    name: impl Fn(&T) -> String,
) {
    for item in now.iter().filter(|item| !was.contains(item)) {
        out.push(format!("{kind} added: {}", name(item)));
    }
    for item in was.iter().filter(|item| !now.contains(item)) {
        out.push(format!("{kind} removed: {}", name(item)));
    }
}

#[cfg(test)]
mod tests {
    use topos_engine::config::{FailOn, GateId, Severity};

    use super::super::tests::{commit_all, write_files, write_repo};
    use super::*;

    const STRICT: &str = "[pr_recap]\npreset = \"strict\"\n";
    const LOOSENED: &str = "[evaluation]\npriority = \"simple\"\n\n[pr_recap]\nfail_on = \"never\"\n\n[pr_recap.gates]\npillar_lost = \"off\"\n\n[[pr_recap.waive]]\ngate = \"new_file_slop\"\npath = \"src/**\"\nreason = \"trust me\"\n\n[[secure.allow]]\npattern = \"os.system\"\nreason = \"trust me\"\n";

    #[test]
    fn the_base_revision_judges_and_the_ranges_own_edits_are_listed() {
        let (_keep, repo) = write_repo(&[(".topos.toml", STRICT)]);
        write_files(&repo, &[(".topos.toml", LOOSENED)]);
        commit_all(&repo, "loosen");

        let policy = base_policy(&repo, "HEAD~1", "HEAD~1", "HEAD");
        assert_eq!(policy.topos.pr_recap.fail_on, FailOn::Warn);
        assert_eq!(
            policy.topos.pr_recap.severity(GateId::PillarLost),
            Severity::Block
        );
        assert!(policy.topos.pr_recap.waivers.is_empty());
        assert!(policy.topos.allow.is_empty());
        assert_eq!(policy.source, Some(repo.join(".topos.toml")));
        for edit in [
            "fail_on: warn → never",
            "gates.pillar_lost: block → off",
            "waiver added: new_file_slop on src/**",
            "allowlist entry added: os.system in **",
            "priority: secure → simple",
        ] {
            assert!(
                policy.unapplied.iter().any(|found| found == edit),
                "missing {edit}: {:?}",
                policy.unapplied
            );
        }
    }

    #[test]
    fn the_base_toposignore_applies_and_the_ranges_edits_to_it_are_listed() {
        let (_keep, repo) = write_repo(&[(".toposignore", "samples/\n")]);
        write_files(&repo, &[(".toposignore", "src/\n")]);
        commit_all(&repo, "hide src");

        let policy = base_policy(&repo, "HEAD~1", "HEAD~1", "HEAD");
        assert_eq!(policy.ignore, ["samples"]);
        assert_eq!(
            policy.unapplied,
            [
                ".toposignore pattern added: src",
                ".toposignore pattern removed: samples"
            ]
        );

        let entry = |path: &str| DiffEntry {
            status: "A".to_string(),
            path: path.to_string(),
            old_path: None,
        };
        let exclusions = Exclusions {
            repo: &repo,
            base: "HEAD~1",
            head: "HEAD",
            ignore: &policy.ignore,
        };
        let skipped = exclusions
            .review(entry("samples/a.py"))
            .err()
            .expect("skipped");
        assert_eq!(skipped.reason, "ignored by .toposignore at base");
        assert!(skipped.source, "a Python file is source");
        assert!(exclusions.review(entry("src/a.py")).is_ok());
        let docs = exclusions
            .review(entry("docs/a.md"))
            .err()
            .expect("skipped");
        assert!(!docs.source, "Markdown is not source");
    }

    #[test]
    fn the_recap_skips_every_path_discovery_excludes() {
        for path in [
            "tests/fixtures/broken.py",
            "pkg/testdata/broken.go",
            "ui/__snapshots__/card.js",
            "htmlcov/index.py",
            ".tox/lib.py",
        ] {
            let entry = DiffEntry {
                status: "A".to_string(),
                path: path.to_string(),
                old_path: None,
            };
            assert_eq!(
                skip_reason(&entry).as_deref(),
                Some("generated or vendor path"),
                "{path}"
            );
        }
    }

    #[test]
    fn uncommitted_edits_do_not_judge_the_worktree() {
        let (_keep, repo) = write_repo(&[(".topos.toml", STRICT)]);
        write_files(
            &repo,
            &[(".topos.toml", "[pr_recap]\nfail_on = \"never\"\n")],
        );

        let policy = base_policy(&repo, "HEAD", "HEAD", ":worktree");
        assert_eq!(policy.topos.pr_recap.fail_on, FailOn::Warn);
        assert!(
            policy
                .unapplied
                .contains(&"fail_on: warn → never".to_string()),
            "{:?}",
            policy.unapplied
        );
    }

    #[test]
    fn a_base_without_a_config_is_judged_by_the_defaults_not_the_heads_file() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        write_files(&repo, &[(".topos.toml", LOOSENED)]);
        commit_all(&repo, "bring a config");

        let policy = base_policy(&repo, "HEAD~1", "HEAD~1", "HEAD");
        assert_eq!(policy.topos.pr_recap, Default::default());
        assert!(policy.topos.allow.is_empty());
        assert_eq!(policy.source, None);
        assert!(!policy.unapplied.is_empty());
    }

    #[test]
    fn a_config_the_range_leaves_alone_reports_nothing() {
        let (_keep, repo) = write_repo(&[(".topos.toml", LOOSENED), ("a.py", "x = 1\n")]);
        write_files(&repo, &[("a.py", "x = 2\n")]);
        commit_all(&repo, "edit code");

        let policy = base_policy(&repo, "HEAD~1", "HEAD~1", "HEAD");
        assert_eq!(policy.topos.pr_recap.fail_on, FailOn::Never);
        assert_eq!(policy.unapplied, Vec::<String>::new());
    }

    #[test]
    fn an_edit_that_landed_on_the_base_branch_is_applied_and_not_charged_to_the_change() {
        let (_keep, repo) = write_repo(&[("a.py", "x = 1\n")]);
        let fork = super::super::gh::git(&repo, &["rev-parse", "HEAD"]).unwrap();
        let fork = fork.trim();
        write_files(&repo, &[(".topos.toml", STRICT)]);
        commit_all(&repo, "the base branch tightens");

        let policy = base_policy(&repo, "HEAD", fork, fork);
        assert_eq!(policy.topos.pr_recap.fail_on, FailOn::Warn);
        assert_eq!(policy.unapplied, Vec::<String>::new());
    }
}
