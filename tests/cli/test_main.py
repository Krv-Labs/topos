from __future__ import annotations

from click.testing import CliRunner
from topos import __version__
from topos.cli.main import _ROOT_HELP, _register_commands, cli

_SUBCOMMANDS = (
    "compare",
    "coverage",
    "depgraph",
    "evaluate",
    "inspect",
    "mcp",
    "uninstall",
    "update",
)


def test_cli_version():
    _register_commands()
    runner = CliRunner()
    result = runner.invoke(cli, ["--version"])
    assert result.exit_code == 0
    assert f"topos, version {__version__}" in result.output


def test_cli_help():
    _register_commands()
    runner = CliRunner()
    result = runner.invoke(cli, ["--help"])
    assert result.exit_code == 0
    assert "Topos: Category-theoretic code quality evaluation." in result.output
    assert "evaluate" in result.output
    assert "compare" in result.output
    assert "inspect" in result.output
    assert "coverage" in result.output
    assert "uninstall" in result.output
    assert "mcp" in result.output
    assert "depgraph" in result.output


def test_main_fast_path_version():
    import subprocess
    import sys

    completed = subprocess.run(
        [sys.executable, "-m", "topos.cli.main", "--version"],
        check=True,
        capture_output=True,
        text=True,
    )
    assert f"topos, version {__version__}" in completed.stdout


def test_cli_entry_point_help_lists_subcommands_cold():
    """Regression test for the installed console-script entry point.

    ``pyproject.toml`` wires ``topos = "topos.cli.main:main"``, but the
    underlying Click group (``cli``) is also reachable directly (e.g. if the
    entry point ever regresses back to pointing at ``cli``, or via any code
    that imports and calls ``cli()`` itself). Click resolves ``--help`` and
    the no-args-is-help check inside ``parse_args()``, which runs before
    ``ToposGroup`` would otherwise register subcommands — so this must be
    exercised in a fresh interpreter that has never called
    ``_register_commands()``, or an in-process test could pass purely because
    an earlier test already populated ``cli.commands``.
    """
    import subprocess
    import sys

    script = (
        "from click.testing import CliRunner\n"
        "from topos.cli.main import cli\n"
        "r = CliRunner().invoke(cli, ['--help'])\n"
        "print(r.output)\n"
        "raise SystemExit(r.exit_code)\n"
    )
    completed = subprocess.run(
        [sys.executable, "-c", script], capture_output=True, text=True
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    for name in _SUBCOMMANDS:
        assert name in completed.stdout, f"{name!r} missing from cold --help output"


def test_cli_entry_point_no_args_lists_subcommands_cold():
    """Bare ``topos`` (no args) must also show real subcommands.

    Click's ``no_args_is_help`` prints the group help but exits with usage
    error code 2 (not 0) — this is intentional Click behavior, not a bug.
    """
    import subprocess
    import sys

    script = (
        "from click.testing import CliRunner\n"
        "from topos.cli.main import cli\n"
        "r = CliRunner().invoke(cli, [])\n"
        "print(r.output)\n"
        "raise SystemExit(r.exit_code)\n"
    )
    completed = subprocess.run(
        [sys.executable, "-c", script], capture_output=True, text=True
    )
    assert completed.returncode == 2, completed.stdout + completed.stderr
    for name in _SUBCOMMANDS:
        assert name in completed.stdout, f"{name!r} missing from cold no-args output"


def test_root_help_matches_click_generated_help():
    """``_ROOT_HELP`` (the fast-path text) must match Click's real output.

    Guards against the fast path silently drifting from the real CLI (e.g. a
    command docstring changes but the hardcoded fast-path string doesn't).
    """
    _register_commands()
    runner = CliRunner()
    result = runner.invoke(cli, ["--help"], prog_name="topos")
    assert result.exit_code == 0
    assert _ROOT_HELP.rstrip() == result.output.rstrip()


def test_dash_v_not_accepted_anywhere():
    """``-V`` is not a supported alias for ``--version`` on either path."""
    import subprocess
    import sys

    _register_commands()
    result = CliRunner().invoke(cli, ["-V"])
    assert result.exit_code != 0

    completed = subprocess.run(
        [sys.executable, "-m", "topos.cli.main", "-V"],
        capture_output=True,
        text=True,
    )
    assert completed.returncode != 0
    assert f"topos, version {__version__}" not in completed.stdout
