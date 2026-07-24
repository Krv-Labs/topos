from __future__ import annotations

import importlib.metadata
import json
import os
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

import click

_PACKAGE_NAME = "topos-mcp"
# Default roots where broad prefix matching is safe (not shared with binary installs).
_PRIVATE_HOMEBREW_PREFIXES = (
    Path("/opt/homebrew"),
    Path("/home/linuxbrew/.linuxbrew"),
)
_SHARED_HOMEBREW_PREFIX = Path("/usr/local")


@dataclass(frozen=True)
class InstallInfo:
    """Detected install channel and related commands."""

    method: str
    provenance: dict[str, str] | None = None
    uninstall_cmd: str | None = None
    update_cmd: str | None = None
    installer: str | None = None


def provenance_file() -> Path:
    override = os.environ.get("TOPOS_PROVENANCE_FILE")
    if override:
        return Path(override).expanduser()
    state_home = Path(os.environ.get("XDG_STATE_HOME", "~/.local/state")).expanduser()
    return state_home / "topos" / "install-provenance"


def load_provenance() -> dict[str, str] | None:
    path = provenance_file()
    if not path.exists():
        return None

    data: dict[str, str] = {}
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip()
        if not key:
            continue
        data[key] = value
    return data or None


def _is_editable_install(dist: importlib.metadata.Distribution) -> bool:
    try:
        direct_url_raw = dist.read_text("direct_url.json")
    except (FileNotFoundError, OSError, UnicodeError):
        return False
    if not direct_url_raw:
        return False
    try:
        direct_url = json.loads(direct_url_raw)
    except json.JSONDecodeError:
        return False
    return bool((direct_url.get("dir_info") or {}).get("editable"))


def _package_manager_info(installer: str) -> InstallInfo:
    if installer == "uv":
        return InstallInfo(
            method="package-manager",
            uninstall_cmd=f"uv pip uninstall {_PACKAGE_NAME}",
            update_cmd=f"uv pip install -U {_PACKAGE_NAME}",
            installer="uv",
        )
    if installer in {"pip", ""}:
        return InstallInfo(
            method="package-manager",
            uninstall_cmd=f"pip uninstall {_PACKAGE_NAME}",
            update_cmd=f"pip install -U {_PACKAGE_NAME}",
            installer="pip",
        )
    return InstallInfo(
        method="package-manager",
        uninstall_cmd=f"{installer} uninstall {_PACKAGE_NAME}",
        update_cmd=f"{installer} install -U {_PACKAGE_NAME}",
        installer=installer,
    )


def _install_info_from_distribution(
    dist: importlib.metadata.Distribution,
) -> InstallInfo:
    if _is_editable_install(dist):
        return InstallInfo(
            method="source",
            update_cmd="git pull && uv pip install -e .",
        )

    try:
        installer_raw = dist.read_text("INSTALLER")
    except (FileNotFoundError, OSError, UnicodeError):
        installer_raw = "pip"

    installer = (installer_raw or "").strip().lower()
    return _package_manager_info(installer)


def _is_homebrew_cellar_layout(relative_parts: tuple[str, ...]) -> bool:
    return (
        len(relative_parts) >= 4
        and relative_parts[0] == "Cellar"
        and bool(relative_parts[1])
        and bool(relative_parts[2])
    )


