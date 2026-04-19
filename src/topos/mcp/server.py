"""
Topos MCP server — FastMCP instance + entry point.

Server name follows the ``{service}_mcp`` convention (MCP best-practices, Nov
2025 spec). Transport is stdio by default; the ``main()`` entry point is wired
to ``topos-mcp`` in ``pyproject.toml``.
"""

from __future__ import annotations

from importlib.metadata import PackageNotFoundError, version

from fastmcp import FastMCP

try:
    __version__ = version("topos")
except PackageNotFoundError:
    __version__ = "dev"


mcp = FastMCP(
    "topos_mcp",
    version=__version__,
    instructions=(
        "Topos evaluates Python code quality on a diamond lattice. "
        "Read topos://docs/workflows for the canonical agent refactor loop. "
        "Key call pattern: topos_evaluate_file → topos_assess_improvement. "
        "Use gitnexus_dir (default: ./.gitnexus) to enable COMPOSABLE/SOUND."
    ),
)


def main() -> None:
    """stdio entry point wired to the ``topos-mcp`` console script."""
    # Import side-effect registers tools / resources / prompts.
    from topos.mcp import (
        prompts,  # noqa: F401
        resources,  # noqa: F401
        tools,  # noqa: F401
    )

    mcp.run()


if __name__ == "__main__":
    main()
