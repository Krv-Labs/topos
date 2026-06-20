"""
Topos MCP server — FastMCP instance + entry point.

Server name follows the ``{service}_mcp`` convention (MCP best-practices, Nov
2025 spec). Transport is stdio by default; the ``main()`` entry point is launched
by the ``topos mcp`` CLI command.
"""

from __future__ import annotations

from topos import __version__

_mcp_instance = None


def _get_mcp():
    """Lazily instantiate FastMCP to avoid circular import with external mcp library."""
    global _mcp_instance
    if _mcp_instance is None:
        from fastmcp import FastMCP

        _mcp_instance = FastMCP(
            "topos_mcp",
            version=__version__,
            instructions=(
                "Topos evaluates structural code quality on a diamond lattice. "
                "For agent loops, load the compact contract with "
                '`topos_get_doc(topic="agent-contract")` or '
                "fetch `topos://docs/agent-contract`. "
                "Key call pattern: topos_evaluate_file → topos_assess_improvement. "
                "Use gitnexus_dir (default: ./.gitnexus) to enable COMPOSABLE/IDEAL. "
                "topos_calculate_coverage reports test-suite coverage — structural "
                "(UAST) and semantic (ECT) — as a separate signal, outside the lattice."
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
    """stdio entry point launched by the ``topos mcp`` CLI command."""
    # Import side-effect registers tools / resources / prompts.
    from topos.mcp import (
        prompts,  # noqa: F401
        resources,  # noqa: F401
        tools,  # noqa: F401
    )

    _get_mcp().run()


if __name__ == "__main__":
    main()
