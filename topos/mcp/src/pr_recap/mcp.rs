//! The `topos_pr_recap` tool body: run the recap, and answer with the
//! summary (`topos.pr_recap.summary.v1`) or, for `detail: "full"`, the
//! `topos.pr_recap.v3` document the CLI's `--json` prints.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::CallToolResult;
use serde_json::{json, Value};
use topos_engine::config::{find_config_file, load_topos_config};
use topos_engine::evaluation::waivers::today_utc;

use super::gh::{git_root, merge_base};
use super::progress::{Progress, Watch};
use super::summary::{self, not_found_block, resolve_paths, NotFound, SUMMARY_SCHEMA};
use super::{
    build_recap, head_commit, parse_preset, plan_coupling, prepare_with_cancel,
    resolve_gate_policy, resolve_priority, resolve_range, Judging, PrRecap,
};
use crate::execution::TimeoutNote;
use crate::formatting::to_tool_result;
use crate::schemas::{Detail, PrRecapInput};
use crate::security::{composable_default_root, resolve_file_root, resolve_project_path};
use topos_engine::adapters::gitnexus::gitnexus_available;

/// What the MCP tool calls the scoring cap in the hints that name it.
const MCP_CAP_NAME: &str = "max_files";

/// How long a recap may run before the tool gives up on it.
const DEADLINE: Duration = Duration::from_secs(300);

/// Overrides [`DEADLINE`], in milliseconds. Meant for tests of the timeout
/// error, which would otherwise take five minutes to reach.
const DEADLINE_ENV: &str = "TOPOS_PR_RECAP_TIMEOUT_MS";

/// The deadline for one recap.
pub fn deadline() -> Duration {
    std::env::var(DEADLINE_ENV)
        .ok()
        .and_then(|millis| millis.parse().ok())
        .map_or(DEADLINE, Duration::from_millis)
}

/// What a timeout error adds about the parameters that change the work.
const TIMEOUT_HINT: &str = "`max_files` caps the files scored, highest churn first; `paths` \
                            selects which files' full records are returned.";

/// The note the executor appends to a timeout error: the phase the run was
/// in when the deadline passed.
pub fn timeout_note(progress: &Arc<Progress>) -> TimeoutNote {
    let progress = Arc::clone(progress);
    Arc::new(move || format!("{}. {TIMEOUT_HINT}", progress.describe()))
}

/// Run the recap for `params` and shape the answer.
pub fn respond(
    params: PrRecapInput,
    cancel: Option<&AtomicBool>,
    progress: &Progress,
) -> CallToolResult {
    progress.begin();
    let detail = params.detail;
    let paths = params.paths.clone();
    match run_pr_recap(params, cancel, progress) {
        Ok((recap, root)) => {
            progress.finish_up();
            match detail {
                Detail::Summary => summary_result(&recap, &root, &paths, progress),
                Detail::Full => full_result(&recap, &root, &paths),
            }
        }
        Err(error) => error_result(detail, &error),
    }
}

/// [`respond`] without a deadline watching it.
#[cfg(test)]
pub fn respond_untimed(params: PrRecapInput, cancel: Option<&AtomicBool>) -> CallToolResult {
    respond(params, cancel, &Progress::default())
}

fn summary_result(
    recap: &PrRecap,
    root: &Path,
    paths: &[String],
    progress: &Progress,
) -> CallToolResult {
    let summary = summary::build(recap, root, paths, progress.timing());
    to_tool_result(&summary, summary.to_markdown())
}

fn full_result(recap: &PrRecap, root: &Path, paths: &[String]) -> CallToolResult {
    let (value, missing) = full_value(recap, root, paths);
    let mut markdown = format!(
        "**PR Recap ({})**: `{}` ({:?}) — {}\nScored {} file(s) (`{}...{}`).",
        recap.check,
        recap.readiness.word(),
        recap.direction,
        recap.reason,
        recap.scope.files_scored,
        recap.base,
        recap.head,
    );
    if !missing.is_empty() {
        markdown.push('\n');
        markdown.push_str(not_found_block(&missing).trim_end());
    }
    to_tool_result(&value, markdown)
}

/// An error in the shape of the mode asked for: the summary carries its
/// schema, the full document keeps its bare `error`.
fn error_result(detail: Detail, error: &str) -> CallToolResult {
    let body = match detail {
        Detail::Summary => json!({ "schema": SUMMARY_SCHEMA, "error": error }),
        Detail::Full => json!({ "error": error }),
    };
    to_tool_result(&body, format!("**Error:** {error}"))
}