def _is_homebrew_executable(path: Path) -> bool:
    """True when the resolved executable lives inside a Homebrew prefix.

    Three-tier classification:
    1. Explicit ``HOMEBREW_PREFIX`` — broad prefix match (custom roots, linked kegs).
    2. Private default roots (``/opt/homebrew``, Linuxbrew) — broad prefix match.
    3. Shared ``/usr/local`` — Cellar layout only (avoids ``/usr/local/bin`` false positives).
    """
    try:
        resolved_path = path.expanduser().resolve()
    except OSError:
        return False

    configured_prefix = os.environ.get("HOMEBREW_PREFIX", "").strip()
    if configured_prefix:
        try:
            prefix_path = Path(configured_prefix).expanduser()
            if prefix_path.is_absolute():
                prefix = prefix_path.resolve()
                if resolved_path.is_relative_to(prefix):
                    return True
        except (OSError, RuntimeError):
            pass

    for prefix_path in _PRIVATE_HOMEBREW_PREFIXES:
        try:
            prefix = prefix_path.expanduser().resolve()
            if resolved_path.is_relative_to(prefix):
                return True
        except OSError:
            continue

    try:
        usr_local = _SHARED_HOMEBREW_PREFIX.resolve()
        relative_parts = resolved_path.relative_to(usr_local).parts
        if _is_homebrew_cellar_layout(relative_parts):
            return True
    except (OSError, ValueError):
        pass

    return False


def _homebrew_info() -> InstallInfo:
    return InstallInfo(
        method="homebrew",
        uninstall_cmd="brew uninstall topos",
        update_cmd="brew upgrade topos",
        installer="brew",
    )


def _install_info_from_provenance(
    provenance: dict[str, str] | None,
) -> InstallInfo | None:
    if provenance and provenance.get("install_method") == "binary-installer":
        return InstallInfo(method="binary-installer", provenance=provenance)
    return None


def detect_install_info() -> InstallInfo:
    provenance = load_provenance()

    try:
        dist = importlib.metadata.distribution(_PACKAGE_NAME)
    except importlib.metadata.PackageNotFoundError:
        # A brew-run binary wins over stale curl-installer provenance records.
        if _is_homebrew_executable(active_executable()):
            return _homebrew_info()
        binary_info = _install_info_from_provenance(provenance)
        if binary_info is not None:
            return binary_info
        return InstallInfo(method="unknown")

    # Active Python install metadata wins over stale binary provenance records.
    return _install_info_from_distribution(dist)


def detect_install_method() -> tuple[str, dict[str, str] | None, str | None]:
    info = detect_install_info()
    return info.method, info.provenance, info.uninstall_cmd


def active_executable() -> Path:
    """Resolved path of the executable/script running this process."""
    return Path(sys.argv[0]).expanduser().resolve()


def find_topos_executables_on_path() -> list[Path]:
    """All distinct ``topos`` executables found on PATH (order preserved)."""
    results: list[Path] = []
    seen: set[Path] = set()
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        entry = entry.strip()
        if not entry:
            continue
        candidate = Path(entry).expanduser() / "topos"
        try:
            if candidate.is_file() and os.access(candidate, os.X_OK):
                resolved = candidate.resolve()
                if resolved not in seen:
                    seen.add(resolved)
                    results.append(resolved)
        except OSError:
            continue
    return results


def channel_label(info: InstallInfo) -> str:
    if info.method == "binary-installer":
        return "binary CLI"
    if info.method == "homebrew":
        return "Homebrew"
    if info.method == "source":
        return "editable source checkout"
    if info.method == "package-manager":
        installer = info.installer or "pip"
        return f"PyPI ({installer})"
    return info.method


def install_layout_notice_lines(info: InstallInfo | None = None) -> list[str] | None:
    """Return notice lines when multiple installs may conflict, else None."""
    info = info or detect_install_info()
    active = active_executable()
    path_bins = find_topos_executables_on_path()
    provenance = load_provenance()

    other_bins = [path for path in path_bins if path != active]
    which_bin = shutil.which("topos")
    which_resolved = Path(which_bin).resolve() if which_bin else None

    stale_binary: Path | None = None
    if provenance and provenance.get("install_method") == "binary-installer":
        recorded = provenance.get("install_path", "").strip()
        if recorded and info.method != "binary-installer":
            recorded_path = Path(recorded).expanduser()
            try:
                if recorded_path.exists() and recorded_path.resolve() != active:
                    stale_binary = recorded_path.resolve()
            except OSError:
                pass

    path_precedence_mismatch = which_resolved is not None and which_resolved != active

    if not other_bins and stale_binary is None and not path_precedence_mismatch:
        return None

    lines = [
        "Multiple Topos installations detected.",
        f"  Active: {active} ({channel_label(info)}) — "
        "`topos update` / `topos uninstall` use this install.",
    ]

    if which_resolved is not None and which_resolved != active:
        lines.append(
            f"  PATH default: {which_resolved} "
            "(runs when you type `topos` without a full path)"
        )

    for path in other_bins:
        if path != which_resolved:
            lines.append(f"  Also on PATH: {path}")

    if stale_binary is not None and stale_binary not in other_bins:
        lines.append(
            f"  Binary installer record: {stale_binary} "
            "(remove with that binary's `topos uninstall`, or delete manually)"
        )

    return lines


