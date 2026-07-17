from __future__ import annotations

import sys
from pathlib import Path

import click

from topos.cli.installation import (
    detect_install_info,
    echo_install_layout_notice,
    prune_path_hints,
    remove_state_dir,
)
from topos.cli.update import run_update
from topos.utils.gitnexus import (
    generate_depgraph,
    gitnexus_available,
    gitnexus_install_hint,
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
    "--keep-path-hints",
    is_flag=True,
    help="Skip removal of installer-added PATH blocks from shell rc files.",
)
def uninstall(dry_run: bool, yes: bool, keep_path_hints: bool) -> None:
    """Safely uninstall topos based on installation provenance."""
    info = detect_install_info()
    echo_install_layout_notice(info=info)
    method = info.method
    provenance = info.provenance
    uninstall_cmd = info.uninstall_cmd

    if method == "package-manager":
        click.echo("Detected package-manager installation.")
        click.echo(f"Run: {uninstall_cmd}")
        return

    if method != "binary-installer" or provenance is None:
        click.echo(
            "Could not determine a managed installer provenance record.",
            err=True,
        )
        click.echo("If installed via pip: pip uninstall topos-mcp", err=True)
        click.echo("If installed via uv: uv pip uninstall topos-mcp", err=True)
        sys.exit(1)

    install_path = provenance.get("install_path", "").strip()
    if not install_path:
        click.echo("Installer provenance is missing install_path.", err=True)
        sys.exit(1)

    path = Path(install_path).expanduser()
    if not _handle_binary_removal(path, dry_run, yes):
        return

    remove_state_dir(dry_run=dry_run)

    if not keep_path_hints:
        prune_path_hints(provenance, dry_run=dry_run)


@click.command()
@click.option(
    "--check",
    is_flag=True,
    help="Exit 0 if up to date, 1 if outdated (for scripts).",
)
@click.option(
    "--version",
    "pin_version",
    default=None,
    help="Pin release version for binary installs (e.g. v0.3.6).",
)
def update(check: bool, pin_version: str | None) -> None:
    """Upgrade Topos using the detected install channel."""
    run_update(check_only=check, pin_version=pin_version)


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

    if not gitnexus_available():
        click.echo(gitnexus_install_hint(), err=True)
        sys.exit(1)

    click.echo(
        "Using GitNexus (https://github.com/abhigyanpatwari/GitNexus) "
        "to generate dependency graph...\n"
    )
    click.echo("  $ gitnexus analyze --skip-agents-md\n")

    result = generate_depgraph(target_dir, capture=False)
    if not result.ok:
        sys.exit(result.returncode)

    click.echo(f"\nDependency graph written to {result.gitnexus_path}")
    click.echo(f"Next: topos evaluate src/ -r --gitnexus-dir {result.gitnexus_path}")


def register_system_commands(cli_group: click.Group) -> None:
    """Attach system and integration commands to the root CLI group."""
    cli_group.add_command(update)
    cli_group.add_command(uninstall)
    cli_group.add_command(mcp)
    cli_group.add_command(depgraph)
