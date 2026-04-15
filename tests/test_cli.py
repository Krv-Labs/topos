from click.testing import CliRunner

from topos.main import cli


def test_cli_help():
    runner = CliRunner()
    result = runner.invoke(cli, ["--help"])
    assert result.exit_code == 0
    assert "Topos: Category-theoretic code quality evaluation" in result.output


def test_evaluate_command(tmp_path):
    runner = CliRunner()

    # Create a dummy python file
    p = tmp_path / "test_file.py"
    p.write_text("def my_func():\n    pass\n", encoding="utf-8")

    result = runner.invoke(cli, ["evaluate", str(p)])
    assert result.exit_code == 0
    assert "test_file.py" in result.output
    assert "Overall:" in result.output


def test_evaluate_command_json(tmp_path):
    runner = CliRunner()
    p = tmp_path / "test_file.py"
    p.write_text("def my_func():\n    pass\n", encoding="utf-8")

    result = runner.invoke(cli, ["evaluate", str(p), "--json"])
    assert result.exit_code == 0
    assert '"results":' in result.output


def test_compare_command(tmp_path):
    runner = CliRunner()
    p1 = tmp_path / "file1.py"
    p1.write_text("x = 1\n", encoding="utf-8")

    p2 = tmp_path / "file2.py"
    p2.write_text("y = 2\n", encoding="utf-8")

    result = runner.invoke(cli, ["compare", str(p1), str(p2), "--verbose"])
    assert result.exit_code == 0
    assert "Edit Distance:" in result.output
    assert "Operations:" in result.output


def test_inspect_command(tmp_path):
    runner = CliRunner()
    p = tmp_path / "inspect_file.py"
    p.write_text("def func(x):\n    return x + 1\n", encoding="utf-8")

    result = runner.invoke(cli, ["inspect", str(p)])
    assert result.exit_code == 0
    assert "Classification" in result.output
    assert "Metrics" in result.output
    assert "func: 1" in result.output


def test_evaluate_no_paths():
    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate"])
    # Because of the click argument decorator `paths` not having default empty,
    # click might intercept it before our code. Wait, click nargs=-1 does not intercept.
    assert result.exit_code == 1


def test_evaluate_directory(tmp_path):
    runner = CliRunner()
    d = tmp_path / "src"
    d.mkdir()
    p = d / "test_file.py"
    p.write_text("def my_func():\n    pass\n", encoding="utf-8")

    result = runner.invoke(cli, ["evaluate", str(d), "-r"])
    assert result.exit_code == 0
    assert "test_file.py" in result.output


def test_evaluate_prints_fallback_when_overall_empty(tmp_path, monkeypatch):
    runner = CliRunner()
    p = tmp_path / "test_file.py"
    p.write_text("def my_func():\n    return 1\n", encoding="utf-8")

    from topos import main as main_module

    monkeypatch.setattr(
        main_module.SubobjectClassifier,
        "combine_dimensions",
        lambda self, _results: {},
    )

    result = runner.invoke(cli, ["evaluate", str(p)])

    assert result.exit_code == 0
    assert "Overall:" in result.output
    assert "no evaluable dimensions" in result.output


def test_evaluate_fails_when_depgraph_requested_but_unavailable(tmp_path):
    runner = CliRunner()
    source = tmp_path / "test_file.py"
    source.write_text("def my_func():\n    return 1\n", encoding="utf-8")
    gitnexus_dir = tmp_path / ".gitnexus"
    gitnexus_dir.mkdir()

    result = runner.invoke(
        cli,
        ["evaluate", str(source), "--gitnexus-dir", str(gitnexus_dir)],
    )

    assert result.exit_code == 1
    assert "Failed to build depgraph" in result.output
    assert "Overall:" not in result.output


def test_inspect_fails_when_depgraph_requested_but_unavailable(tmp_path):
    runner = CliRunner()
    source = tmp_path / "inspect_file.py"
    source.write_text("def func(x):\n    return x + 1\n", encoding="utf-8")
    gitnexus_dir = tmp_path / ".gitnexus"
    gitnexus_dir.mkdir()

    result = runner.invoke(
        cli,
        ["inspect", str(source), "--gitnexus-dir", str(gitnexus_dir)],
    )

    assert result.exit_code == 1
    assert "Failed to build depgraph" in result.output


