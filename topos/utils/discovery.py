"""Prune third-party and ignored paths when discovering source files."""

from __future__ import annotations

import fnmatch
import subprocess
from collections.abc import Callable, Iterator
from pathlib import Path

# Directory names skipped during traversal (common venvs, caches, build outputs).
SKIP_DIR_NAMES: frozenset[str] = frozenset(
    {
        ".git",
        ".gitnexus",
        ".hg",
        ".svn",
        ".venv",
        "venv",
        "venv.bak",
        "env",
        "__pycache__",
        "__pypackages__",
        "node_modules",
        "dist",
        "build",
        "out",
        "target",
        ".next",
        ".turbo",
        "coverage",
        "htmlcov",
        ".pytest_cache",
        ".mypy_cache",
        ".tox",
        ".ruff_cache",
        ".eggs",
        ".pixi",
    }
)

_TOPOSIGNORE_NAME = ".toposignore"


def is_virtualenv_root(dir_path: Path) -> bool:
    """True when *dir_path* looks like a Python virtual environment root."""
    if (dir_path / "pyvenv.cfg").is_file():
        return True
    bin_dir = dir_path / "bin"
    if (bin_dir / "python").exists() or (bin_dir / "python3").exists():
        return True
    scripts = dir_path / "Scripts"
    return (scripts / "python.exe").is_file()


def should_skip_dir(dir_path: Path) -> bool:
    """Whether to avoid descending into *dir_path* during discovery."""
    if dir_path.name in SKIP_DIR_NAMES:
        return True
    return is_virtualenv_root(dir_path)


def find_git_root(start: Path) -> Path | None:
    """Return the repository root containing ``.git``, if any."""
    resolved = start.resolve()
    for candidate in (resolved, *resolved.parents):
        if (candidate / ".git").exists():
            return candidate
    return None


def _load_ignore_patterns(ignore_file: Path) -> list[str]:
    if not ignore_file.is_file():
        return []
    patterns: list[str] = []
    for raw in ignore_file.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("!"):
            continue
        patterns.append(line.rstrip("/"))
    return patterns


def _matches_ignore_pattern(rel_posix: str, pattern: str) -> bool:
    """Best-effort gitignore-style match for common project patterns."""
    if pattern.startswith("/"):
        return fnmatch.fnmatch(rel_posix, pattern.lstrip("/")) or rel_posix == pattern.lstrip(
            "/"
        )
    if "/" in pattern:
        return fnmatch.fnmatch(rel_posix, pattern) or rel_posix.startswith(f"{pattern}/")
    name = rel_posix.rsplit("/", 1)[-1]
    if fnmatch.fnmatch(name, pattern):
        return True
    return fnmatch.fnmatch(rel_posix, pattern) or f"/{pattern}/" in f"/{rel_posix}/"


def _toposignore_checker(root: Path) -> Callable[[Path], bool] | None:
    patterns = _load_ignore_patterns(root / _TOPOSIGNORE_NAME)
    if not patterns:
        return None

    def check(path: Path) -> bool:
        try:
            rel = path.relative_to(root).as_posix()
        except ValueError:
            return False
        return any(_matches_ignore_pattern(rel, pat) for pat in patterns)

    return check


def _git_check_ignore_checker(git_root: Path) -> Callable[[Path], bool] | None:
    try:
        subprocess.run(
            ["git", "--version"],
            capture_output=True,
            check=True,
            timeout=2,
        )
    except (FileNotFoundError, OSError, subprocess.TimeoutExpired):
        return None

    def check(path: Path) -> bool:
        try:
            rel = path.relative_to(git_root).as_posix()
        except ValueError:
            return False
        try:
            result = subprocess.run(
                ["git", "-C", str(git_root), "check-ignore", "-q", "--", rel],
                capture_output=True,
                timeout=1,
            )
        except (OSError, subprocess.TimeoutExpired):
            return False
        return result.returncode == 0

    return check


def build_path_skip_checker(scan_root: Path) -> Callable[[Path], bool]:
    """Compose hard-coded, ``.toposignore``, and git ignore checks for *scan_root*."""
    git_root = find_git_root(scan_root)
    checkers: list[Callable[[Path], bool]] = []

    topos = _toposignore_checker(scan_root)
    if topos is not None:
        checkers.append(topos)

    if git_root is not None:
        git_check = _git_check_ignore_checker(git_root)

        def git_or_topos(path: Path) -> bool:
            if git_check is not None and git_check(path):
                return True
            return topos is not None and topos(path)

        if git_check is not None or topos is not None:
            return git_or_topos

    if topos is not None:
        return topos

    return lambda _path: False


def iter_source_files(
    root: Path,
    *,
    suffixes: tuple[str, ...],
    recursive: bool = True,
    is_ignored: Callable[[Path], bool] | None = None,
) -> Iterator[Path]:
    """Yield source files under *root*, pruning venvs and ignored directories."""
    if root.is_file():
        if root.suffix in suffixes and not (is_ignored and is_ignored(root)):
            yield root
        return

    if not root.is_dir():
        return

    ignore = is_ignored or (lambda _p: False)
    stack: list[Path] = [root]

    while stack:
        current = stack.pop()
        try:
            entries = list(current.iterdir())
        except OSError:
            continue

        for entry in sorted(entries, key=lambda p: p.name):
            if entry.is_dir():
                if should_skip_dir(entry) or ignore(entry):
                    continue
                if recursive:
                    stack.append(entry)
            elif entry.is_file() and entry.suffix in suffixes:
                if not ignore(entry):
                    yield entry


def collect_source_files(
    paths: tuple[str, ...] | list[str],
    *,
    suffixes: tuple[str, ...],
    recursive: bool = True,
) -> list[Path]:
    """Collect source files from explicit paths (files or directories)."""
    files: set[Path] = set()

    for path_str in paths:
        path = Path(path_str)
        if path.is_file():
            if path.suffix in suffixes:
                files.add(path)
            continue
        if not path.is_dir():
            continue

        is_ignored = build_path_skip_checker(path)
        files.update(
            iter_source_files(
                path,
                suffixes=suffixes,
                recursive=recursive,
                is_ignored=is_ignored,
            )
        )

    return sorted(files)
