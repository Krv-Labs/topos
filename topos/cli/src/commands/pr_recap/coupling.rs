//! Whether this run has coupling graphs, and whether to build them.
//!
//! COMPOSABLE and split tracing read GitNexus graphs of the base and the
//! head. Building them is the slow step of a recap (about 20 s a side, the
//! two in parallel), so it is settled in three steps, all before the
//! spinner starts: [`plan_coupling`] reads what is already built without
//! running anything, [`settle`] asks on a terminal when a build is needed,
//! and [`prepare`] builds under the spinner. A run that cannot ask takes
//! the default, which is to build, so a required check never passes or
//! fails on whether a terminal was attached or a cache survived.

use std::path::Path;

use console::Style;

use super::model::{CouplingReason, CouplingStatus};
use super::view::{seconds, short_rev};
use super::PrRecapArgs;
use crate::commands::depgraph::{
    build_estimate_ms, last_build_ms, pr_store_state, prepare_pr_stores, PrStores, StoreState,
};
use crate::commands::gh::resolve_commit;
use crate::commands::interaction::{ask, Asker, Interaction, Question};
use crate::commands::render::{paint, RenderOptions};

/// What this run can do about coupling graphs, before anything is built.
pub(super) enum CouplingPlan {
    /// No graphs this run, and why.
    Unavailable(CouplingStatus),
    /// Both graphs are built at these commits.
    Ready(PrStores),
    /// The graphs need building.
    NeedsBuild {
        pr: u64,
        base_sha: String,
        head_sha: String,
        state: StoreState,
        estimate_ms: Option<u64>,
    },
}

/// A status with no graphs behind it.
pub(super) fn unmeasured(reason: CouplingReason, note: impl Into<String>) -> CouplingStatus {
    CouplingStatus {
        measured: false,
        note: note.into(),
        reason,
        estimate_ms: None,
    }
}

/// Read what is already built for `base`/`head`. Fast: plain file reads and
/// two `git rev-parse` calls, never gitnexus. `gitnexus_available` is
/// passed in so tests do not depend on `PATH`. `--no-coupling` wins over
/// everything, `--yes` included; graphs already built are used without
/// gitnexus installed.
pub(super) fn plan_coupling(
    root: &Path,
    base: &str,
    head: &str,
    args: &PrRecapArgs,
    gitnexus_available: bool,
) -> CouplingPlan {
    if args.no_coupling {
        return CouplingPlan::Unavailable(unmeasured(CouplingReason::Flag, "--no-coupling"));
    }
    let Some(pr) = args.pr else {
        return CouplingPlan::Unavailable(unmeasured(
            CouplingReason::NoPr,
            "pass a pull request number to measure COMPOSABLE",
        ));
    };
    let (Ok(base_sha), Ok(head_sha)) = (resolve_commit(root, base), resolve_commit(root, head))
    else {
        return CouplingPlan::Unavailable(unmeasured(
            CouplingReason::Error,
            format!("could not resolve {base}...{head}"),
        ));
    };
    let stores = match PrStores::locate(root, pr) {
        Ok(stores) => stores,
        Err(error) => return CouplingPlan::Unavailable(unmeasured(CouplingReason::Error, error)),
    };
    let state = pr_store_state(&stores, &base_sha, &head_sha);
    if state == StoreState::Ready {
        return CouplingPlan::Ready(stores);
    }
    if !gitnexus_available {
        return CouplingPlan::Unavailable(unmeasured(
            CouplingReason::GitnexusMissing,
            "gitnexus not installed (npm install -g gitnexus)",
        ));
    }
    let history = stores.parent.parent().and_then(last_build_ms);
    CouplingPlan::NeedsBuild {
        pr,
        base_sha,
        head_sha,
        state,
        estimate_ms: build_estimate_ms(state, history),
    }
}

/// Decide a [`CouplingPlan::NeedsBuild`]: ask when `interaction` allows,
/// else take `--yes` or the default (build). A declined build becomes
/// [`CouplingPlan::Unavailable`]. Every other plan passes through without
/// a question. Ctrl-C at the prompt comes back as an error. Must run
/// before the spinner starts: the prompt and the spinner share stderr.
pub(super) fn settle(
    plan: CouplingPlan,
    interaction: Interaction,
    asker: &mut dyn Asker,
) -> Result<CouplingPlan, String> {
    let CouplingPlan::NeedsBuild {
        pr,
        ref base_sha,
        ref head_sha,
        state,
        estimate_ms,
    } = plan
    else {
        return Ok(plan);
    };
    let build = ask(
        interaction,
        asker,
        &question(pr, base_sha, head_sha, state, estimate_ms),
    )?;
    if interaction == Interaction::Prompt {
        let status = if build {
            "◇  Building coupling graphs (base and head in parallel)"
        } else {
            "◇  Skipping COMPOSABLE"
        };
        eprintln!(
            "{}",
            paint(status, Style::new().bold(), RenderOptions::stderr())
        );
    }
    if build {
        return Ok(plan);
    }
    let reason = if interaction == Interaction::Prompt {
        CouplingReason::Declined
    } else {
        CouplingReason::NotAsked
    };
    Ok(CouplingPlan::Unavailable(CouplingStatus {
        estimate_ms,
        ..unmeasured(reason, "graphs not built")
    }))
}

