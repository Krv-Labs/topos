"""
Topos MCP server package.

Public surface:
    - ``mcp``: the ``FastMCP`` instance with all tools, resources, and prompts
      registered (via side-effect imports below).
    - ``main``: stdio entry point.
"""

from topos.mcp import prompts as _prompts  # noqa: F401, E402
from topos.mcp import resources as _resources  # noqa: F401, E402

# Side-effect imports register tools / resources / prompts on ``mcp``.
from topos.mcp import tools as _tools  # noqa: F401, E402
from topos.mcp.server import __version__, main, mcp

__all__ = ["__version__", "main", "mcp"]
