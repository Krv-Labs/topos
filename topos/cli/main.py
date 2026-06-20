"""
CLI entry point — root Click group and command registration.
"""

from __future__ import annotations

import click

from topos import __version__
from topos.cli.commands.quality import register_quality_commands
from topos.cli.commands.system import register_system_commands


@click.group(context_settings={"help_option_names": ["-h", "--help"]})
@click.version_option(version=__version__, prog_name="topos")
def cli() -> None:
    """
    Topos: Category-theoretic code quality evaluation.

    Treating programs as morphisms in a world of structured code.
    """


register_quality_commands(cli)
register_system_commands(cli)


def main() -> None:
    """Console script entrypoint."""
    cli()


if __name__ == "__main__":
    main()
