"""Dependency-graph (``.gitnexus``) status and generation tools.

COMPOSABLE depends on a ``.gitnexus`` index. ``topos_depgraph_status`` lets an
agent discover graph state (missing / present / stale / load_error /
schema_mismatch / invalid_dir) without shelling out, and
``topos_generate_depgraph`` performs
the side-effecting regeneration behind an approval-gated annotation.
"""

from __future__ import annotations

from fastmcp.tools.base import ToolResult

from topos.utils.gitnexus import generate_depgraph

from ..evaluation import DepgraphStatus, depgraph_status
from ..formatting import to_tool_result
from ..schemas import (
    AgentContract,
    DepgraphState,
    DepgraphStatusInput,
    DepgraphStatusResult,
    GenerateDepgraphInput,
    GenerateDepgraphResult,
)
from ..security import resolve_file_root, resolve_within_root
from ..server import mcp

_READ_ONLY_ANN = {
    "title": "Topos Depgraph Status",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}

# Generation shells out to GitNexus and rewrites .gitnexus — not read-only and
# not idempotent. Clients should treat it as approval-gated.
_GENERATE_ANN = {
    "title": "Topos Generate Depgraph",
    "readOnlyHint": False,
    "destructiveHint": False,
    "idempotentHint": False,
    "openWorldHint": True,
}

# state -> (recommended action, next_tool, blocked_by code)
_STATE_GUIDANCE: dict[DepgraphState, tuple[str, str | None, str | None]] = {
    DepgraphState.MISSING: (
        "Run topos_generate_depgraph to build the graph and score COMPOSABLE.",
        "topos_generate_depgraph",
        "missing_gitnexus_dir",
    ),
    DepgraphState.STALE: (
        "Run topos_generate_depgraph to refresh the stale graph before trusting "
        "COMPOSABLE.",
        "topos_generate_depgraph",
        "stale_gitnexus_dir",
    ),
    DepgraphState.LOAD_ERROR: (
        "The graph failed to load; reinstall GitNexus dependencies and run "
        "topos_generate_depgraph.",
        "topos_generate_depgraph",
        "gitnexus_load_error",
    ),
    DepgraphState.SCHEMA_MISMATCH: (
        "LadybugDB storage version mismatch; upgrade GitNexus/ladybug, then run "
        "topos_generate_depgraph.",
        "topos_generate_depgraph",
        "gitnexus_schema_mismatch",
    ),
    DepgraphState.INVALID_DIR: (
        "The gitnexus_dir override is invalid (outside the file root or does not "
        "exist); fix the path, then retry. Generating won't help.",
        None,
        "invalid_gitnexus_dir",
    ),
    DepgraphState.PRESENT: (
        "COMPOSABLE is scorable; proceed with topos_evaluate_file.",
        "topos_evaluate_file",
        None,
    ),
}


@mcp.tool(
    name="topos_depgraph_status",
    tags={"depgraph", "workflow"},
    annotations=_READ_ONLY_ANN,
)
def topos_depgraph_status(params: DepgraphStatusInput) -> ToolResult:
    """Report ``.gitnexus`` availability and freshness (read-only).

    Distinguishes a missing graph from a stale one and from a load/schema
    failure, so an agent knows whether COMPOSABLE can be trusted and what to do
    next. Never shells out and never mutates state.
    """
    project_root = resolve_file_root()
    if params.gitnexus_dir is not None:
        resolved, err = resolve_within_root(params.gitnexus_dir)
        if err or resolved is None:
            return _status_error((err or {}).get("error", "path error"))

    status = depgraph_status(params.gitnexus_dir, project_root, str(project_root))
    return _status_to_result(status)


