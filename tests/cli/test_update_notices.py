from __future__ import annotations

from datetime import UTC, datetime, timedelta
from unittest.mock import patch

from click.testing import CliRunner

from topos.cli.main import cli
from topos.cli.update import (
    cache_is_fresh,
    is_outdated,
    maybe_show_update_notice,
    normalize_version,
    should_skip_passive_notice,
)


def test_normalize_version():
    assert normalize_version("v0.3.5") == (0, 3, 5)
    assert normalize_version("1.2") == (1, 2, 0)


def test_is_outdated():
    assert is_outdated("0.3.5", "0.3.6")
    assert not is_outdated("0.3.6", "0.3.6")
    assert not is_outdated("0.3.7", "0.3.6")


def test_should_skip_passive_notice_mcp():
    assert should_skip_passive_notice(invoked_subcommand="mcp", help_requested=False)


def test_should_skip_passive_notice_env(monkeypatch):
    monkeypatch.setenv("TOPOS_NO_UPDATE_NOTICES", "1")
    assert should_skip_passive_notice(invoked_subcommand="evaluate", help_requested=False)


def test_should_skip_passive_notice_ci(monkeypatch):
    monkeypatch.setenv("CI", "true")
    assert should_skip_passive_notice(invoked_subcommand="evaluate", help_requested=False)


def test_cache_is_fresh():
    recent = {
        "checked_at": datetime.now(UTC).isoformat(),
        "latest": "0.3.6",
    }
    assert cache_is_fresh(recent)

    stale = {
        "checked_at": (datetime.now(UTC) - timedelta(hours=25)).isoformat(),
        "latest": "0.3.6",
    }
    assert not cache_is_fresh(stale)


@patch("topos.cli.update.is_outdated", return_value=True)
@patch("topos.cli.update.save_update_check_cache")
@patch("topos.cli.update.latest_version_for_channel", return_value="9.9.9")
@patch("topos.cli.update.load_update_check_cache", return_value=None)
@patch("topos.cli.update.detect_install_info")
@patch("topos.cli.update.click.echo")
@patch("topos.cli.update.sys.stderr.isatty", return_value=True)
def test_maybe_show_update_notice_emits(
    mock_isatty,
    mock_echo,
    mock_detect,
    mock_load_cache,
    mock_latest,
    mock_save,
    mock_outdated,
    monkeypatch,
):
    from topos.cli.installation import InstallInfo

    monkeypatch.delenv("CI", raising=False)
    mock_detect.return_value = InstallInfo(method="binary-installer")

    maybe_show_update_notice(invoked_subcommand="evaluate", help_requested=False)

    mock_echo.assert_called_once()
    assert "Update available" in mock_echo.call_args[0][0]
    assert mock_echo.call_args[1]["err"] is True


@patch("topos.cli.update.latest_version_for_channel")
@patch("topos.cli.update.load_update_check_cache")
@patch("topos.cli.update.detect_install_info")
@patch("topos.cli.update.click.echo")
@patch("topos.cli.update.sys.stderr.isatty", return_value=True)
def test_maybe_show_update_notice_uses_fresh_cache(
    mock_isatty,
    mock_echo,
    mock_detect,
    mock_load_cache,
    mock_latest,
    monkeypatch,
):
    from topos.cli.installation import InstallInfo

    monkeypatch.delenv("CI", raising=False)
    mock_detect.return_value = InstallInfo(method="binary-installer")
    mock_load_cache.return_value = {
        "checked_at": datetime.now(UTC).isoformat(),
        "latest": "9.9.9",
    }

    with patch("topos.cli.update.is_outdated", return_value=True):
        maybe_show_update_notice(invoked_subcommand="evaluate", help_requested=False)

    mock_latest.assert_not_called()
    mock_echo.assert_called_once()


def test_passive_notice_skipped_for_mcp():
    with patch("topos.mcp.server.main"):
        with patch("topos.cli.main.maybe_show_update_notice") as mock_notice:
            runner = CliRunner()
            result = runner.invoke(cli, ["mcp"])
            assert result.exit_code == 0
            mock_notice.assert_called_once()
            assert mock_notice.call_args.kwargs["invoked_subcommand"] == "mcp"


def test_passive_notice_skipped_for_update_command():
    with patch("topos.cli.commands.system.run_update") as mock_run_update:
        with patch("topos.cli.main.maybe_show_update_notice") as mock_notice:
            runner = CliRunner()
            with patch(
                "topos.cli.update.latest_version_for_channel",
                return_value="0.0.0",
            ):
                result = runner.invoke(cli, ["update", "--check"])
            assert result.exit_code in (0, 1, 2)
            mock_run_update.assert_called_once()
            mock_notice.assert_called_once()
            assert mock_notice.call_args.kwargs["invoked_subcommand"] == "update"
