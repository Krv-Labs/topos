"""
Topos MCP server package.

Public surface:
    - ``mcp``: the ``FastMCP`` instance with all tools, resources, and prompts
      registered (via side-effect imports in main() and decorators).
    - ``main``: stdio entry point.
"""

from .server import __version__, main, mcp

__all__ = ["__version__", "main", "mcp"]
