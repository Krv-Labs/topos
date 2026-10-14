//! Dependency-graph (`.gitnexus`) status and generation tools.
//!
//! COMPOSABLE depends on a `.gitnexus` index. `topos_depgraph_status` lets
//! an agent discover graph state without shelling out, and
//! `topos_generate_depgraph` performs the side-effecting regeneration
//! behind an approval-gated annotation.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResponse, CallToolResult, ErrorData as McpError};
use rmcp::service::RequestContext;
use rmcp::{tool, tool_router, RoleServer};

use crate::evaluation::{
    cap_generation_detail, depgraph_analyze_timeout, resolve_depgraph_target,
    run_locked_depgraph_generation, DepgraphStatus, LockedGenerateOutcome,
};
use crate::execution::{EvalCancelled, StatusReporter};
use crate::formatting::{depgraph_unavailable_risk_flags, finish_agent_contract, to_tool_result};
use crate::schemas::{
    DepgraphState, DepgraphStatusInput, DepgraphStatusResult, GenerateDepgraphInput,
    GenerateDepgraphResult,
};
use crate::server::ToposServer;

fn parse_state(state: &str) -> DepgraphState {
    match state {
        "missing" => DepgraphState::Missing,
        "present" => DepgraphState::Present,
        "stale" => DepgraphState::Stale,
        "load_error" => DepgraphState::LoadError,
        "schema_mismatch" => DepgraphState::SchemaMismatch,
        "branch_not_indexed" => DepgraphState::BranchNotIndexed,
        _ => DepgraphState::InvalidDir,
    }
}

/// state -> (recommended action, next_tool, blocked_by code)
fn state_guidance(
    state: DepgraphState,
) -> (&'static str, Option<&'static str>, Option<&'static str>) {
    match state {
        DepgraphState::Missing => (
            "Run topos_generate_depgraph to build the graph and score COMPOSABLE.",
            Some("topos_generate_depgraph"),
            Some("missing_gitnexus_dir"),
        ),
        DepgraphState::Stale => (
            "Run topos_generate_depgraph to refresh the stale graph before trusting COMPOSABLE.",
            Some("topos_generate_depgraph"),
            Some("stale_gitnexus_dir"),
        ),
        DepgraphState::LoadError => (
            "The graph failed to load; reinstall GitNexus dependencies and run \
             topos_generate_depgraph.",
            Some("topos_generate_depgraph"),
            Some("gitnexus_load_error"),
        ),
        DepgraphState::SchemaMismatch => (
            "Graph store was written by a newer GitNexus than this Topos can read. Upgrade \
             Topos (bundled ladybug), or downgrade GitNexus and regenerate with force=true; \
             regenerating with the current GitNexus will not fix it.",
            None,
            Some("gitnexus_schema_mismatch"),
        ),
        DepgraphState::InvalidDir => (
            "The gitnexus_dir override is invalid (outside the file root); \
             fix the path, then retry. Generating won't help.",
            None,
            Some("invalid_gitnexus_dir"),
        ),
        DepgraphState::BranchNotIndexed => (
            "No GitNexus store is indexed for the currently checked-out branch (other \
             branches may be indexed). Run topos_generate_depgraph to index this branch.",
            Some("topos_generate_depgraph"),
            Some("branch_not_indexed_gitnexus_dir"),
        ),
        DepgraphState::Present => (
            "COMPOSABLE is scorable; proceed with topos_evaluate_file.",
            Some("topos_evaluate_file"),
            None,
        ),
    }
}

fn status_to_result(status: &DepgraphStatus) -> DepgraphStatusResult {
    let state = parse_state(status.state);
    let (action, next_tool, blocked_code) = state_guidance(state);
    let blocked_by: Vec<String> = blocked_code.into_iter().map(str::to_string).collect();
    let risk_flags = if state == DepgraphState::Present {
        Vec::new()
    } else {
        depgraph_unavailable_risk_flags(&blocked_by)
    };
    DepgraphStatusResult {
        state,
        gitnexus_dir: status.gitnexus_dir.clone(),
        gitnexus_mtime: status.gitnexus_mtime,
        git_head_mtime: status.git_head_mtime,
        coupling_available: state == DepgraphState::Present,
        detail: status.detail.clone(),
        recommended_next_action: action.to_string(),
        agent_contract: Some(finish_agent_contract(
            blocked_by,
            risk_flags,
            next_tool.map(str::to_string),
            vec![action.to_string()],
            Vec::new(),
        )),
        error: None,
    }
}

