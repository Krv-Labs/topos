"""Context-budget ratchet for the MCP tool-definition surface.

Every agent session pays for the JSON wire definition of *all* registered
tools (name + description + inputSchema + outputSchema + annotations). At the
time this guard was added the total was ~88,900 chars (~22,200 tokens), with
``outputSchema`` blocks dominating the bill.

These tests are upper-bound *ratchets*, not equality checks: the ceilings are
set comfortably above today's measured surface so the suite is green now, and we
lower them as later phases shrink the schemas. They exist to catch silent
regrowth, not to pin exact sizes.

Token cost is approximated as ``chars // 4`` -- the same crude heuristic used to
take the original baseline. We deliberately avoid a real tokenizer dependency;
the path to a precise figure would be to swap ``_approx_tokens`` for, e.g.,
``tiktoken``/``anthropic`` token counting, but that is not worth a new dep for a
regression guard.
"""

from __future__ import annotations

import asyncio
import json

import pytest

# Importing this module registers every tool on the FastMCP instance.
from topos.mcp import tools  # noqa: F401
from topos.mcp.server import _get_mcp

# Loose ceilings -- GREEN today, ratcheted DOWN as the surface shrinks.
TOTAL_CEILING_CHARS = 95_000
# RATCHET: lower this ceiling as Phases 1-2 shrink the surface
# (target <32_000 chars / ~8K tokens).

PER_TOOL_CEILING_CHARS = 25_000
# RATCHET: lower this per-tool ceiling as Phases 1-2 shrink the surface
# (today's worst offender, topos_assess_improvement, is ~22,765 chars).


def _approx_tokens(chars: int) -> int:
    """Crude token estimate matching the baseline heuristic (~4 chars/token)."""
    return chars // 4


def _wire_sizes() -> dict[str, int]:
    """Return {tool_name: serialized_wire_chars} for all registered tools."""

    async def _collect() -> dict[str, int]:
        sizes: dict[str, int] = {}
        for tool in await _get_mcp().list_tools():
            # ``to_mcp_tool`` yields the exact dict shipped over the wire:
            # name, description, inputSchema, outputSchema, annotations.
            wire = tool.to_mcp_tool().model_dump(exclude_none=True)
            sizes[wire["name"]] = len(json.dumps(wire))
        return sizes

    return asyncio.run(_collect())


def _report(sizes: dict[str, int]) -> str:
    """Human-readable breakdown, emitted only on assertion failure."""
    lines = [
        f"{n:8d} chars (~{_approx_tokens(n):5d} tok)  {name}"
        for name, n in sorted(sizes.items(), key=lambda kv: -kv[1])
    ]
    total = sum(sizes.values())
    lines.append(f"{total:8d} chars (~{_approx_tokens(total):5d} tok)  TOTAL")
    return "\n".join(lines)


def test_total_tool_surface_under_ceiling() -> None:
    sizes = _wire_sizes()
    total = sum(sizes.values())
    assert total <= TOTAL_CEILING_CHARS, (
        f"MCP tool surface grew to {total} chars "
        f"(~{_approx_tokens(total)} tok), ceiling {TOTAL_CEILING_CHARS}.\n"
        + _report(sizes)
    )


def test_per_tool_surface_under_ceiling() -> None:
    sizes = _wire_sizes()
    over = {n: c for n, c in sizes.items() if c > PER_TOOL_CEILING_CHARS}
    assert not over, (
        f"Tool(s) exceed per-tool ceiling {PER_TOOL_CEILING_CHARS} chars: "
        f"{over}.\n" + _report(sizes)
    )


if __name__ == "__main__":
    # Convenience: ``python tests/mcp/test_context_budget.py`` prints the budget.
    print(_report(_wire_sizes()))
    raise SystemExit(pytest.main([__file__, "-q"]))
