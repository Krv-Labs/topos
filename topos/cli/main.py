"""
CLI entry point — root Click group and command registration.
"""

from __future__ import annotations

import sys

import click

from topos._version import __version__

_ROOT_HELP = """Usage: topos [OPTIONS] COMMAND [ARGS]...

  Topos: Category-theoretic code quality evaluation.

  Treating programs as morphisms in a world of structured code.

Options:
  --version  Show the version and exit.
  -h, --help  Show this message and exit.

Commands:
  compare    Compare structural distance between two programs.
  coverage   Measure structural (UAST) and semantic (CPG Topological) test...
  depgraph   Commands for working with dependency graphs.
  evaluate   Evaluate code quality using the characteristic morphism χ_S : P → Ω.
  inspect    Inspect detailed metrics for a single file.
  mcp        Run the Topos MCP server (stdio).
  uninstall  Remove Topos from this machine.
  update     Upgrade Topos using the detected install channel.
"""


def _fast_path(argv: list[str]) -> bool:
    """Handle trivial invocations without loading evaluation stacks."""
    if len(argv) != 2:
        return False
    if argv[1] in {"--version", "-V"}:
        click.echo(f"topos, version {__version__}")
        return True
    if argv[1] in {"--help", "-h"}:
        click.echo(_ROOT_HELP.rstrip())
        return True
    return False


_commands_registered = False


def _register_commands() -> None:
    global _commands_registered
    if _commands_registered:
        return
    from topos.cli.commands.quality import register_quality_commands
    from topos.cli.commands.system import register_system_commands

    register_quality_commands(cli)
    register_system_commands(cli)
    _commands_registered = True


class ToposGroup(click.Group):
    """Defer subcommand registration until the first real invocation."""

    def invoke(self, ctx: click.Context) -> object:
        _register_commands()
        return super().invoke(ctx)


@click.group(
    cls=ToposGroup,
    context_settings={"help_option_names": ["-h", "--help"]},
)
@click.version_option(version=__version__, prog_name="topos")
@click.pass_context
def cli(ctx: click.Context) -> None:
    """
    Topos: Category-theoretic code quality evaluation.

    Treating programs as morphisms in a world of structured code.
    """
    ctx.ensure_object(dict)


@cli.result_callback()
@click.pass_context
def _notify_updates(ctx: click.Context, result: object, **kwargs: object) -> None:
    from topos.cli.update import (
        maybe_show_install_layout_notice,
        maybe_show_update_notice,
    )

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
    if _fast_path(sys.argv):
        return
    cli()


if __name__ == "__main__":
    main()
