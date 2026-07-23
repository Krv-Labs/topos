from __future__ import annotations

from importlib.metadata import PackageNotFoundError
from pathlib import Path

from topos.cli.evaluation import collect_files
from topos.cli.installation import (
    detect_install_info,
    detect_install_method,
    install_layout_notice_lines,
    load_provenance,
    prune_path_hints,
)


def test_collect_files(tmp_path: Path):
    d = tmp_path / "src"
    d.mkdir()
    (d / "a.py").write_text("", encoding="utf-8")
    (d / "b.txt").write_text("", encoding="utf-8")
    (d / "sub").mkdir()
    (d / "sub" / "c.py").write_text("", encoding="utf-8")

    # Non-recursive
    files = collect_files((str(d),), recursive=False, language="python")
    assert len(files) == 1
    assert files[0].name == "a.py"

    # Recursive
    files = collect_files((str(d),), recursive=True, language="python")
    assert len(files) == 2
    assert any(f.name == "a.py" for f in files)
    assert any(f.name == "c.py" for f in files)


def test_load_provenance_missing(monkeypatch):
    monkeypatch.setenv("TOPOS_PROVENANCE_FILE", "/non/existent/file")
    assert load_provenance() is None


def test_load_provenance_valid(tmp_path: Path, monkeypatch):
    prov_file = tmp_path / "prov"
    prov_file.write_text("key1=value1\n# comment\nkey2 = value2\n", encoding="utf-8")
    monkeypatch.setenv("TOPOS_PROVENANCE_FILE", str(prov_file))

    data = load_provenance()
    assert data == {"key1": "value1", "key2": "value2"}


def test_prune_path_hints(tmp_path: Path):
    rc = tmp_path / ".zshrc"
    content = (
        "export PATH=foo\n"
        "# BEGIN TOPOS INSTALLER PATH\n"
        "export PATH=topos\n"
        "# END TOPOS INSTALLER PATH\n"
        "export PATH=bar\n"
    )
    rc.write_text(content, encoding="utf-8")

    provenance = {
        "path_hint_file": str(rc),
        "path_hint_begin": "# BEGIN TOPOS INSTALLER PATH",
        "path_hint_end": "# END TOPOS INSTALLER PATH",
    }

    prune_path_hints(provenance, dry_run=False)

    new_content = rc.read_text(encoding="utf-8")
    assert "topos" not in new_content
    assert "export PATH=foo" in new_content
    assert "export PATH=bar" in new_content
    assert "# BEGIN TOPOS INSTALLER PATH" not in new_content


def test_detect_install_method_pip(monkeypatch):
    from unittest.mock import MagicMock

    mock_dist = MagicMock()

    def read_text(name: str) -> str:
        if name == "direct_url.json":
            raise FileNotFoundError
        if name == "INSTALLER":
            return "pip"
        raise FileNotFoundError

    mock_dist.read_text.side_effect = read_text

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", lambda name: mock_dist)
        m.setattr("topos.cli.installation.load_provenance", lambda: None)

        method, prov, cmd = detect_install_method()
        assert method == "package-manager"
        assert cmd == "pip uninstall topos-mcp"


def test_detect_install_method_uv(monkeypatch):
    from unittest.mock import MagicMock

    mock_dist = MagicMock()

    def read_text(name: str) -> str:
        if name == "direct_url.json":
            raise FileNotFoundError
        if name == "INSTALLER":
            return "uv"
        raise FileNotFoundError

    mock_dist.read_text.side_effect = read_text

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", lambda name: mock_dist)
        m.setattr("topos.cli.installation.load_provenance", lambda: None)

        info = detect_install_info()
        assert info.method == "package-manager"
        assert info.installer == "uv"
        assert info.update_cmd == "uv pip install -U topos-mcp"


def test_detect_install_method_editable(monkeypatch):
    from unittest.mock import MagicMock

    mock_dist = MagicMock()
    mock_dist.read_text.return_value = (
        '{"url": "file:///src/topos", "dir_info": {"editable": true}}'
    )

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", lambda name: mock_dist)
        m.setattr("topos.cli.installation.load_provenance", lambda: None)

        info = detect_install_info()
        assert info.method == "source"
        assert info.update_cmd == "git pull && uv pip install -e ."


def test_detect_install_method_editable_over_stale_binary_provenance(monkeypatch):
    from unittest.mock import MagicMock

    mock_dist = MagicMock()
    mock_dist.read_text.return_value = (
        '{"url": "file:///src/topos", "dir_info": {"editable": true}}'
    )
    stale_provenance = {
        "install_method": "binary-installer",
        "install_path": "/home/user/.local/bin/topos",
    }

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", lambda name: mock_dist)
        m.setattr("topos.cli.installation.load_provenance", lambda: stale_provenance)

        info = detect_install_info()
        assert info.method == "source"


