from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

from click.testing import CliRunner
from topos.cli.main import cli


def test_mcp_invokes_server():
    with patch("topos.mcp.server.main") as mock_mcp_main:
        runner = CliRunner()
        result = runner.invoke(cli, ["mcp"])
        assert result.exit_code == 0
        mock_mcp_main.assert_called_once()


def test_depgraph_generate_no_gitnexus():
    with patch("shutil.which", return_value=None):
        runner = CliRunner()
        result = runner.invoke(cli, ["depgraph", "generate"])
        assert result.exit_code != 0
        assert "GitNexus not found" in result.output


@patch("subprocess.run")
@patch("shutil.which", return_value="/usr/local/bin/gitnexus")
def test_depgraph_generate_success(mock_which, mock_run):
    mock_run.return_value.returncode = 0
    runner = CliRunner()
    result = runner.invoke(cli, ["depgraph", "generate"])
    assert result.exit_code == 0
    assert "Using GitNexus" in result.output
    mock_run.assert_called_once_with(
        ["gitnexus", "analyze", "--index-only"],
        cwd=Path.cwd(),
    )


def test_uninstall_package_manager():
    with patch("topos.cli.commands.system.detect_install_method") as mock_detect:
        mock_detect.return_value = ("package-manager", None, "pip uninstall topos")
        runner = CliRunner()
        result = runner.invoke(cli, ["uninstall"])
        assert result.exit_code == 0
        assert "Detected package-manager installation." in result.output
        assert "pip uninstall topos" in result.output


def test_uninstall_binary_dry_run(tmp_path: Path):
    binary = tmp_path / "topos"
    binary.touch()

    provenance = {"install_method": "binary-installer", "install_path": str(binary)}

    with patch("topos.cli.commands.system.detect_install_method") as mock_detect:
        mock_detect.return_value = ("binary-installer", provenance, None)
        runner = CliRunner()
        result = runner.invoke(cli, ["uninstall", "--dry-run"])
        assert result.exit_code == 0
        assert f"[dry-run] Would remove binary: {binary}" in result.output
        assert binary.exists()


def test_uninstall_binary_confirm_no(tmp_path: Path):
    binary = tmp_path / "topos"
    binary.touch()

    provenance = {"install_method": "binary-installer", "install_path": str(binary)}

    with patch("topos.cli.commands.system.detect_install_method") as mock_detect:
        mock_detect.return_value = ("binary-installer", provenance, None)
        runner = CliRunner()
        # "n" for the confirmation prompt
        result = runner.invoke(cli, ["uninstall"], input="n\n")
        assert result.exit_code == 0
        assert "Uninstall cancelled." in result.output
        assert binary.exists()


def test_uninstall_binary_yes(tmp_path: Path):
    binary = tmp_path / "topos"
    binary.touch()

    provenance = {"install_method": "binary-installer", "install_path": str(binary)}

    with (
        patch("topos.cli.commands.system.detect_install_method") as mock_detect,
        patch("topos.cli.commands.system.remove_provenance_record") as mock_remove,
    ):
        mock_detect.return_value = ("binary-installer", provenance, None)
        runner = CliRunner()
        result = runner.invoke(cli, ["uninstall", "--yes"])
        assert result.exit_code == 0
        assert f"Removed binary: {binary}" in result.output
        assert not binary.exists()
        mock_remove.assert_called_once()
