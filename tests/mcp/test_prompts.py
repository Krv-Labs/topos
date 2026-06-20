"""Tests for prompts."""

from __future__ import annotations

import asyncio

import topos.mcp.prompts  # noqa: F401
import topos.mcp.resources  # noqa: F401
import topos.mcp.tools  # noqa: F401
from topos.mcp.server import mcp


def test_refactor_prompt_registered() -> None:
    prompts = asyncio.run(mcp.list_prompts())
    names = {p.name for p in prompts}
    assert "topos_refactor_until_ideal" in names


def test_refactor_prompt_renders_with_filepath() -> None:
    prompt = asyncio.run(mcp.get_prompt("topos_refactor_until_ideal"))
    assert prompt is not None
    rendered = asyncio.run(prompt.render({"filepath": "topos/__init__.py"}))
    text = " ".join(msg.content.text for msg in rendered.messages)
    assert "topos/__init__.py" in text
    assert "SUSPICIOUS_NO_STRUCTURAL_CHANGE" in text
    assert "topos://docs/agent-contract" in text
    assert '"params"' in text
    assert "topos_evaluate_file" in text
    assert "topos_assess_improvement" in text
    assert "behavior tests" in text
    assert "Step 1" not in text
    assert "Begin at Step 1" not in text
    assert "FIRST:" not in text
    assert len(text) < 2500
