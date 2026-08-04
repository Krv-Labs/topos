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

use crate::evaluation::{
    cap_generation_detail, depgraph_status, resolve_mcp_composable_project_root,
    resolve_override_for_root, DepgraphStatus,
};
use crate::formatting::to_tool_result;
use crate::schemas::{
    AgentContract, DepgraphState, DepgraphStatusInput, DepgraphStatusResult, GenerateDepgraphInput,
    GenerateDepgraphResult,
};
use crate::security::{resolve_file_root, resolve_within_root};
use crate::server::ToposServer;
use std::path::{Path, PathBuf};

/// Choose the analyze root for `topos_generate_depgraph`.
///
/// Explicit `directory` wins. When omitted, derive from `gitnexus_dir` the
/// same way `topos_depgraph_status` does so status → generate stay aligned.
fn resolve_generate_project_root(
    directory: Option<&Path>,
    gitnexus_dir: Option<&str>,
    file_root: &Path,
) -> PathBuf {
    match directory {
        Some(dir) => dir.to_path_buf(),
        None => resolve_mcp_composable_project_root(gitnexus_dir, file_root),
    }
}

/// Choose the `gitnexus_dir` override to pass to [`depgraph_status`]
/// alongside the root [`resolve_generate_project_root`] just derived.
///
/// Only when that root was itself derived from `gitnexus_dir` (no explicit
/// `directory`) has it already absorbed a relative override's subdirectory
/// — rejoining the raw override against it would then double that
/// subdirectory, so resolve it to an absolute path first in that case. An
/// explicit `directory` is independent of `gitnexus_dir`, so the raw
/// (still relative-to-`directory`) override is correct as-is there.
fn resolve_generate_status_override(
    directory: Option<&Path>,
    gitnexus_dir: Option<&str>,
    file_root: &Path,
) -> Option<String> {
    match directory {
        Some(_) => gitnexus_dir.map(str::to_string),
        None => resolve_override_for_root(gitnexus_dir, file_root),
    }
}

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
        agent_contract: Some(AgentContract {
            next_tool: None,
            next_actions: vec!["install/repair GitNexus, then retry".to_string()],
            blocked_by: vec!["gitnexus_generate_failed".to_string()],
            verification_gates: Vec::new(),
            risk_flags: vec!["composable_unavailable".to_string()],
        }),
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
        agent_contract: Some(AgentContract {
            next_tool: Some("topos_evaluate_file".to_string()),
            next_actions: vec!["re-evaluate; COMPOSABLE is now scorable".to_string()],
            blocked_by: Vec::new(),
            verification_gates: Vec::new(),
            risk_flags: Vec::new(),
        }),
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
    pub fn topos_depgraph_status(
        &self,
        Parameters(params): Parameters<DepgraphStatusInput>,
    ) -> CallToolResult {
        let file_root = match resolve_file_root() {
            Ok(root) => root,
            Err(err) => {
                let model = status_error(err);
                let md = render_status_md(&model);
                return to_tool_result(&model, md);
            }
        };
        let project_root = crate::evaluation::resolve_mcp_composable_project_root(
            params.gitnexus_dir.as_deref(),
            &file_root,
        );
        if let Some(dir) = &params.gitnexus_dir {
            if let Err(err) = resolve_within_root(dir) {
                let model = status_error(err);
                let md = render_status_md(&model);
                return to_tool_result(&model, md);
            }
        }
        // Resolved to an absolute path against `file_root` — must be used
        // below instead of `params.gitnexus_dir`, since `project_root` above
        // already absorbed a relative override's subdirectory; rejoining the
        // original relative string against it a second time would double
        // that subdirectory.
        let resolved_override =
            resolve_override_for_root(params.gitnexus_dir.as_deref(), &file_root);
        let status = depgraph_status(
            resolved_override.as_deref(),
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
        let file_root = match resolve_file_root() {
            Ok(root) => root,
            Err(err) => {
                let model = generate_error(err);
                let md = render_generate_md(&model);
                return to_tool_result(&model, md);
            }
        };
        if let Some(dir) = &params.gitnexus_dir {
            if let Err(err) = resolve_within_root(dir) {
                let model = generate_error(err);
                let md = render_generate_md(&model);
                return to_tool_result(&model, md);
            }
        }
        let resolved_directory = match &params.directory {
            Some(dir) => match resolve_within_root(dir) {
                Ok(resolved) if resolved.is_dir() => Some(resolved),
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
            None => None,
        };
        let target_dir = resolve_generate_project_root(
            resolved_directory.as_deref(),
            params.gitnexus_dir.as_deref(),
            &file_root,
        );
        let status_override = resolve_generate_status_override(
            resolved_directory.as_deref(),
            params.gitnexus_dir.as_deref(),
            &file_root,
        );

        let mut state_before = None;
        if !params.force {
            let status = depgraph_status(
                status_override.as_deref(),
                &target_dir,
                &target_dir.to_string_lossy(),
            );
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
            let model = generation_failed(result.returncode, &result.message, state_before);
            let md = render_generate_md(&model);
            return to_tool_result(&model, md);
        }

        let model = generation_succeeded(
            result
                .gitnexus_path
                .map(|p| p.to_string_lossy().to_string()),
            &result.message,
            state_before,
        );
        let md = render_generate_md(&model);
        to_tool_result(&model, md)
    }
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

    #[test]
    fn resolve_generate_project_root_derives_from_gitnexus_dir_like_status() {
        // Callers pass a canonical file_root (resolve_file_root does); do the
        // same here so macOS /tmp -> /private/tmp does not fake an escape.
        let file_root =
            std::env::temp_dir().join(format!("topos_generate_root_{}", std::process::id()));
        std::fs::create_dir_all(&file_root).unwrap();
        let file_root = file_root.canonicalize().unwrap();
        let nested = file_root.join("nested");
        std::fs::create_dir_all(nested.join(".gitnexus")).unwrap();
        let store = nested.join(".gitnexus");

        let derived =
            resolve_generate_project_root(None, Some(store.to_str().unwrap()), &file_root);
        assert_eq!(derived, nested.canonicalize().unwrap());

        let explicit = resolve_generate_project_root(
            Some(file_root.as_path()),
            Some(store.to_str().unwrap()),
            &file_root,
        );
        assert_eq!(explicit, file_root);

        std::fs::remove_dir_all(&file_root).ok();
    }

    #[test]
    fn resolve_generate_status_override_matches_the_root_it_pairs_with() {
        // Regression: `depgraph_status` re-joins a *relative* override
        // against whatever root it's given. When `directory` is omitted,
        // `resolve_generate_project_root` derives a root from a relative
        // `gitnexus_dir` with a subdirectory (e.g. `repo/.gitnexus`) — so
        // the override paired with it must already be resolved to an
        // absolute path, or the subdirectory doubles
        // (`repo/.gitnexus` -> `repo/repo/.gitnexus`).
        let file_root = std::env::temp_dir().join(format!(
            "topos_generate_status_override_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(file_root.join("repo/.gitnexus")).unwrap();
        let file_root = file_root.canonicalize().unwrap();
        let raw_override = "repo/.gitnexus";

        let derived_override =
            resolve_generate_status_override(None, Some(raw_override), &file_root);
        assert_eq!(
            derived_override.as_deref(),
            file_root.join("repo/.gitnexus").to_str(),
            "must resolve to the real store, not double the subdirectory"
        );

        // An explicit `directory` is independent of `gitnexus_dir` — pass
        // the raw override through unresolved, still relative to that
        // directory.
        let explicit_override = resolve_generate_status_override(
            Some(file_root.as_path()),
            Some(raw_override),
            &file_root,
        );
        assert_eq!(explicit_override.as_deref(), Some(raw_override));

        std::fs::remove_dir_all(&file_root).ok();
    }
}
