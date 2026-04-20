"""
Path-safety helpers for the Topos MCP server.

The server refuses to read files outside ``FILE_ACCESS_ROOT``. Resolution
order:

1. ``TOPOS_MCP_FILE_ROOT`` env var, if set.
2. The nearest ancestor of ``cwd`` that contains ``.git`` or ``pyproject.toml``
   (auto-detect project root).
3. Fail closed: tools return an error explaining how to configure the root.

This avoids the silent fallback to ``cwd`` that made the old default fail open
when launched from some MCP clients.
"""

from __future__ import annotations

import os
from functools import lru_cache
from pathlib import Path

_PROJECT_MARKERS = (".git", "pyproject.toml")


class FileRootNotConfiguredError(RuntimeError):
    """Raised when no file-access root could be determined."""


def _auto_detect_root(start: Path) -> Path | None:
    current = start.resolve()
    for candidate in (current, *current.parents):
        for marker in _PROJECT_MARKERS:
            if (candidate / marker).exists():
                return candidate
    return None


@lru_cache(maxsize=1)
def resolve_file_root() -> Path:
    """Determine the canonical file-access root, caching the result.

    Call ``reset_file_root_cache()`` after mutating ``TOPOS_MCP_FILE_ROOT``
    during tests.
    """
    env_value = os.getenv("TOPOS_MCP_FILE_ROOT")
    if env_value:
        return Path(env_value).expanduser().resolve()

    detected = _auto_detect_root(Path.cwd())
    if detected is not None:
        return detected

    raise FileRootNotConfiguredError(
        "TOPOS_MCP_FILE_ROOT is unset and no project marker (.git / "
        "pyproject.toml) was found by walking up from cwd. Set "
        "TOPOS_MCP_FILE_ROOT to the repository root before starting the MCP "
        "server."
    )


def reset_file_root_cache() -> None:
    """Clear the cached root; useful in tests."""
    resolve_file_root.cache_clear()


def is_within_root(path: Path, root: Path | None = None) -> bool:
    """Return True if ``path`` is equal to or a descendant of the root."""
    root = root or resolve_file_root()
    try:
        path.resolve().relative_to(root)
        return True
    except (ValueError, FileRootNotConfiguredError):
        return False


def read_safe_utf8_file(
    filepath: str | Path,
) -> tuple[str | None, dict[str, str] | None]:
    """Read a UTF-8 file if it is within the configured root.

    Returns ``(source, None)`` on success or ``(None, {"error": "..."})`` when
    the file cannot be read safely.
    """
    path = Path(filepath)

    try:
        root = resolve_file_root()
    except FileRootNotConfiguredError as exc:
        return None, {"error": str(exc)}

    try:
        resolved_path = path.resolve(strict=False)
    except OSError:
        return None, {"error": f"Invalid path: {filepath}"}

    if not is_within_root(resolved_path, root):
        return None, {
            "error": (
                f"Access denied: path must be inside {root}. Got: {resolved_path}"
            )
        }

    try:
        return resolved_path.read_text(encoding="utf-8"), None
    except FileNotFoundError:
        return None, {"error": f"File not found: {filepath}"}
    except IsADirectoryError:
        return None, {"error": f"Path is not a file: {filepath}"}
    except UnicodeDecodeError:
        return None, {"error": f"File is not valid UTF-8 text: {filepath}"}
    except OSError as exc:
        return None, {"error": f"Unable to read file '{filepath}': {exc}"}


def resolve_within_root(
    filepath: str | Path,
) -> tuple[Path | None, dict[str, str] | None]:
    """Resolve a path and check it's inside the root, without reading it."""
    try:
        root = resolve_file_root()
    except FileRootNotConfiguredError as exc:
        return None, {"error": str(exc)}

    resolved = Path(filepath).resolve(strict=False)
    if not is_within_root(resolved, root):
        return None, {
            "error": (f"Access denied: path must be inside {root}. Got: {resolved}")
        }
    return resolved, None
