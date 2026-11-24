//! The `.topos.toml` a range is judged by.
//!
//! A change cannot be trusted to bring its own rules: a waiver, a looser
//! gate, `fail_on = "never"` or an allowlist entry added in the range
//! would clear the very regression it arrives with. So the policy, the
//! waivers and the allowlist are read at the base revision, and whatever
//! the range itself does to the file is listed as not applied.

use std::path::{Path, PathBuf};

use topos_engine::config::{parse_topos_config, ToposConfig};

use super::git::show_file;
use super::{priority_name, resolve_priority};

const CONFIG_FILE: &str = ".topos.toml";

/// The project config for one run, and what the range did to it.
pub struct BasePolicy {
    /// The config at the base revision; the defaults when it has none.
    pub topos: ToposConfig,
    /// The file `topos` was read from, when the base revision has one.
    pub source: Option<PathBuf>,
    /// Every way the head's config differs from the one at the fork
    /// point: edits this range makes, none of which judged it.
    pub unapplied: Vec<String>,
}

/// Read the config that judges `fork..head` from `base`, the revision the
/// change merges into. `fork` is their merge-base, so an edit that landed
/// on the base branch after the fork is not reported as this change's.
/// `head` is a revision, or `:worktree` for the files on disk.
pub fn base_policy(root: &Path, base: &str, fork: &str, head: &str) -> BasePolicy {
    let at = |rev: &str| show_file(root, rev, CONFIG_FILE).ok();
    let parse =
        |text: Option<&str>| parse_topos_config(text.unwrap_or_default(), Some(root.to_path_buf()));
    let judged = at(base);
    let edited = if head == ":worktree" {
        std::fs::read_to_string(root.join(CONFIG_FILE)).ok()
    } else {
        at(head)
    };
    BasePolicy {
        topos: parse(judged.as_deref()),
        source: judged.is_some().then(|| root.join(CONFIG_FILE)),
        unapplied: edits(&parse(at(fork).as_deref()), &parse(edited.as_deref())),
    }
}

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
