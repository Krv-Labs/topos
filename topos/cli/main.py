"""
CLI entry point — root Click group and command registration.
"""

from __future__ import annotations

import sys

import click

from topos import __version__
from topos.cli.commands.quality import register_quality_commands
from topos.cli.commands.system import register_system_commands
from topos.cli.update import maybe_show_install_layout_notice, maybe_show_update_notice


@click.group(context_settings={"help_option_names": ["-h", "--help"]})
@click.version_option(version=__version__, prog_name="topos")
@click.pass_context
def cli(ctx: click.Context) -> None:
    """
    Topos: Category-theoretic code quality evaluation.

    Treating programs as morphisms in a world of structured code.
    """
    ctx.ensure_object(dict)


register_quality_commands(cli)
register_system_commands(cli)


@cli.result_callback()
@click.pass_context
def _notify_updates(ctx: click.Context, result: object, **kwargs: object) -> None:
    help_requested = any(arg in sys.argv for arg in ("-h", "--help"))
    maybe_show_install_layout_notice(
        invoked_subcommand=ctx.invoked_subcommand,
        help_requested=help_requested,
    )
    maybe_show_update_notice(
        invoked_subcommand=ctx.invoked_subcommand,
        help_requested=help_requested,
    )


def main() -> None:
    """Console script entrypoint."""
    cli()


if __name__ == "__main__":
    main()