def test_detect_install_method_pip_over_stale_binary_provenance(monkeypatch):
    from unittest.mock import MagicMock

    mock_dist = MagicMock()

    def read_text(name: str) -> str:
        if name == "direct_url.json":
            raise FileNotFoundError
        if name == "INSTALLER":
            return "pip"
        raise FileNotFoundError

    mock_dist.read_text.side_effect = read_text
    stale_provenance = {
        "install_method": "binary-installer",
        "install_path": "/home/user/.local/bin/topos",
    }

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", lambda name: mock_dist)
        m.setattr("topos.cli.installation.load_provenance", lambda: stale_provenance)

        info = detect_install_info()
        assert info.method == "package-manager"
        assert info.installer == "pip"


def test_detect_install_method_binary_without_python_package(monkeypatch):
    def raise_not_found(name: str):
        raise PackageNotFoundError

    stale_provenance = {
        "install_method": "binary-installer",
        "install_path": "/home/user/.local/bin/topos",
    }

    with monkeypatch.context() as m:
        m.setattr(
            "importlib.metadata.distribution",
            raise_not_found,
        )
        m.setattr("topos.cli.installation.load_provenance", lambda: stale_provenance)
        m.setattr(
            "topos.cli.installation.active_executable",
            lambda: Path("/home/user/.local/bin/topos"),
        )
        m.delenv("HOMEBREW_PREFIX", raising=False)

        info = detect_install_info()
        assert info.method == "binary-installer"
        assert info.provenance == stale_provenance


def test_detect_install_method_homebrew_cellar(monkeypatch):
    def raise_not_found(name: str):
        raise PackageNotFoundError

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", raise_not_found)
        m.setattr("topos.cli.installation.load_provenance", lambda: None)
        m.setattr(
            "topos.cli.installation.active_executable",
            lambda: Path("/opt/homebrew/Cellar/topos/0.3.12/bin/topos"),
        )
        m.delenv("HOMEBREW_PREFIX", raising=False)

        info = detect_install_info()
        assert info.method == "homebrew"
        assert info.installer == "brew"
        assert info.update_cmd == "brew upgrade topos"
        assert info.uninstall_cmd == "brew uninstall topos"


def test_detect_install_method_homebrew_intel_cellar(monkeypatch):
    def raise_not_found(name: str):
        raise PackageNotFoundError

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", raise_not_found)
        m.setattr("topos.cli.installation.load_provenance", lambda: None)
        m.setattr(
            "topos.cli.installation.active_executable",
            lambda: Path("/usr/local/Cellar/topos/0.3.12/bin/topos"),
        )
        m.delenv("HOMEBREW_PREFIX", raising=False)

        assert detect_install_info().method == "homebrew"


def test_detect_install_method_homebrew_linux_cellar(monkeypatch):
    def raise_not_found(name: str):
        raise PackageNotFoundError

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", raise_not_found)
        m.setattr("topos.cli.installation.load_provenance", lambda: None)
        m.setattr(
            "topos.cli.installation.active_executable",
            lambda: Path("/home/linuxbrew/.linuxbrew/Cellar/topos/0.3.12/bin/topos"),
        )
        m.delenv("HOMEBREW_PREFIX", raising=False)

        assert detect_install_info().method == "homebrew"


def test_detect_install_method_homebrew_prefix_env(monkeypatch, tmp_path: Path):
    def raise_not_found(name: str):
        raise PackageNotFoundError

    prefix = tmp_path / "linuxbrew"
    binary = prefix / "bin" / "topos"

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", raise_not_found)
        m.setattr("topos.cli.installation.load_provenance", lambda: None)
        m.setattr("topos.cli.installation.active_executable", lambda: binary)
        m.setenv("HOMEBREW_PREFIX", str(prefix))

        info = detect_install_info()
        assert info.method == "homebrew"


def test_detect_install_method_ignores_unrelated_cellar(monkeypatch, tmp_path: Path):
    def raise_not_found(name: str):
        raise PackageNotFoundError

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", raise_not_found)
        m.setattr("topos.cli.installation.load_provenance", lambda: None)
        m.setattr(
            "topos.cli.installation.active_executable",
            lambda: tmp_path / "fake" / "Cellar" / "topos" / "0.3.12" / "bin" / "topos",
        )
        m.delenv("HOMEBREW_PREFIX", raising=False)

        assert detect_install_info().method == "unknown"


def test_detect_install_method_binary_in_usr_local_bin(monkeypatch):
    def raise_not_found(name: str):
        raise PackageNotFoundError

    provenance = {
        "install_method": "binary-installer",
        "install_path": "/usr/local/bin/topos",
    }

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", raise_not_found)
        m.setattr("topos.cli.installation.load_provenance", lambda: provenance)
        m.setattr(
            "topos.cli.installation.active_executable",
            lambda: Path("/usr/local/bin/topos"),
        )
        m.delenv("HOMEBREW_PREFIX", raising=False)

        info = detect_install_info()
        assert info.method == "binary-installer"
        assert info.provenance == provenance