/// The v3 document as the CLI prints it, repo-relative paths and all. With
/// `paths`, its `files` holds just those records and `paths_not_found`
/// lists the ones that matched no scored file.
pub(super) fn full_value(recap: &PrRecap, root: &Path, paths: &[String]) -> (Value, Vec<NotFound>) {
    let mut value = serde_json::to_value(recap).unwrap_or(Value::Null);
    if paths.is_empty() {
        return (value, Vec::new());
    }
    let resolved = resolve_paths(recap, root, paths);
    if let Some(object) = value.as_object_mut() {
        let files: Vec<_> = resolved.found.iter().map(|&i| &recap.files[i]).collect();
        object.insert("files".to_string(), json!(files));
        if !resolved.missing.is_empty() {
            object.insert("paths_not_found".to_string(), json!(resolved.missing));
        }
    }
    (value, resolved.missing)
}

fn run_pr_recap(
    params: PrRecapInput,
    cancel: Option<&AtomicBool>,
    progress: &Progress,
) -> Result<(PrRecap, PathBuf), String> {
    let root = git_root(&project_root(params.directory.as_deref())?)?;
    let judging = judging_for(&params, &root)?;
    let (base, head, review) = resolve_range(&root, params.pr, params.base, params.head)?;
    let base = merge_base(&root, &base, head_commit(&head))?;
    let plan = plan_coupling(
        &root,
        &base,
        &head,
        params.pr,
        params.no_coupling,
        gitnexus_available(),
    );
    let (stores, coupling) = prepare_with_cancel(&root, plan, &|| {}, cancel);
    let mut recap = build_recap(
        &root,
        &base,
        &head,
        params.max_files,
        stores.as_ref(),
        coupling,
        &judging,
        &mut Watch(progress),
    )?;
    recap.review = review;
    Ok((recap, root))
}

fn project_root(directory: Option<&str>) -> Result<PathBuf, String> {
    let Some(directory) = directory else {
        return Ok(composable_default_root(&resolve_file_root()?));
    };
    let (resolved, project) = resolve_project_path(directory)?;
    if !resolved.is_dir() {
        return Err(format!("Path is not a directory: {}", resolved.display()));
    }
    Ok(composable_default_root(&project))
}