fn status_error(message: String) -> DepgraphStatusResult {
    DepgraphStatusResult {
        state: DepgraphState::InvalidDir,
        gitnexus_dir: None,
        gitnexus_mtime: None,
        git_head_mtime: None,
        coupling_available: false,
        detail: None,
        recommended_next_action: "Fix the gitnexus_dir path, then retry.".to_string(),
        agent_contract: Some(finish_agent_contract(
            vec!["invalid_gitnexus_dir".to_string()],
            depgraph_unavailable_risk_flags(&["invalid_gitnexus_dir".to_string()]),
            None,
            Vec::new(),
            Vec::new(),
        )),
        error: Some(message),
    }
}

fn generate_error(message: String) -> GenerateDepgraphResult {
    GenerateDepgraphResult {
        ok: false,
        returncode: 1,
        gitnexus_dir: None,
        generated: false,
        state_before: None,
        message: message.clone(),
        agent_contract: Some(finish_agent_contract(
            vec!["path_error".to_string()],
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
        )),
        error: Some(message),
    }
}

/// Build the failed-generation result, bounding GitNexus's own output.
///
/// `generate_depgraph` runs with `capture = true` here, so a failing
/// `gitnexus analyze` folds its entire trimmed stderr into
/// `DepgraphGenerationResult::message`. That one string is then spent three
/// times over out of the agent's context window: once in `message`, once in
/// `error`, and once more when [`render_generate_md`] echoes `error` into the
/// markdown channel. [`cap_generation_detail`] is the same bound the evaluate
/// path applies to this exact string — see its doc comment for why truncating
/// is safe for the `blocked_by` marker contract (truncation is monotonic, so
/// it can only drop a substring match, never fabricate one). The codes here
/// are fixed literals, so they are unaffected either way.
fn generation_failed(
    returncode: i32,
    message: &str,
    state_before: Option<DepgraphState>,
) -> GenerateDepgraphResult {
    let detail = cap_generation_detail(message);
    GenerateDepgraphResult {
        ok: false,
        returncode,
        gitnexus_dir: None,
        generated: false,
        state_before,
        message: detail.clone(),
        agent_contract: Some(finish_agent_contract(
            vec!["gitnexus_generate_failed".to_string()],
            vec!["composable_unavailable".to_string()],
            None,
            vec!["install/repair GitNexus, then retry".to_string()],
            Vec::new(),
        )),
        error: Some(detail),
    }
}

/// Build the successful-generation result, bounding GitNexus's own output.
///
/// The success message arrives unbounded for the same reason the failure one
/// does:
/// with `capture = true`, `finished_result` returns the child's whole stdout
/// whenever it printed anything, so `gitnexus analyze` progress output on a
/// large repo reaches the agent verbatim through `message` and the markdown.
/// The resolved store path travels separately in `gitnexus_dir`, so nothing
/// actionable is lost to the cap.
fn generation_succeeded(
    gitnexus_dir: Option<String>,
    message: &str,
    state_before: Option<DepgraphState>,
) -> GenerateDepgraphResult {
    GenerateDepgraphResult {
        ok: true,
        returncode: 0,
        gitnexus_dir,
        generated: true,
        state_before,
        message: cap_generation_detail(message),
        agent_contract: Some(finish_agent_contract(
            Vec::new(),
            Vec::new(),
            Some("topos_evaluate_file".to_string()),
            vec!["re-evaluate; COMPOSABLE is now scorable".to_string()],
            Vec::new(),
        )),
        error: None,
    }
}

