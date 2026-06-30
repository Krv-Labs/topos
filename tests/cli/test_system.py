from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

from click.testing import CliRunner
from topos.cli.installation import InstallInfo
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
        ["gitnexus", "analyze", "--skip-agents-md"],
        cwd=Path.cwd(),
    )


def test_uninstall_package_manager():
    info = InstallInfo(method="package-manager", uninstall_cmd="pip uninstall topos")
    with (
        patch("topos.cli.commands.system.detect_install_info", return_value=info),
        patch("topos.cli.commands.system.echo_install_layout_notice"),
    ):
        runner = CliRunner()
        result = runner.invoke(cli, ["uninstall"])
        assert result.exit_code == 0
        assert "Detected package-manager installation." in result.output
        assert "pip uninstall topos" in result.output


def test_uninstall_binary_dry_run(tmp_path: Path):
    binary = tmp_path / "topos"
    binary.touch()

    provenance = {"install_method": "binary-installer", "install_path": str(binary)}
    info = InstallInfo(method="binary-installer", provenance=provenance)

    with (
        patch("topos.cli.commands.system.detect_install_info", return_value=info),
        patch("topos.cli.commands.system.echo_install_layout_notice"),
        patch("topos.cli.commands.system.remove_state_dir") as mock_remove_state,
        patch("topos.cli.commands.system.prune_path_hints") as mock_prune,
    ):
        runner = CliRunner()
        result = runner.invoke(cli, ["uninstall", "--dry-run"])
        assert result.exit_code == 0
        assert f"[dry-run] Would remove binary: {binary}" in result.output
        assert binary.exists()
        mock_remove_state.assert_called_once_with(dry_run=True)
        mock_prune.assert_called_once_with(provenance, dry_run=True)


def test_uninstall_binary_confirm_no(tmp_path: Path):
    binary = tmp_path / "topos"
    binary.touch()

    provenance = {"install_method": "binary-installer", "install_path": str(binary)}
    info = InstallInfo(method="binary-installer", provenance=provenance)

    with (
        patch("topos.cli.commands.system.detect_install_info", return_value=info),
        patch("topos.cli.commands.system.echo_install_layout_notice"),
        patch("topos.cli.commands.system.remove_state_dir") as mock_remove_state,
    ):
        runner = CliRunner()
        result = runner.invoke(cli, ["uninstall"], input="n\n")
        assert result.exit_code == 0
        assert "Uninstall cancelled." in result.output
        assert binary.exists()
        mock_remove_state.assert_not_called()


def test_uninstall_binary_yes(tmp_path: Path):
    binary = tmp_path / "topos"
    binary.touch()

    provenance = {"install_method": "binary-installer", "install_path": str(binary)}
    info = InstallInfo(method="binary-installer", provenance=provenance)

    with (
        patch("topos.cli.commands.system.detect_install_info", return_value=info),
        patch("topos.cli.commands.system.echo_install_layout_notice"),
        patch("topos.cli.commands.system.remove_state_dir") as mock_remove_state,
        patch("topos.cli.commands.system.prune_path_hints") as mock_prune,
    ):
        runner = CliRunner()
        result = runner.invoke(cli, ["uninstall", "--yes"])
        assert result.exit_code == 0
        assert f"Removed binary: {binary}" in result.output
        assert not binary.exists()
        mock_remove_state.assert_called_once_with(dry_run=False)
        mock_prune.assert_called_once_with(provenance, dry_run=False)


def test_uninstall_binary_keep_path_hints(tmp_path: Path):
    binary = tmp_path / "topos"
    binary.touch()

    provenance = {"install_method": "binary-installer", "install_path": str(binary)}
    info = InstallInfo(method="binary-installer", provenance=provenance)

    with (
        patch("topos.cli.commands.system.detect_install_info", return_value=info),
        patch("topos.cli.commands.system.echo_install_layout_notice"),
        patch("topos.cli.commands.system.remove_state_dir"),
        patch("topos.cli.commands.system.prune_path_hints") as mock_prune,
    ):
        runner = CliRunner()
        result = runner.invoke(cli, ["uninstall", "--yes", "--keep-path-hints"])
        assert result.exit_code == 0
        mock_prune.assert_not_called()


def test_uninstall_removes_state_dir(tmp_path: Path):
    """remove_state_dir is called and removes the XDG state directory."""
    state_dir = tmp_path / "state" / "topos"
    state_dir.mkdir(parents=True)
    (state_dir / "update-check.json").write_text("{}")
    (state_dir / "install-provenance").write_text("install_method=binary-installer\n")

    binary = tmp_path / "bin" / "topos"
    binary.parent.mkdir()
    binary.touch()

    provenance = {"install_method": "binary-installer", "install_path": str(binary)}
    info = InstallInfo(method="binary-installer", provenance=provenance)

    with (
        patch("topos.cli.commands.system.detect_install_info", return_value=info),
        patch("topos.cli.commands.system.echo_install_layout_notice"),
        patch("topos.cli.commands.system.prune_path_hints"),
        patch(
            "topos.cli.installation.topos_state_dir",
            return_value=state_dir,
        ),
    ):
        runner = CliRunner()
        result = runner.invoke(cli, ["uninstall", "--yes"])
        assert result.exit_code == 0
        assert not state_dir.exists()
        assert "Removed state directory" in result.output


