from __future__ import annotations

import importlib.metadata
import os
from pathlib import Path

import click


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


def detect_install_method() -> tuple[str, dict[str, str] | None, str | None]:
    provenance = load_provenance()
    if provenance and provenance.get("install_method") == "binary-installer":
        return "binary-installer", provenance, None

    try:
        dist = importlib.metadata.distribution("topos")
    except importlib.metadata.PackageNotFoundError:
        return "unknown", None, None

    try:
        installer_raw = dist.read_text("INSTALLER")
    except (FileNotFoundError, OSError, UnicodeError):
        installer_raw = "pip"

    installer = (installer_raw or "").strip().lower()
    if installer == "uv":
        return "package-manager", None, "uv pip uninstall topos"
    if installer in {"pip", ""}:
        return "package-manager", None, "pip uninstall topos"
    return "package-manager", None, f"{installer} uninstall topos"


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
