"""Tool registration hub — importing this module attaches every tool to ``mcp``."""

from . import (  # noqa: F401
    assess,
    compare,
    coverage,
    depgraph,
    docs,
    evaluate,
    inspect,
    preferences,
    refactor,
)