@patch("topos.cli.update.subprocess.run")
@patch("topos.cli.update.subprocess.Popen")
def test_update_binary_invokes_install_script(mock_popen, mock_run, tmp_path: Path):
    from unittest.mock import MagicMock

    binary = tmp_path / "bin" / "topos"
    binary.parent.mkdir(parents=True)
    binary.touch()
    provenance = {"install_method": "binary-installer", "install_path": str(binary)}

    curl_proc = mock_popen.return_value
    curl_proc.stdout = MagicMock()
    curl_proc.returncode = 0
    mock_run.return_value.returncode = 0

    with patch(
        "topos.cli.update.detect_install_info",
    ) as mock_detect:
        from topos.cli.installation import InstallInfo

        mock_detect.return_value = InstallInfo(
            method="binary-installer",
            provenance=provenance,
        )
        runner = CliRunner()
        result = runner.invoke(cli, ["update"])
        assert result.exit_code == 0
        assert "Updating Topos via install.sh" in result.output

    mock_popen.assert_called_once()
    _, kwargs = mock_run.call_args
    env = kwargs["env"]
    assert env["TOPOS_UPDATE"] == "1"
    assert env["TOPOS_INSTALL"] == str(binary.parent)
    assert env["TOPOS_NO_MODIFY_PATH"] == "1"


@patch("topos.cli.update.subprocess.run")
@patch("topos.cli.update.subprocess.Popen")
def test_update_binary_fails_when_curl_fails(mock_popen, mock_run, tmp_path: Path):
    from unittest.mock import MagicMock

    binary = tmp_path / "bin" / "topos"
    binary.parent.mkdir(parents=True)
    binary.touch()
    provenance = {"install_method": "binary-installer", "install_path": str(binary)}

    # curl fails (e.g. network down): writes nothing, exits non-zero. sh then
    # reads EOF and exits 0 — the update must NOT report success.
    curl_proc = mock_popen.return_value
    curl_proc.stdout = MagicMock()
    curl_proc.returncode = 22
    mock_run.return_value.returncode = 0

    with patch("topos.cli.update.detect_install_info") as mock_detect:
        from topos.cli.installation import InstallInfo

        mock_detect.return_value = InstallInfo(
            method="binary-installer",
            provenance=provenance,
        )
        runner = CliRunner()
        result = runner.invoke(cli, ["update"])
        assert result.exit_code != 0
        assert "Failed to download install.sh" in result.output


@patch("topos.cli.update.subprocess.run")
def test_update_package_manager_uv(mock_run):
    mock_run.return_value.returncode = 0

    with patch("topos.cli.update.detect_install_info") as mock_detect:
        from topos.cli.installation import InstallInfo

        mock_detect.return_value = InstallInfo(
            method="package-manager",
            installer="uv",
            update_cmd="uv pip install -U topos-mcp",
        )
        runner = CliRunner()
        result = runner.invoke(cli, ["update"])
        assert result.exit_code == 0
        mock_run.assert_called_once_with(
            ["uv", "pip", "install", "-U", "topos-mcp"],
            check=False,
        )


@patch("topos.cli.update.subprocess.run")
def test_update_package_manager_pip(mock_run):
    mock_run.return_value.returncode = 0

    with patch("topos.cli.update.detect_install_info") as mock_detect:
        from topos.cli.installation import InstallInfo

        mock_detect.return_value = InstallInfo(
            method="package-manager",
            installer="pip",
            update_cmd="pip install -U topos-mcp",
        )
        runner = CliRunner()
        result = runner.invoke(cli, ["update"])
        assert result.exit_code == 0
        mock_run.assert_called_once_with(
            ["pip", "install", "-U", "topos-mcp"],
            check=False,
        )


def test_update_source_prints_instructions():
    with patch("topos.cli.update.detect_install_info") as mock_detect:
        from topos.cli.installation import InstallInfo

        mock_detect.return_value = InstallInfo(
            method="source",
            update_cmd="git pull && uv pip install -e .",
        )
        runner = CliRunner()
        result = runner.invoke(cli, ["update"])
        assert result.exit_code == 0
        assert "editable/source installation" in result.output
        assert "git pull && uv pip install -e ." in result.output


@patch("topos.cli.update.__version__", "0.3.5")
@patch("topos.cli.update.latest_version_for_channel", return_value="0.3.6")
def test_update_check_outdated(mock_latest):
    with patch("topos.cli.update.detect_install_info") as mock_detect:
        from topos.cli.installation import InstallInfo

        mock_detect.return_value = InstallInfo(method="package-manager")
        runner = CliRunner()
        result = runner.invoke(cli, ["update", "--check"])
        assert result.exit_code == 1
        assert "Outdated: 0.3.5 → 0.3.6" in result.output


@patch("topos.cli.update.__version__", "0.3.6")
@patch("topos.cli.update.latest_version_for_channel", return_value="0.3.6")
def test_update_check_current(mock_latest):
    with patch("topos.cli.update.detect_install_info") as mock_detect:
        from topos.cli.installation import InstallInfo

        mock_detect.return_value = InstallInfo(method="binary-installer")
        runner = CliRunner()
        result = runner.invoke(cli, ["update", "--check"])
        assert result.exit_code == 0
        assert "Up to date: 0.3.6" in result.output


def test_update_unknown_lists_paths():
    with patch("topos.cli.update.detect_install_info") as mock_detect:
        from topos.cli.installation import InstallInfo

        mock_detect.return_value = InstallInfo(method="unknown")
        runner = CliRunner()
        result = runner.invoke(cli, ["update"])
        assert result.exit_code == 0
        assert "Could not determine install channel" in result.output
        assert "uv pip install -U topos-mcp" in result.output