/// Carry out a settled plan: build the graphs a [`CouplingPlan::NeedsBuild`]
/// still wants, and say where the graphs came from or why there are none.
pub(super) fn prepare(root: &Path, plan: CouplingPlan) -> (Option<PrStores>, CouplingStatus) {
    match plan {
        CouplingPlan::Unavailable(status) => (None, status),
        CouplingPlan::Ready(stores) => {
            let status = CouplingStatus {
                measured: true,
                note: format!("reused from {}", stores.parent.display()),
                reason: CouplingReason::Cached,
                estimate_ms: None,
            };
            (Some(stores), status)
        }
        CouplingPlan::NeedsBuild {
            pr,
            base_sha,
            head_sha,
            ..
        } => match prepare_pr_stores(root, pr, &base_sha, &head_sha) {
            Ok(stores) => {
                let status = CouplingStatus {
                    measured: true,
                    note: format!("built from {}", stores.parent.display()),
                    reason: CouplingReason::Built,
                    estimate_ms: None,
                };
                (Some(stores), status)
            }
            Err(error) => (None, unmeasured(CouplingReason::Error, error)),
        },
    }
}

/// The build question, with what each graph costs from `state`.
fn question(
    pr: u64,
    base_sha: &str,
    head_sha: &str,
    state: StoreState,
    estimate_ms: Option<u64>,
) -> Question {
    let (base, head) = (short_rev(base_sha), short_rev(head_sha));
    let wait = estimate_ms.map_or_else(
        || "usually 10–60 s".to_string(),
        |ms| format!("about {}", seconds(ms)),
    );
    let cost = match state {
        StoreState::BaseReusable => {
            format!("Base {base} is already built; head {head} is new: {wait}")
        }
        StoreState::HeadReusable => {
            format!("Head {head} is already built; base {base} is new: {wait}")
        }
        StoreState::Cold | StoreState::Ready => {
            format!("Neither is built yet: {wait} now, reused on the next run")
        }
    };
    Question {
        title: format!("Build coupling graphs for #{pr}?"),
        plan: vec![
            format!(
                "COMPOSABLE and split tracing read GitNexus graphs of base {base} and head {head}"
            ),
            cost,
            "--yes or --no-coupling answers this ahead of time".to_string(),
        ],
        yes: "Yes, build them",
        no: "No, report COMPOSABLE as not measured",
        default: true,
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::commands::depgraph::write_commits;
    use crate::commands::pr_recap::tests::{commit_all, write_files, write_repo};

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: PrRecapArgs,
    }

    fn args(argv: &[&str]) -> PrRecapArgs {
        Cli::try_parse_from(std::iter::once("pr-recap").chain(argv.iter().copied()))
            .expect("valid flags")
            .args
    }

    /// A repo with two commits; returns their shas, base first.
    fn repo() -> (tempfile::TempDir, std::path::PathBuf, String, String) {
        let (keep, repo) = write_repo(&[("src/a.py", "x = 1\n")]);
        write_files(&repo, &[("src/a.py", "x = 2\n")]);
        commit_all(&repo, "edit");
        let base = resolve_commit(&repo, "HEAD~1").unwrap();
        let head = resolve_commit(&repo, "HEAD").unwrap();
        (keep, repo, base, head)
    }

    fn reason(plan: &CouplingPlan) -> Option<CouplingReason> {
        match plan {
            CouplingPlan::Unavailable(status) => Some(status.reason),
            _ => None,
        }
    }

    fn built_stores(repo: &Path, pr: u64, base: &str, head: &str) {
        let stores = PrStores::locate(repo, pr).unwrap();
        for side in [&stores.base, &stores.head] {
            std::fs::create_dir_all(side.join(".gitnexus")).unwrap();
        }
        write_commits(&stores, base, head, 26_500).unwrap();
    }

    #[test]
    fn yes_conflicts_with_no_input() {
        assert!(Cli::try_parse_from(["pr-recap", "--yes", "--no-input"]).is_err());
        assert!(args(&["-y"]).yes);
        assert!(args(&["--no-input"]).no_input);
    }

    #[test]
    fn no_pr_and_the_flag_never_plan_a_build() {
        let (_keep, repo, base, head) = repo();
        let plan = plan_coupling(&repo, &base, &head, &args(&[]), true);
        assert_eq!(reason(&plan), Some(CouplingReason::NoPr));
        for argv in [
            &["5", "--no-coupling"][..],
            &["5", "--no-coupling", "--yes"],
        ] {
            let plan = plan_coupling(&repo, &base, &head, &args(argv), true);
            assert_eq!(reason(&plan), Some(CouplingReason::Flag), "{argv:?}");
        }
    }

    #[test]
    fn a_missing_gitnexus_only_matters_when_something_needs_building() {
        let (_keep, repo, base, head) = repo();
        let plan = plan_coupling(&repo, &base, &head, &args(&["5"]), false);
        assert_eq!(reason(&plan), Some(CouplingReason::GitnexusMissing));

        built_stores(&repo, 5, &base, &head);
        let plan = plan_coupling(&repo, &base, &head, &args(&["5"]), false);
        assert!(
            matches!(plan, CouplingPlan::Ready(_)),
            "built graphs load without gitnexus"
        );
        let plan = plan_coupling(&repo, &base, &head, &args(&["5"]), true);
        assert!(matches!(plan, CouplingPlan::Ready(_)));
    }

    #[test]
    fn a_cold_store_needs_a_build_with_an_estimate_from_history() {
        let (_keep, repo, base, head) = repo();
        let plan = plan_coupling(&repo, &base, &head, &args(&["5"]), true);
        let CouplingPlan::NeedsBuild {
            pr,
            state,
            estimate_ms,
            ..
        } = plan
        else {
            panic!("expected a build");
        };
        assert_eq!((pr, state, estimate_ms), (5, StoreState::Cold, None));

        // Another pull request's last build took 26.5 s.
        built_stores(&repo, 9, "b", "h");
        let plan = plan_coupling(&repo, &base, &head, &args(&["5"]), true);
        let CouplingPlan::NeedsBuild { estimate_ms, .. } = plan else {
            panic!("expected a build");
        };
        assert_eq!(estimate_ms, Some(25_000));

        // PR 5 already has this base built: one side is reused.
        built_stores(&repo, 5, &base, "0000000");
        let plan = plan_coupling(&repo, &base, &head, &args(&["5"]), true);
        let CouplingPlan::NeedsBuild {
            state, estimate_ms, ..
        } = plan
        else {
            panic!("expected a build");
        };
        assert_eq!(state, StoreState::BaseReusable);
        assert_eq!(estimate_ms, Some(20_000));
    }

    /// Always answers `answer`; `None` stands for a skip.
    struct Fixed(Option<bool>);

    impl Asker for Fixed {
        fn confirm(&mut self, _: &Question) -> Result<Option<bool>, String> {
            Ok(self.0)
        }
    }

    struct Interrupted;

    impl Asker for Interrupted {
        fn confirm(&mut self, _: &Question) -> Result<Option<bool>, String> {
            Err("interrupted at the prompt".to_string())
        }
    }

    fn needs_build() -> CouplingPlan {
        CouplingPlan::NeedsBuild {
            pr: 362,
            base_sha: "add9761".repeat(2),
            head_sha: "3812be2".repeat(2),
            state: StoreState::Cold,
            estimate_ms: Some(25_000),
        }
    }

    #[test]
    fn settling_asks_only_on_a_prompt() {
        let declined = settle(needs_build(), Interaction::Prompt, &mut Fixed(Some(false))).unwrap();
        let CouplingPlan::Unavailable(status) = declined else {
            panic!("a no builds nothing");
        };
        assert_eq!(status.reason, CouplingReason::Declined);
        assert_eq!(status.estimate_ms, Some(25_000));

        let skipped = settle(needs_build(), Interaction::Prompt, &mut Fixed(None)).unwrap();
        assert_eq!(reason(&skipped), Some(CouplingReason::Declined));

        for (interaction, answer) in [
            (Interaction::Prompt, Some(true)),
            (Interaction::AssumeYes, Some(false)),
            (Interaction::Defaults, Some(false)),
        ] {
            let plan = settle(needs_build(), interaction, &mut Fixed(answer)).unwrap();
            assert!(
                matches!(plan, CouplingPlan::NeedsBuild { .. }),
                "{interaction:?} builds"
            );
        }

        assert!(settle(needs_build(), Interaction::Prompt, &mut Interrupted).is_err());
        // Nothing to decide: no question, even on a prompt.
        let flag = CouplingPlan::Unavailable(unmeasured(CouplingReason::Flag, "--no-coupling"));
        let plan = settle(flag, Interaction::Prompt, &mut Interrupted).unwrap();
        assert_eq!(reason(&plan), Some(CouplingReason::Flag));
    }

    #[test]
    fn the_question_names_the_commits_and_the_wait() {
        let cold = question(
            362,
            "add9761aaaa",
            "3812be2bbbb",
            StoreState::Cold,
            Some(25_000),
        );
        assert_eq!(cold.title, "Build coupling graphs for #362?");
        assert_eq!(
            cold.plan,
            [
                "COMPOSABLE and split tracing read GitNexus graphs of base add9761 and head 3812be2",
                "Neither is built yet: about 25 s now, reused on the next run",
                "--yes or --no-coupling answers this ahead of time",
            ]
        );
        assert!(cold.default, "a run that cannot ask builds");
        let half = question(
            362,
            "add9761aaaa",
            "3812be2bbbb",
            StoreState::BaseReusable,
            Some(20_000),
        );
        assert_eq!(
            half.plan[1],
            "Base add9761 is already built; head 3812be2 is new: about 20 s"
        );
        let fresh = question(362, "add9761aaaa", "3812be2bbbb", StoreState::Cold, None);
        assert!(
            fresh.plan[1].contains("usually 10–60 s"),
            "{}",
            fresh.plan[1]
        );
    }
}
