//! The `topos_pr_recap` tool body: run the recap, and answer with the
//! summary (`topos.pr_recap.summary.v1`) or, for `detail: "full"`, the
//! `topos.pr_recap.v3` document the CLI's `--json` prints.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

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
use crate::schemas::{PrRecapDetail, PrRecapInput};
use crate::security::{composable_default_root, resolve_file_root, resolve_project_path};
use topos_engine::adapters::gitnexus::gitnexus_available;

/// What the MCP tool calls the scoring cap in the hints that name it.
const MCP_CAP_NAME: &str = "max_files";

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
                PrRecapDetail::Summary => summary_result(&recap, &root, &paths, progress),
                PrRecapDetail::Full => full_result(&recap, &root, &paths),
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
    let markdown = format!(
        "**PR Recap ({})**: `{}` ({:?}) — {}\nScored {} file(s) (`{}...{}`).\n{}",
        recap.check,
        recap.readiness.word(),
        recap.direction,
        recap.reason,
        recap.scope.files_scored,
        recap.base,
        recap.head,
        not_found_block(&missing),
    );
    to_tool_result(&value, markdown)
}

/// An error in the shape of the mode asked for: the summary carries its
/// schema, the full document keeps its bare `error`.
fn error_result(detail: PrRecapDetail, error: &str) -> CallToolResult {
    let body = match detail {
        PrRecapDetail::Summary => json!({ "schema": SUMMARY_SCHEMA, "error": error }),
        PrRecapDetail::Full => json!({ "error": error }),
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
    use super::*;
    use crate::pr_recap::progress::Phase;

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
}