@mcp.tool(
    name="topos_generate_depgraph",
    tags={"depgraph", "workflow"},
    annotations=_GENERATE_ANN,
)
def topos_generate_depgraph(params: GenerateDepgraphInput) -> ToolResult:
    """Generate the ``.gitnexus`` dependency graph via GitNexus (side-effecting).

    Runs ``gitnexus analyze`` in the target directory. Not read-only and not
    idempotent — intended to be approval-gated by the client. Requires the
    ``gitnexus`` CLI (``npm install -g gitnexus``).
    """
    project_root = resolve_file_root()
    if params.directory is not None:
        resolved, err = resolve_within_root(params.directory)
        if err or resolved is None:
            return _generate_error((err or {}).get("error", "path error"))
        if not resolved.is_dir():
            return _generate_error(f"Not a directory: {resolved}")
        target_dir = resolved
    else:
        target_dir = project_root

    result = generate_depgraph(target_dir)
    if not result.ok:
        model = GenerateDepgraphResult(
            ok=False,
            returncode=result.returncode,
            message=result.message,
            agent_contract=AgentContract(
                blocked_by=["gitnexus_generate_failed"],
                risk_flags=["composable_unavailable"],
                next_actions=["install/repair GitNexus, then retry"],
            ),
            error=result.message,
        )
        return to_tool_result(model, _render_generate_md(model))

    model = GenerateDepgraphResult(
        ok=True,
        returncode=0,
        gitnexus_dir=str(result.gitnexus_path),
        message=result.message,
        agent_contract=AgentContract(
            next_tool="topos_evaluate_file",
            next_actions=["re-evaluate; COMPOSABLE is now scorable"],
        ),
    )
    return to_tool_result(model, _render_generate_md(model))


def _status_to_result(status: DepgraphStatus) -> ToolResult:
    state = DepgraphState(status.state)
    action, next_tool, blocked_code = _STATE_GUIDANCE[state]
    blocked_by = [blocked_code] if blocked_code else []
    # Mirror the state-specific code into risk_flags (not just composable_unavailable)
    # so clients branching on risk_flags alone can tell stale / load_error /
    # schema_mismatch / invalid_dir apart.
    risk_flags: list[str] = []
    if state != DepgraphState.PRESENT:
        risk_flags.append("composable_unavailable")
    if blocked_code:
        risk_flags.append(blocked_code)
    model = DepgraphStatusResult(
        state=state,
        gitnexus_dir=status.gitnexus_dir,
        gitnexus_mtime=status.gitnexus_mtime,
        git_head_mtime=status.git_head_mtime,
        coupling_available=state == DepgraphState.PRESENT,
        detail=status.detail,
        recommended_next_action=action,
        agent_contract=AgentContract(
            next_tool=next_tool,
            next_actions=[action],
            blocked_by=blocked_by,
            risk_flags=risk_flags,
        ),
    )
    return to_tool_result(model, _render_status_md(model))


def _status_error(message: str) -> ToolResult:
    # A rejected gitnexus_dir path is an invalid override, not a missing graph:
    # never route it to generation (which won't fix a bad path).
    model = DepgraphStatusResult(
        state=DepgraphState.INVALID_DIR,
        coupling_available=False,
        recommended_next_action="Fix the gitnexus_dir path, then retry.",
        agent_contract=AgentContract(
            blocked_by=["invalid_gitnexus_dir"],
            risk_flags=["invalid_gitnexus_dir", "composable_unavailable"],
        ),
        error=message,
    )
    return to_tool_result(model, _render_status_md(model))


def _generate_error(message: str) -> ToolResult:
    model = GenerateDepgraphResult(
        ok=False,
        returncode=1,
        message=message,
        agent_contract=AgentContract(blocked_by=["path_error"]),
        error=message,
    )
    return to_tool_result(model, _render_generate_md(model))


def _render_status_md(r: DepgraphStatusResult) -> str:
    if r.error:
        return f"**Error:** {r.error}"
    lines = [
        f"**Depgraph state:** `{r.state.value}`",
        f"**COMPOSABLE scorable:** {r.coupling_available}",
    ]
    if r.gitnexus_dir:
        lines.append(f"**.gitnexus:** `{r.gitnexus_dir}`")
    if r.detail:
        lines.append(f"**Detail:** {r.detail}")
    lines.append(f"**Next:** {r.recommended_next_action}")
    return "\n".join(lines)


def _render_generate_md(r: GenerateDepgraphResult) -> str:
    if r.error:
        return f"**Error:** {r.error}"
    head = "Dependency graph generated." if r.ok else "Generation failed."
    lines = [f"**{head}**", r.message]
    if r.gitnexus_dir:
        lines.append(f"**.gitnexus:** `{r.gitnexus_dir}`")
    return "\n".join(lines)
