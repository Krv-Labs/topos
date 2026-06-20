from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

import click

from topos.cli.installation import (
    detect_install_method,
    prune_path_hints,
    remove_provenance_record,
)


def _handle_binary_removal(path: Path, dry_run: bool, yes: bool) -> bool:
    """Helper to handle binary removal logic."""
    if dry_run:
        if path.exists():
            click.echo(f"[dry-run] Would remove binary: {path}")
        else:
            click.echo(f"[dry-run] Binary already removed: {path}")
        return True

    if not yes:
        confirmed = click.confirm(f"Remove binary at {path}?", default=False)
        if not confirmed:
            click.echo("Uninstall cancelled.")
            return False

    if path.exists():
        if not (path.is_file() or path.is_symlink()):
            click.echo(f"Refusing to remove non-file path: {path}", err=True)
            sys.exit(1)

        try:
            path.unlink()
        except OSError as exc:
            click.echo(f"Failed to remove binary {path}: {exc}", err=True)
            sys.exit(1)
        else:
            click.echo(f"Removed binary: {path}")
    else:
        click.echo(f"Binary already removed: {path}")
    return True


@click.command()
@click.option(
    "--dry-run",
    is_flag=True,
    help="Show what would be removed without changing anything.",
)
@click.option(
    "--yes",
    is_flag=True,
    help="Skip confirmation prompts.",
)
@click.option(
    "--prune-path-hints",
    "prune_path_hints_flag",
    is_flag=True,
    help="Remove PATH hint blocks previously added by the installer.",
)
def uninstall(dry_run: bool, yes: bool, prune_path_hints_flag: bool) -> None:
    """Safely uninstall topos based on installation provenance."""
    method, provenance, uninstall_cmd = detect_install_method()

    if method == "package-manager":
        click.echo("Detected package-manager installation.")
        click.echo(f"Run: {uninstall_cmd}")
        return

    if method != "binary-installer" or provenance is None:
        click.echo(
            "Could not determine a managed installer provenance record.",
            err=True,
        )
        click.echo("If installed via pip: pip uninstall topos", err=True)
        click.echo("If installed via uv: uv pip uninstall topos", err=True)
        sys.exit(1)

    install_path = provenance.get("install_path", "").strip()
    if not install_path:
        click.echo("Installer provenance is missing install_path.", err=True)
        sys.exit(1)

    path = Path(install_path).expanduser()
    if not _handle_binary_removal(path, dry_run, yes):
        return

    if not dry_run:
        remove_provenance_record()

    if prune_path_hints_flag:
        prune_path_hints(provenance, dry_run=dry_run)
    else:
        path_hint_file = provenance.get("path_hint_file", "").strip()
        if path_hint_file:
            click.echo(
                "PATH hints were left unchanged. Re-run with --prune-path-hints "
                "to remove installer-added PATH blocks."
            )


@click.command()
def mcp() -> None:
    """Run the Topos MCP server (stdio)."""
    from topos.mcp.server import main as mcp_main

    mcp_main()


@click.group()
def depgraph() -> None:
    """Commands for working with dependency graphs."""


@depgraph.command("generate")
@click.option(
    "--dir",
    "directory",
    type=click.Path(exists=True, file_okay=False),
    default=None,
    help="Repository root to analyze (default: current working directory).",
)
def depgraph_generate(directory: str | None) -> None:
    """Generate a dependency graph using GitNexus."""
    target_dir = Path(directory).resolve() if directory else Path.cwd()

    if shutil.which("gitnexus") is None:
        click.echo(
            "GitNexus not found. Install it with: npm install -g gitnexus",
            err=True,
        )
        sys.exit(1)

    click.echo(
        "Using GitNexus (https://github.com/abhigyanpatwari/GitNexus) "
        "to generate dependency graph...\n"
    )
    click.echo("  $ gitnexus analyze --index-only\n")

    proc = subprocess.run(["gitnexus", "analyze", "--index-only"], cwd=target_dir)
    if proc.returncode != 0:
        sys.exit(proc.returncode)

    gitnexus_path = target_dir / ".gitnexus"
    click.echo(f"\nDependency graph written to {gitnexus_path}")
    click.echo(f"Next: topos evaluate src/ -r --gitnexus-dir {gitnexus_path}")


def register_system_commands(cli_group: click.Group) -> None:
    """Attach system and integration commands to the root CLI group."""
    cli_group.add_command(uninstall)
    cli_group.add_command(mcp)
    cli_group.add_command(depgraph)