def test_detect_install_method_ignores_malformed_homebrew_prefix(monkeypatch):
    def raise_not_found(name: str):
        raise PackageNotFoundError

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", raise_not_found)
        m.setattr("topos.cli.installation.load_provenance", lambda: None)
        m.setattr(
            "topos.cli.installation.active_executable",
            lambda: Path("/opt/homebrew/Cellar/topos/0.3.12/bin/topos"),
        )
        m.setenv("HOMEBREW_PREFIX", "~definitely_no_such_topos_user_200")

        assert detect_install_info().method == "homebrew"


def test_detect_install_method_homebrew_wins_over_stale_binary_provenance(monkeypatch):
    def raise_not_found(name: str):
        raise PackageNotFoundError

    stale_provenance = {
        "install_method": "binary-installer",
        "install_path": "/home/user/.local/bin/topos",
    }

    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", raise_not_found)
        m.setattr("topos.cli.installation.load_provenance", lambda: stale_provenance)
        m.setattr(
            "topos.cli.installation.active_executable",
            lambda: Path("/opt/homebrew/Cellar/topos/0.3.12/bin/topos"),
        )
        m.delenv("HOMEBREW_PREFIX", raising=False)

        info = detect_install_info()
        assert info.method == "homebrew"
        assert info.uninstall_cmd == "brew uninstall topos"


def test_channel_label_homebrew():
    from topos.cli.installation import InstallInfo, channel_label

    assert channel_label(InstallInfo(method="homebrew")) == "Homebrew"


def test_install_layout_notice_none_for_single_install(monkeypatch):
    active = Path("/venv/bin/topos")

    with monkeypatch.context() as m:
        m.setattr("topos.cli.installation.active_executable", lambda: active)
        m.setattr(
            "topos.cli.installation.find_topos_executables_on_path", lambda: [active]
        )
        m.setattr("topos.cli.installation.load_provenance", lambda: None)
        m.setattr("shutil.which", lambda name: str(active))

        assert install_layout_notice_lines() is None


def test_install_layout_notice_multiple_on_path(monkeypatch, tmp_path):
    from topos.cli.installation import InstallInfo

    # Use tmp_path so paths are already resolved — avoids macOS /home symlink issues
    # when comparing path_bins (resolved) against shutil.which output (also resolved).
    active = tmp_path / "venv" / "bin" / "topos"
    other = tmp_path / "local" / "bin" / "topos"

    with monkeypatch.context() as m:
        m.setattr("topos.cli.installation.active_executable", lambda: active)
        m.setattr(
            "topos.cli.installation.find_topos_executables_on_path",
            lambda: [other, active],
        )
        m.setattr("topos.cli.installation.load_provenance", lambda: None)
        m.setattr("shutil.which", lambda name: str(other))
        m.setattr(
            "topos.cli.installation.detect_install_info",
            lambda: InstallInfo(method="source"),
        )

        lines = install_layout_notice_lines()
        assert lines is not None
        joined = "\n".join(lines)
        assert "Multiple Topos installations detected." in joined
        assert str(active) in joined
        assert "editable source checkout" in joined
        assert "runs when you type `topos`" in joined
        assert str(other) in joined
        # The PATH-default binary must not also appear as "Also on PATH".
        assert joined.count(str(other)) == 1


def test_install_layout_notice_stale_binary_provenance(monkeypatch):
    from topos.cli.installation import InstallInfo

    active = Path("/src/topos/.venv/bin/topos")
    stale_binary = Path("/home/user/.local/bin/topos")
    stale_provenance = {
        "install_method": "binary-installer",
        "install_path": str(stale_binary),
    }

    def fake_exists(self: Path) -> bool:
        return self == stale_binary

    with monkeypatch.context() as m:
        m.setattr("topos.cli.installation.active_executable", lambda: active)
        m.setattr(
            "topos.cli.installation.find_topos_executables_on_path",
            lambda: [active],
        )
        m.setattr("topos.cli.installation.load_provenance", lambda: stale_provenance)
        m.setattr("shutil.which", lambda name: str(active))
        m.setattr(
            "topos.cli.installation.detect_install_info",
            lambda: InstallInfo(method="source"),
        )
        m.setattr(Path, "exists", fake_exists, raising=False)

        lines = install_layout_notice_lines()
        assert lines is not None
        joined = "\n".join(lines)
        assert f"Active: {active}" in joined
        assert "Binary installer record:" in joined
        assert ".local/bin/topos" in joined
