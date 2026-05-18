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
    assert "topos://docs/workflows" in text