pub(crate) fn render_status_md(r: &DepgraphStatusResult) -> String {
    if let Some(err) = &r.error {
        return format!("**Error:** {err}");
    }
    let mut lines = vec![
        format!("**Depgraph state:** `{:?}`", r.state).to_lowercase(),
        format!("**COMPOSABLE scorable:** {}", r.coupling_available),
    ];
    if let Some(dir) = &r.gitnexus_dir {
        lines.push(format!("**.gitnexus:** `{dir}`"));
    }
    if let Some(detail) = &r.detail {
        lines.push(format!("**Detail:** {detail}"));
    }
    lines.push(format!("**Next:** {}", r.recommended_next_action));
    lines.join("\n")
}

pub(crate) fn render_generate_md(r: &GenerateDepgraphResult) -> String {
    if let Some(err) = &r.error {
        return format!("**Error:** {err}");
    }
    let head = if r.ok && r.generated {
        "Dependency graph generated."
    } else if r.ok {
        "Dependency graph current."
    } else {
        "Generation failed."
    };
    let mut lines = vec![format!("**{head}**"), r.message.clone()];
    if let Some(dir) = &r.gitnexus_dir {
        lines.push(format!("**.gitnexus:** `{dir}`"));
    }
    lines.join("\n")
}

#[tool_router(router = depgraph_router, vis = "pub(crate)")]
impl ToposServer {
    /// Report `.gitnexus` availability and freshness (read-only).
    ///
    /// Distinguishes a missing graph from a stale one and from a
    /// load/schema failure, so an agent knows whether COMPOSABLE can be
    /// trusted and what to do next. Never shells out and never mutates
    /// state.
    #[tool(
        name = "topos_depgraph_status",
        annotations(
            title = "Topos Depgraph Status",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn topos_depgraph_status(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(params): Parameters<DepgraphStatusInput>,
    ) -> Result<CallToolResult, McpError> {
        self.executor
            .run_inline(&context, move |_cancel_flag| {
                Ok(depgraph_status_sync(params))
            })
            .await
    }

    /// Generate the `.gitnexus` dependency graph via GitNexus
    /// (side-effecting).
    ///
    /// Ensures the graph by default: no-ops when current, otherwise runs
    /// `gitnexus analyze`. `force=true` always regenerates.
    #[tool(
        name = "topos_generate_depgraph",
        annotations(
            title = "Topos Generate Depgraph",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn topos_generate_depgraph(
        &self,
        context: RequestContext<RoleServer>,
        Parameters(params): Parameters<GenerateDepgraphInput>,
    ) -> Result<CallToolResponse, McpError> {
        let (target, pre_status) = match resolve_depgraph_target(
            params.directory.as_deref(),
            params.gitnexus_dir.as_deref(),
            true,
            !params.force,
        ) {
            Ok(resolved) => resolved,
            Err(err) => {
                let model = generate_error(err);
                let md = render_generate_md(&model);
                return Ok(CallToolResponse::Complete(to_tool_result(&model, md)));
            }
        };

        let mut state_before = None;
        if let Some(status) = pre_status {
            let state = parse_state(status.state);
            state_before = Some(state);
            if state == DepgraphState::Present {
                let model = already_current_result(status.gitnexus_dir, state_before);
                let md = render_generate_md(&model);
                return Ok(CallToolResponse::Complete(to_tool_result(&model, md)));
            }
            if state == DepgraphState::SchemaMismatch {
                let model = schema_mismatch_result(&status, state, state_before);
                let md = render_generate_md(&model);
                return Ok(CallToolResponse::Complete(to_tool_result(&model, md)));
            }
        }

        let timeout = match depgraph_analyze_timeout(&target.target_dir) {
            Ok(timeout) => timeout,
            Err(result) => {
                let model = generation_failed(result.returncode, &result.message, state_before);
                let md = render_generate_md(&model);
                return Ok(CallToolResponse::Complete(to_tool_result(&model, md)));
            }
        };
        let force = params.force;

        self.executor
            .run_task(
                &context,
                timeout,
                "Generating GitNexus dependency graph",
                move |cancel_flag, reporter| {
                    generate_depgraph_blocking(
                        &target.target_dir,
                        target.status_override.as_deref(),
                        force,
                        state_before,
                        Some(cancel_flag),
                        Some(reporter),
                    )
                },
            )
            .await
    }
}

pub(crate) fn depgraph_status_sync(params: DepgraphStatusInput) -> CallToolResult {
    let (_target, status) = match resolve_depgraph_target(
        params.directory.as_deref(),
        params.gitnexus_dir.as_deref(),
        false,
        true,
    ) {
        Ok(resolved) => resolved,
        Err(err) => {
            let model = status_error(err);
            let md = render_status_md(&model);
            return to_tool_result(&model, md);
        }
    };
    let status = status.expect("include_status=true");
    let model = status_to_result(&status);
    let md = render_status_md(&model);
    to_tool_result(&model, md)
}

fn already_current_result(
    gitnexus_dir: Option<String>,
    state_before: Option<DepgraphState>,
) -> GenerateDepgraphResult {
    GenerateDepgraphResult {
        ok: true,
        returncode: 0,
        gitnexus_dir,
        generated: false,
        state_before,
        message: "Dependency graph already current.".to_string(),
        agent_contract: Some(finish_agent_contract(
            Vec::new(),
            Vec::new(),
            Some("topos_evaluate_file".to_string()),
            vec!["re-evaluate; COMPOSABLE is scorable".to_string()],
            Vec::new(),
        )),
        error: None,
    }
}

fn schema_mismatch_result(
    status: &DepgraphStatus,
    state: DepgraphState,
    state_before: Option<DepgraphState>,
) -> GenerateDepgraphResult {
    let (action, _, blocked_code) = state_guidance(state);
    let message = status.detail.clone().unwrap_or_else(|| action.to_string());
    GenerateDepgraphResult {
        ok: false,
        returncode: 1,
        gitnexus_dir: status.gitnexus_dir.clone(),
        generated: false,
        state_before,
        message: message.clone(),
        agent_contract: Some({
            let blocked_by: Vec<String> = blocked_code.into_iter().map(str::to_string).collect();
            finish_agent_contract(
                blocked_by.clone(),
                depgraph_unavailable_risk_flags(&blocked_by),
                None,
                vec![action.to_string()],
                Vec::new(),
            )
        }),
        error: Some(message),
    }
}

fn generate_depgraph_blocking(
    target_dir: &Path,
    status_override: Option<&str>,
    force: bool,
    state_before: Option<DepgraphState>,
    cancel_flag: Option<&AtomicBool>,
    status_reporter: Option<&StatusReporter>,
) -> Result<CallToolResult, EvalCancelled> {
    let model = match run_locked_depgraph_generation(
        target_dir,
        status_override,
        force,
        cancel_flag,
        status_reporter,
    )? {
        LockedGenerateOutcome::AlreadyCurrent { gitnexus_dir } => {
            already_current_result(gitnexus_dir, state_before)
        }
        LockedGenerateOutcome::Generated(result) if !result.ok => {
            generation_failed(result.returncode, &result.message, state_before)
        }
        LockedGenerateOutcome::Generated(result) => generation_succeeded(
            result
                .gitnexus_path
                .map(|p| p.to_string_lossy().to_string()),
            &result.message,
            state_before,
        ),
    };
    let md = render_generate_md(&model);
    Ok(to_tool_result(&model, md))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stable fragment of the elision suffix `cap_generation_detail` appends.
    /// Matching a fragment rather than the whole suffix keeps these tests from
    /// pinning the exact wording owned by `crate::evaluation`.
    const ELISION: &str = "bytes elided";

    /// Generous headroom over `GENERATION_DETAIL_CAP` (200 bytes) plus the
    /// elision suffix — the point is "kilobytes do not survive", not an exact
    /// length the cap constant would have to be re-derived from.
    const BOUND: usize = 512;

    #[test]
    fn generation_failure_caps_message_error_and_markdown() {
        let stderr = format!(
            "Error: gitnexus analyze crashed\n{}",
            "    at Module._compile (node:internal/modules/cjs/loader:1105:14)\n".repeat(120)
        );
        assert!(stderr.len() > 4096, "fixture must be multi-kilobyte");

        let model = generation_failed(1, &stderr, Some(DepgraphState::Missing));

        assert!(model.message.contains(ELISION));
        assert!(
            model.message.len() < BOUND,
            "message: {}",
            model.message.len()
        );
        let error = model.error.clone().expect("failure must carry error");
        assert_eq!(
            error, model.message,
            "both channels carry the same capped text"
        );
        // The markdown channel returns early on `error`, so capping `error` is
        // what bounds it — assert on the rendered string, not just the struct.
        let md = render_generate_md(&model);
        assert!(md.contains(ELISION));
        assert!(md.len() < BOUND, "markdown: {}", md.len());
        // The actionable head of the child's output survives the cut.
        assert!(model.message.starts_with("Error: gitnexus analyze crashed"));
    }

    #[test]
    fn generation_success_caps_chatty_child_stdout() {
        let stdout = "indexing src/main.rs ...\n".repeat(400);
        let model = generation_succeeded(
            Some("/repo/.gitnexus".to_string()),
            &stdout,
            Some(DepgraphState::Stale),
        );

        assert!(model.ok && model.generated);
        assert!(model.message.contains(ELISION));
        let md = render_generate_md(&model);
        assert!(md.len() < BOUND, "markdown: {}", md.len());
        // The store path travels in its own field, so the cap never hides it.
        assert!(md.contains("/repo/.gitnexus"));
    }

    #[test]
    fn short_generation_output_is_passed_through_untouched() {
        let model = generation_failed(2, "gitnexus analyze failed.", None);
        assert_eq!(model.message, "gitnexus analyze failed.");
        assert_eq!(model.returncode, 2);

        let ok = generation_succeeded(None, "Dependency graph written to /repo/.gitnexus", None);
        assert_eq!(ok.message, "Dependency graph written to /repo/.gitnexus");
    }

    #[test]
    fn multibyte_child_output_is_never_split_mid_character() {
        // Sweep lengths straddling the cap: a cut that lands on a char
        // boundary for one payload can land inside a code point for another.
        for len in 1..400 {
            for glyph in ["é", "字", "🜁"] {
                let payload = glyph.repeat(len);
                let failed = generation_failed(1, &payload, None);
                let succeeded = generation_succeeded(None, &payload, None);
                // Slicing a `&str` off a char boundary panics, so reaching
                // here already proves the cut was safe. Assert the kept head
                // is a genuine prefix too, so a cap that silently mangled the
                // payload could not pass by merely not crashing.
                for model in [&failed, &succeeded] {
                    let head = model.message.split(" … [").next().expect("non-empty");
                    assert!(payload.starts_with(head), "{len} x {glyph}");
                }
                // Rendering proves the markdown channel is safe too.
                let _ = render_generate_md(&failed);
                let _ = render_generate_md(&succeeded);
            }
        }
    }

    #[test]
    fn failure_contract_codes_are_unchanged_by_capping() {
        let model = generation_failed(1, &"x".repeat(8192), None);
        let contract = model.agent_contract.expect("failure carries a contract");
        assert_eq!(contract.blocked_by, vec!["gitnexus_generate_failed"]);
        assert_eq!(contract.risk_flags, vec!["composable_unavailable"]);
        assert!(contract.next_tool.is_none());
    }

    #[test]
    fn state_guidance_codes_are_stable() {
        let expected = [
            (DepgraphState::Missing, Some("missing_gitnexus_dir")),
            (DepgraphState::Stale, Some("stale_gitnexus_dir")),
            (DepgraphState::LoadError, Some("gitnexus_load_error")),
            (
                DepgraphState::SchemaMismatch,
                Some("gitnexus_schema_mismatch"),
            ),
            (DepgraphState::InvalidDir, Some("invalid_gitnexus_dir")),
            (
                DepgraphState::BranchNotIndexed,
                Some("branch_not_indexed_gitnexus_dir"),
            ),
            (DepgraphState::Present, None),
        ];
        for (state, code) in expected {
            let (_, _, blocked) = state_guidance(state);
            assert_eq!(blocked, code, "{state:?}");
        }
    }
}
