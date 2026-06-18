"""
Project configuration for Topos — the ``.topos.toml`` allowlist.

Security findings are *contextual*: a call like ``yaml.load`` may be an
intentional, trusted pattern in (say) an ML-experiments project.  The
allowlist lets a project acknowledge such patterns so they stop being
reported as actionable findings.

Anti-gaming stance
------------------
The allowlist is **advisory and fully disclosed**, never a silent score
lift (see :mod:`topos.evaluation.suppression`).  To make casual gaming
costly, every entry **requires a non-empty ``reason``**; entries without
one are dropped.  The canonical SECURE verdict is always computed from the
full registry regardless of this file.
"""

from __future__ import annotations

import fnmatch
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

_CONFIG_FILENAME = ".topos.toml"
_CLI_REASON = "CLI --allow (ephemeral)"


@dataclass(frozen=True)
class AllowEntry:
    """A single acknowledged-risk entry from ``[secure.allow]``."""

    pattern: str
    reason: str
    scope: str = "**"

    def matches_path(self, rel_path: str) -> bool:
        """Whether this entry's ``scope`` glob covers *rel_path* (posix)."""
        if self.scope in ("", "**", "*"):
            return True
        return fnmatch.fnmatch(rel_path, self.scope)


@dataclass(frozen=True)
class ToposConfig:
    """Resolved project configuration."""

    allow: list[AllowEntry] = field(default_factory=list)
    root: Path | None = None  # directory the .topos.toml lives in (scope base)

    @property
    def allow_patterns(self) -> set[str]:
        return {entry.pattern for entry in self.allow}

    def entries_for(self, file_path: Path | str | None) -> list[AllowEntry]:
        """Allow entries whose scope covers *file_path*."""
        rel = self._relativize(file_path)
        return [entry for entry in self.allow if entry.matches_path(rel)]

    def _relativize(self, file_path: Path | str | None) -> str:
        if file_path is None:
            return ""
        path = Path(file_path)
        if self.root is not None:
            try:
                path = path.resolve().relative_to(self.root.resolve())
            except ValueError:
                path = Path(file_path)
        return path.as_posix()


def find_config_file(start: Path) -> Path | None:
    """Walk up from *start* (file or dir) to locate ``.topos.toml``."""
    current = start if start.is_dir() else start.parent
    current = current.resolve()
    for directory in (current, *current.parents):
        candidate = directory / _CONFIG_FILENAME
        if candidate.is_file():
            return candidate
    return None


def load_topos_config(start: Path) -> ToposConfig:
    """Load the nearest ``.topos.toml`` at or above *start*.

    Returns an empty config (no allowlist) when no file is found or the
    file is malformed — configuration is best-effort and never fatal.
    """
    config_file = find_config_file(start)
    if config_file is None:
        return ToposConfig()

    try:
        data = tomllib.loads(config_file.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return ToposConfig(root=config_file.parent)

    raw_entries = (data.get("secure") or {}).get("allow") or []
    return ToposConfig(
        allow=_parse_allow_entries(raw_entries),
        root=config_file.parent,
    )


def _parse_allow_entries(raw_entries: object) -> list[AllowEntry]:
    if not isinstance(raw_entries, list):
        return []
    entries: list[AllowEntry] = []
    for raw in raw_entries:
        if not isinstance(raw, dict):
            continue
        pattern = raw.get("pattern")
        reason = raw.get("reason")
        # reason is mandatory anti-gaming friction — drop entries without one.
        if not isinstance(pattern, str) or not pattern.strip():
            continue
        if not isinstance(reason, str) or not reason.strip():
            continue
        scope = raw.get("scope")
        entries.append(
            AllowEntry(
                pattern=pattern.strip(),
                reason=reason.strip(),
                scope=scope.strip() if isinstance(scope, str) and scope.strip() else "**",
            )
        )
    return entries


def merge_cli_allows(config: ToposConfig, allows: tuple[str, ...]) -> ToposConfig:
    """Merge one-off ``--allow`` CLI patterns into *config* (scope ``**``)."""
    extra: list[AllowEntry] = []
    for raw in allows:
        for pattern in raw.split(","):
            pattern = pattern.strip()
            if pattern:
                extra.append(AllowEntry(pattern=pattern, reason=_CLI_REASON))
    if not extra:
        return config
    return ToposConfig(allow=[*config.allow, *extra], root=config.root)