fn judging_for(params: &PrRecapInput, root: &Path) -> Result<Judging, String> {
    let preset = params.preset.as_deref().map(parse_preset).transpose()?;
    let topos = load_topos_config(root);
    Ok(Judging {
        priority: resolve_priority(params.priority.as_deref(), &topos)?,
        gate: resolve_gate_policy(params.strict, preset, &topos.pr_recap),
        source: preset.is_none().then(|| find_config_file(root)).flatten(),
        topos,
        today: today_utc(),
        cap_name: MCP_CAP_NAME,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::pr_recap::progress::Phase;
    use crate::pr_recap::summary::tests::{fixture, input};
    use crate::pr_recap::tests::{
        branchy, commit_all, judging, no_coupling, recap, write_files, write_repo,
    };
    use crate::pr_recap::{build_recap, PrRecap};
    use crate::schemas::Detail;
    use topos_engine::evaluation::policies::base::Priority;

    #[test]
    fn the_timeout_note_names_the_phase_and_the_parameters() {
        let progress = Arc::new(Progress::default());
        let note = timeout_note(&progress);
        assert!(note().contains("queued"));
        progress.begin();
        assert_eq!(progress.phase(), Phase::Preparing);
        let text = note();
        assert!(text.contains("preparing"), "{text}");
        assert!(
            text.contains("`max_files`") && text.contains("`paths`"),
            "{text}"
        );
    }

    /// Summary JSON for the 41-file fixture (3 blocks, 38 warnings) stays under
    /// this; the same range's full v3 document is ~74K.
    const SUMMARY_CEILING_CHARS: usize = 4_500;
    const TEXT_CEILING_CHARS: usize = 3_000;

    fn mcp_recap(repo: &Path, max_files: usize) -> PrRecap {
        let mcp_judging = crate::pr_recap::Judging {
            cap_name: "max_files",
            ..judging(Priority::Secure)
        };
        build_recap(
            repo,
            "HEAD~1",
            "HEAD",
            max_files,
            None,
            no_coupling(),
            &mcp_judging,
            &mut (),
        )
        .unwrap()
    }

    #[test]
    fn summary_and_text_stay_under_their_ceilings() {
        let (_keep, repo) = fixture(40, 3);
        let response = respond_untimed(input(&repo, Detail::Summary, &[]), None);
        let structured = response.structured_content.expect("structured");
        let size = serde_json::to_string(&structured).unwrap().len();
        let text = response.content[0].as_text().unwrap().text.clone();
        let full = respond_untimed(input(&repo, Detail::Full, &[]), None);
        let full_size = serde_json::to_string(&full.structured_content.unwrap())
            .unwrap()
            .len();
        eprintln!(
            "summary {size} chars, text {} chars, full {full_size}",
            text.len()
        );
        assert_eq!(structured["schema"], SUMMARY_SCHEMA);
        assert!(size < SUMMARY_CEILING_CHARS, "{size} chars");
        assert!(
            text.len() < TEXT_CEILING_CHARS,
            "{} chars:\n{text}",
            text.len()
        );
        assert!(size * 3 < full_size);
    }

    #[test]
    fn incomplete_hint_names_the_mcp_parameter() {
        let (_keep, repo) = fixture(3, 1);
        let mut params = input(&repo, Detail::Summary, &[]);
        params.max_files = 1;
        let structured = respond_untimed(params, None).structured_content.unwrap();
        assert_eq!(structured["incomplete"], true);
        let reason = structured["reason"].as_str().unwrap();
        assert!(!reason.contains("--max-files"), "{reason}");
        assert!(reason.contains("max_files"), "{reason}");
    }

    #[test]
    fn full_mode_is_the_cli_document_apart_from_the_parameter_name() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        let files = [
            ("src/--max-files.py", branchy("a")),
            ("src/b.py", branchy("b")),
            ("src/c.py", branchy("c")),
        ];
        let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, b)| (*p, b.as_str())).collect();
        write_files(&repo, &borrowed);
        commit_all(&repo, "add");
        // Nothing capped: the two documents are the same value.
        let cli = serde_json::to_value(recap(&repo, "HEAD~1", "HEAD", 0)).unwrap();
        let mcp = serde_json::to_value(mcp_recap(&repo, 0)).unwrap();
        assert_eq!(cli, mcp);
        assert!(cli.to_string().contains("src/--max-files.py"));

        // Capped: only the hints name their own parameter, and a path that
        // contains the CLI flag is left alone.
        let cli = serde_json::to_string(&recap(&repo, "HEAD~1", "HEAD", 1)).unwrap();
        let mcp = serde_json::to_string(&mcp_recap(&repo, 1)).unwrap();
        assert!(cli.contains("raise --max-files") && mcp.contains("raise max_files"));
        let unify = |text: &str, name: &str| {
            text.replace(&format!("(raise {name})"), "(raise CAP)")
                .replace(&format!("over the {name} cap"), "over the CAP cap")
                .replace(&format!("Raise {name} to"), "Raise CAP to")
        };
        assert_eq!(unify(&cli, "--max-files"), unify(&mcp, "max_files"));
        assert!(mcp.contains("src/--max-files.py"));
    }

    #[test]
    fn errors_keep_the_shape_of_the_mode_asked_for() {
        let params = |detail| {
            let mut params = input(Path::new("/no/such/dir"), detail, &[]);
            params.directory = Some("/no/such/dir".to_string());
            params
        };
        let summary = respond_untimed(params(Detail::Summary), None);
        let body = summary.structured_content.unwrap();
        assert_eq!(body["schema"], SUMMARY_SCHEMA);
        assert!(body["error"].as_str().is_some());
        assert_eq!(summary.is_error, Some(true));
        let full = respond_untimed(params(Detail::Full), None);
        let body = full.structured_content.unwrap();
        assert!(body.get("schema").is_none());
        assert!(body["error"].as_str().is_some());
    }

    #[test]
    fn full_detail_text_lists_paths_not_found() {
        let (_keep, repo) = fixture(2, 0);
        let response = respond_untimed(input(&repo, Detail::Full, &["nope.py"]), None);
        let text = &response.content[0].as_text().unwrap().text;
        assert!(
            text.contains("Not found `nope.py`: not changed in this range"),
            "{text}"
        );
    }

    #[test]
    fn progress_after_a_real_run_has_finished_and_timed_the_scoring() {
        let (_keep, repo) = fixture(2, 0);
        let progress = Progress::default();
        let response = respond(input(&repo, Detail::Summary, &[]), None, &progress);
        assert_eq!(progress.phase(), Phase::Finishing);
        let timing = progress.timing();
        assert!(timing.scoring_ms.unwrap() <= timing.elapsed_ms);
        assert!(progress.describe().contains("after scoring 3 file(s)"));
        let scope = &response.structured_content.unwrap()["scope"];
        assert!(scope["scoring_ms"].as_u64().unwrap() <= scope["elapsed_ms"].as_u64().unwrap());
    }

    #[test]
    fn a_run_with_nothing_to_score_never_reports_scoring() {
        let (_keep, repo) = write_repo(&[("README.md", "# hi\n")]);
        write_files(&repo, &[("README.md", "# changed\n")]);
        commit_all(&repo, "docs only");
        let progress = Progress::default();
        respond(input(&repo, Detail::Summary, &[]), None, &progress);
        assert_eq!(progress.phase(), Phase::Finishing);
        assert_eq!(progress.timing().scoring_ms, None);
        assert!(!progress.describe().contains("while scoring"));
    }
}