def echo_install_layout_notice(
    *, info: InstallInfo | None = None, err: bool = True
) -> bool:
    """Print install layout notice when relevant. Returns True if printed."""
    lines = install_layout_notice_lines(info)
    if lines is None:
        return False
    click.echo("\n".join(lines), err=err)
    return True


def prune_path_hints(provenance: dict[str, str], dry_run: bool) -> None:
    path_hint_file = provenance.get("path_hint_file", "").strip()
    if not path_hint_file:
        click.echo("No PATH hint file recorded in installer provenance.")
        return

    marker_begin = provenance.get(
        "path_hint_begin", "# BEGIN TOPOS INSTALLER PATH"
    ).strip()
    marker_end = provenance.get("path_hint_end", "# END TOPOS INSTALLER PATH").strip()
    rc_path = Path(path_hint_file).expanduser()

    if not rc_path.exists():
        click.echo(f"PATH hint file already absent: {rc_path}")
        return

    original_content = rc_path.read_text(encoding="utf-8")
    had_trailing_newline = original_content.endswith("\n")
    original_lines = original_content.splitlines()
    begin_index = None
    end_index = None

    for idx, line in enumerate(original_lines):
        stripped = line.strip()
        if stripped == marker_begin and begin_index is None:
            begin_index = idx
            continue
        if stripped == marker_end and begin_index is not None and end_index is None:
            end_index = idx
            break

    if begin_index is None:
        click.echo(f"No installer PATH hint block found in {rc_path}")
        return

    if end_index is None:
        click.echo(
            f"Malformed PATH hint block in {rc_path}: missing end marker {marker_end}"
        )
        return

    removed_lines = end_index - begin_index + 1
    updated_lines = original_lines[:begin_index] + original_lines[end_index + 1 :]

    if dry_run:
        click.echo(
            f"[dry-run] Would prune {removed_lines} PATH hint lines in {rc_path}"
        )
        return

    new_content = "\n".join(updated_lines)
    if had_trailing_newline:
        new_content += "\n"
    rc_path.write_text(new_content, encoding="utf-8")
    click.echo(f"Pruned installer PATH hints from {rc_path}")


def topos_state_dir() -> Path:
    state_home = Path(os.environ.get("XDG_STATE_HOME", "~/.local/state")).expanduser()
    return state_home / "topos"


def remove_state_dir(dry_run: bool = False) -> None:
    """Remove the entire topos XDG state directory (caches, provenance)."""
    state_dir = topos_state_dir()
    if not state_dir.exists():
        return
    if dry_run:
        click.echo(f"[dry-run] Would remove state directory: {state_dir}")
        return
    try:
        shutil.rmtree(state_dir)
    except OSError as exc:
        click.echo(f"Failed to remove state directory {state_dir}: {exc}", err=True)
        return
    click.echo(f"Removed state directory: {state_dir}")


def remove_provenance_record() -> None:
    provenance_path = provenance_file()
    try:
        provenance_path.unlink()
    except FileNotFoundError:
        return
    except OSError as exc:
        click.echo(
            f"Failed to remove provenance file {provenance_path}: {exc}",
            err=True,
        )
        return
    click.echo(f"Removed provenance record: {provenance_path}")
