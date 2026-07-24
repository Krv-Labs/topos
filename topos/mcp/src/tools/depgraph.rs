//! Dependency-graph (`.gitnexus`) status and generation tools.
//!
//! COMPOSABLE depends on a `.gitnexus` index. `topos_depgraph_status` lets
//! an agent discover graph state without shelling out, and
//! `topos_generate_depgraph` performs the side-effecting regeneration
//! behind an approval-gated annotation.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use topos_engine::adapters::gitnexus::generate_depgraph;

use crate::evaluation::{depgraph_status, DepgraphStatus};
use crate::formatting::to_tool_result;
use crate::schemas::{
    AgentContract, DepgraphState, DepgraphStatusInput, DepgraphStatusResult, GenerateDepgraphInput,
    GenerateDepgraphResult,
};
use crate::security::{resolve_file_root, resolve_within_root};
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
            "The gitnexus_dir override is invalid (outside the file root or does not exist); \
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
    let risk_flags: Vec<String> = if state != DepgraphState::Present {
        let mut flags = vec!["composable_unavailable".to_string()];
        flags.extend(blocked_code.map(str::to_string));
        flags
    } else {
        Vec::new()
    };
    DepgraphStatusResult {
        state,
        gitnexus_dir: status.gitnexus_dir.clone(),
        gitnexus_mtime: status.gitnexus_mtime,
        git_head_mtime: status.git_head_mtime,
        coupling_available: state == DepgraphState::Present,
        detail: status.detail.clone(),
        recommended_next_action: action.to_string(),
        agent_contract: Some(AgentContract {
            next_tool: next_tool.map(str::to_string),
            next_actions: vec![action.to_string()],
            blocked_by,
            verification_gates: Vec::new(),
            risk_flags,
        }),
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
        agent_contract: Some(AgentContract {
            next_tool: None,
            next_actions: Vec::new(),
            blocked_by: vec!["invalid_gitnexus_dir".to_string()],
            verification_gates: Vec::new(),
            risk_flags: vec![
                "invalid_gitnexus_dir".to_string(),
                "composable_unavailable".to_string(),
            ],
        }),
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
        agent_contract: Some(AgentContract {
            next_tool: None,
            next_actions: Vec::new(),
            blocked_by: vec!["path_error".to_string()],
            verification_gates: Vec::new(),
            risk_flags: Vec::new(),
        }),
        error: Some(message),
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
    pub fn topos_depgraph_status(
        &self,
        Parameters(params): Parameters<DepgraphStatusInput>,
    ) -> CallToolResult {
        let project_root = match resolve_file_root() {
            Ok(root) => root,
            Err(err) => {
                let model = status_error(err);
                let md = render_status_md(&model);
                return to_tool_result(&model, md);
            }
        };
        if let Some(dir) = &params.gitnexus_dir {
            if let Err(err) = resolve_within_root(dir) {
                let model = status_error(err);
                let md = render_status_md(&model);
                return to_tool_result(&model, md);
            }
        }
        let status = depgraph_status(
            params.gitnexus_dir.as_deref(),
            &project_root,
            &project_root.to_string_lossy(),
        );
        let model = status_to_result(&status);
        let md = render_status_md(&model);
        to_tool_result(&model, md)
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
            open_world_hint = true
        )
    )]
    pub fn topos_generate_depgraph(
        &self,
        Parameters(params): Parameters<GenerateDepgraphInput>,
    ) -> CallToolResult {
        let project_root = match resolve_file_root() {
            Ok(root) => root,
            Err(err) => {
                let model = generate_error(err);
                let md = render_generate_md(&model);
                return to_tool_result(&model, md);
            }
        };
        let target_dir = match &params.directory {
            Some(dir) => match resolve_within_root(dir) {
                Ok(resolved) if resolved.is_dir() => resolved,
                Ok(resolved) => {
                    let model = generate_error(format!("Not a directory: {}", resolved.display()));
                    let md = render_generate_md(&model);
                    return to_tool_result(&model, md);
                }
                Err(err) => {
                    let model = generate_error(err);
                    let md = render_generate_md(&model);
                    return to_tool_result(&model, md);
                }
            },
            None => project_root.clone(),
        };

        let mut state_before = None;
        if !params.force {
            let status = depgraph_status(None, &target_dir, &target_dir.to_string_lossy());
            let state = parse_state(status.state);
            state_before = Some(state);
            if state == DepgraphState::Present {
                let model = GenerateDepgraphResult {
                    ok: true,
                    returncode: 0,
                    gitnexus_dir: status.gitnexus_dir,
                    generated: false,
                    state_before,
                    message: "Dependency graph already current.".to_string(),
                    agent_contract: Some(AgentContract {
                        next_tool: Some("topos_evaluate_file".to_string()),
                        next_actions: vec!["re-evaluate; COMPOSABLE is scorable".to_string()],
                        blocked_by: Vec::new(),
                        verification_gates: Vec::new(),
                        risk_flags: Vec::new(),
                    }),
                    error: None,
                };
                let md = render_generate_md(&model);
                return to_tool_result(&model, md);
            }
            if state == DepgraphState::SchemaMismatch {
                let (action, _, blocked_code) = state_guidance(state);
                let message = status.detail.clone().unwrap_or_else(|| action.to_string());
                let model = GenerateDepgraphResult {
                    ok: false,
                    returncode: 1,
                    gitnexus_dir: status.gitnexus_dir,
                    generated: false,
                    state_before,
                    message: message.clone(),
                    agent_contract: Some(AgentContract {
                        next_tool: None,
                        next_actions: vec![action.to_string()],
                        blocked_by: blocked_code.into_iter().map(str::to_string).collect(),
                        verification_gates: Vec::new(),
                        risk_flags: vec![
                            "gitnexus_schema_mismatch".to_string(),
                            "composable_unavailable".to_string(),
                        ],
                    }),
                    error: Some(message),
                };
                let md = render_generate_md(&model);
                return to_tool_result(&model, md);
            }
        }

        let result = generate_depgraph(&target_dir, true, None);
        if !result.ok {
            let model = GenerateDepgraphResult {
                ok: false,
                returncode: result.returncode,
                gitnexus_dir: None,
                generated: false,
                state_before,
                message: result.message.clone(),
                agent_contract: Some(AgentContract {
                    next_tool: None,
                    next_actions: vec!["install/repair GitNexus, then retry".to_string()],
                    blocked_by: vec!["gitnexus_generate_failed".to_string()],
                    verification_gates: Vec::new(),
                    risk_flags: vec!["composable_unavailable".to_string()],
                }),
                error: Some(result.message),
            };
            let md = render_generate_md(&model);
            return to_tool_result(&model, md);
        }

        let model = GenerateDepgraphResult {
            ok: true,
            returncode: 0,
            gitnexus_dir: result
                .gitnexus_path
                .map(|p| p.to_string_lossy().to_string()),
            generated: true,
            state_before,
            message: result.message,
            agent_contract: Some(AgentContract {
                next_tool: Some("topos_evaluate_file".to_string()),
                next_actions: vec!["re-evaluate; COMPOSABLE is now scorable".to_string()],
                blocked_by: Vec::new(),
                verification_gates: Vec::new(),
                risk_flags: Vec::new(),
            }),
            error: None,
        };
        let md = render_generate_md(&model);
        to_tool_result(&model, md)
    }
}