def test_uninstall_binary_installer_dry_run(tmp_path, monkeypatch):
    runner = CliRunner()
    bin_path = tmp_path / "bin" / "topos"
    bin_path.parent.mkdir(parents=True)
    bin_path.write_text("binary", encoding="utf-8")

    provenance = tmp_path / "install-provenance"
    provenance.write_text(
        "\n".join(
            [
                "install_method=binary-installer",
                f"install_path={bin_path}",
                "install_version=v1.2.3",
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    monkeypatch.setenv("TOPOS_PROVENANCE_FILE", str(provenance))

    result = runner.invoke(cli, ["uninstall", "--dry-run", "--yes"])
    assert result.exit_code == 0
    assert "Would remove binary" in result.output
    assert bin_path.exists()


def test_uninstall_binary_installer_idempotent(tmp_path, monkeypatch):
    runner = CliRunner()
    bin_path = tmp_path / "bin" / "topos"
    bin_path.parent.mkdir(parents=True)
    provenance = tmp_path / "install-provenance"
    provenance.write_text(
        "\n".join(
            [
                "install_method=binary-installer",
                f"install_path={bin_path}",
                "install_version=v1.2.3",
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    monkeypatch.setenv("TOPOS_PROVENANCE_FILE", str(provenance))

    result = runner.invoke(cli, ["uninstall", "--yes"])
    assert result.exit_code == 0
    assert "already removed" in result.output


def test_uninstall_binary_installer_removes_provenance(tmp_path, monkeypatch):
    runner = CliRunner()
    bin_path = tmp_path / "bin" / "topos"
    bin_path.parent.mkdir(parents=True)
    bin_path.write_text("binary", encoding="utf-8")

    provenance = tmp_path / "install-provenance"
    provenance.write_text(
        "\n".join(
            [
                "install_method=binary-installer",
                f"install_path={bin_path}",
                "install_version=v1.2.3",
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    monkeypatch.setenv("TOPOS_PROVENANCE_FILE", str(provenance))

    result = runner.invoke(cli, ["uninstall", "--yes"])
    assert result.exit_code == 0
    assert not provenance.exists()
    assert "Removed provenance record" in result.output


def test_uninstall_binary_installer_non_file_path(tmp_path, monkeypatch):
    runner = CliRunner()
    bin_path = tmp_path / "bin" / "topos"
    bin_path.mkdir(parents=True)

    provenance = tmp_path / "install-provenance"
    provenance.write_text(
        "\n".join(
            [
                "install_method=binary-installer",
                f"install_path={bin_path}",
                "install_version=v1.2.3",
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    monkeypatch.setenv("TOPOS_PROVENANCE_FILE", str(provenance))

    result = runner.invoke(cli, ["uninstall", "--yes"])
    assert result.exit_code == 1
    assert "Refusing to remove non-file path" in result.output


def test_uninstall_package_manager_prints_command(monkeypatch):
    runner = CliRunner()

    from topos import main as main_module

    monkeypatch.setattr(
        main_module,
        "_detect_install_method",
        lambda: ("package-manager", None, "uv pip uninstall topos"),
    )
    result = runner.invoke(cli, ["uninstall"])

    assert result.exit_code == 0
    assert "Detected package-manager installation." in result.output
    assert "uv pip uninstall topos" in result.output


def test_uninstall_prune_path_hints(tmp_path, monkeypatch):
    runner = CliRunner()
    bin_path = tmp_path / "bin" / "topos"
    bin_path.parent.mkdir(parents=True)
    bin_path.write_text("binary", encoding="utf-8")

    rc_file = tmp_path / ".bashrc"
    rc_file.write_text(
        "\n".join(
            [
                "export LANG=en_US.UTF-8",
                "# BEGIN TOPOS INSTALLER PATH",
                "# Added by Topos installer",
                'export PATH="/tmp/topos-bin:$PATH"',
                "# END TOPOS INSTALLER PATH",
                "alias ll='ls -la'",
            ]
        )
        + "\n",
        encoding="utf-8",
    )

    provenance = tmp_path / "install-provenance"
    provenance.write_text(
        "\n".join(
            [
                "install_method=binary-installer",
                f"install_path={bin_path}",
                f"path_hint_file={rc_file}",
                "path_hint_begin=# BEGIN TOPOS INSTALLER PATH",
                "path_hint_end=# END TOPOS INSTALLER PATH",
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    monkeypatch.setenv("TOPOS_PROVENANCE_FILE", str(provenance))

    result = runner.invoke(cli, ["uninstall", "--yes", "--prune-path-hints"])
    assert result.exit_code == 0
    updated = rc_file.read_text(encoding="utf-8")
    assert "BEGIN TOPOS INSTALLER PATH" not in updated
    assert "END TOPOS INSTALLER PATH" not in updated
    assert "export LANG=en_US.UTF-8" in updated
    assert "alias ll='ls -la'" in updated


def test_uninstall_prune_path_hints_requires_matching_markers(tmp_path, monkeypatch):
    runner = CliRunner()
    bin_path = tmp_path / "bin" / "topos"
    bin_path.parent.mkdir(parents=True)
    bin_path.write_text("binary", encoding="utf-8")

    rc_file = tmp_path / ".bashrc"
    rc_file.write_text(
        "\n".join(
            [
                "export LANG=en_US.UTF-8",
                "# BEGIN TOPOS INSTALLER PATH",
                "# Added by Topos installer",
                'export PATH="/tmp/topos-bin:$PATH"',
            ]
        )
        + "\n",
        encoding="utf-8",
    )

    provenance = tmp_path / "install-provenance"
    provenance.write_text(
        "\n".join(
            [
                "install_method=binary-installer",
                f"install_path={bin_path}",
                f"path_hint_file={rc_file}",
                "path_hint_begin=# BEGIN TOPOS INSTALLER PATH",
                "path_hint_end=# END TOPOS INSTALLER PATH",
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    monkeypatch.setenv("TOPOS_PROVENANCE_FILE", str(provenance))

    result = runner.invoke(cli, ["uninstall", "--yes", "--prune-path-hints"])
    assert result.exit_code == 0
    assert "Malformed PATH hint block in" in result.output.replace("\n", " ")
    updated = rc_file.read_text(encoding="utf-8")
    assert "# BEGIN TOPOS INSTALLER PATH" in updated
    assert "Malformed" not in updated


def test_detect_install_method_falls_back_when_installer_metadata_missing(tmp_path, monkeypatch):
    monkeypatch.setenv("TOPOS_PROVENANCE_FILE", str(tmp_path / "nonexistent"))
    from topos import main as main_module

    class DummyDist:
        def read_text(self, _filename: str) -> str:
            raise FileNotFoundError("missing")

    monkeypatch.setattr(
        main_module.importlib.metadata,
        "distribution",
        lambda _name: DummyDist(),
    )

    assert main_module._detect_install_method() == (
        "package-manager",
        None,
        "pip uninstall topos",
    )
