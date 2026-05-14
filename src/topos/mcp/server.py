"""
Topos MCP server — FastMCP instance + entry point.

Server name follows the ``{service}_mcp`` convention (MCP best-practices, Nov
2025 spec). Transport is stdio by default; the ``main()`` entry point is wired
to ``topos-mcp`` in ``pyproject.toml``.
"""

from __future__ import annotations

from importlib.metadata import PackageNotFoundError, version

try:
    __version__ = version("topos")
except PackageNotFoundError:
    __version__ = "dev"


_mcp_instance = None


def _get_mcp():
    """Lazily instantiate FastMCP to avoid circular import with external mcp library."""
    global _mcp_instance
    if _mcp_instance is None:
        from fastmcp import FastMCP

        _mcp_instance = FastMCP(
            "mcp",
            version=__version__,
            instructions=(
                "Topos evaluates Python code quality on a diamond lattice. "
                "FIRST: load the workflow guide — call "
                '`topos_get_doc(topic="workflows")` '
                "(works on any client) OR fetch `topos://docs/workflows` as a resource "
                "(Claude Code, Cursor). "
                "Key call pattern: topos_evaluate_file → topos_assess_improvement. "
                "Use gitnexus_dir (default: ./.gitnexus) to enable COMPOSABLE/SOUND."
            ),
        )
    return _mcp_instance


# Export lazy accessor as 'mcp' for backward compatibility with existing code
class _MCPProxy:
    """Proxy to lazily initialize FastMCP."""

    def __getattr__(self, name):
        return getattr(_get_mcp(), name)

    def __call__(self, *args, **kwargs):
        return _get_mcp()(*args, **kwargs)


mcp = _MCPProxy()


def main() -> None:
    """stdio entry point wired to the ``topos-mcp`` console script."""
    # Import side-effect registers tools / resources / prompts.
    from topos.mcp import (
        prompts,  # noqa: F401
        resources,  # noqa: F401
        tools,  # noqa: F401
    )

    _get_mcp().run()


if __name__ == "__main__":
    main()
