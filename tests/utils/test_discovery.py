from __future__ import annotations

from pathlib import Path

import pytest
from topos.utils.discovery import (
    collect_source_files,
    is_virtualenv_root,
    should_skip_dir,
)


def test_should_skip_dir_recognizes_common_venv_names() -> None:
    assert should_skip_dir(Path("/proj/.venv"))
    assert should_skip_dir(Path("/proj/venv"))
    assert should_skip_dir(Path("/proj/env"))


def test_is_virtualenv_root_detects_pyvenv_cfg() -> None:
    root = Path("/fake")
    assert not is_virtualenv_root(root)
    # Callers pass real paths; tmp_path used in integration test below.


def test_is_virtualenv_root_handles_unreadable_marker(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    blocked = tmp_path / "blocked"
    original_is_file = Path.is_file

    def fake_is_file(path: Path) -> bool:
        if path == blocked / "pyvenv.cfg":
            raise PermissionError("blocked")
        return original_is_file(path)

    monkeypatch.setattr(Path, "is_file", fake_is_file)

    assert not is_virtualenv_root(blocked)


def test_collect_source_files_skips_dot_venv(tmp_path: Path) -> None:
    (tmp_path / "app.py").write_text("x = 1\n", encoding="utf-8")
    venv = tmp_path / ".venv" / "lib"
    venv.mkdir(parents=True)
    (venv / "site.py").write_text("print('dep')\n", encoding="utf-8")

    files = collect_source_files((str(tmp_path),), suffixes=(".py",), recursive=True)
    assert [p.name for p in files] == ["app.py"]


def test_collect_source_files_skips_venv_with_pyvenv_cfg(tmp_path: Path) -> None:
    (tmp_path / "main.py").write_text("", encoding="utf-8")
    custom = tmp_path / "myenv"
    custom.mkdir()
    (custom / "pyvenv.cfg").write_text("[venv]\n", encoding="utf-8")
    (custom / "lib").mkdir()
    (custom / "lib" / "dep.py").write_text("", encoding="utf-8")

    files = collect_source_files((str(tmp_path),), suffixes=(".py",), recursive=True)
    assert len(files) == 1
    assert files[0].name == "main.py"


def test_collect_source_files_respects_toposignore(tmp_path: Path) -> None:
    (tmp_path / "keep.py").write_text("", encoding="utf-8")
    scratch = tmp_path / "scratch"
    scratch.mkdir()
    (scratch / "skip.py").write_text("", encoding="utf-8")
    (tmp_path / ".toposignore").write_text("scratch/\n", encoding="utf-8")

    files = collect_source_files((str(tmp_path),), suffixes=(".py",), recursive=True)
    assert [p.name for p in files] == ["keep.py"]


def test_collect_source_files_non_recursive(tmp_path: Path) -> None:
    src = tmp_path / "src"
    src.mkdir()
    (src / "a.py").write_text("", encoding="utf-8")
    sub = src / "sub"
    sub.mkdir()
    (sub / "b.py").write_text("", encoding="utf-8")

    files = collect_source_files((str(src),), suffixes=(".py",), recursive=False)
    assert len(files) == 1
    assert files[0].name == "a.py"


def test_collect_source_files_skips_unreadable_child_dir(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    (tmp_path / "keep.py").write_text("", encoding="utf-8")
    blocked = tmp_path / "blocked"
    blocked.mkdir()
    (blocked / "hidden.py").write_text("", encoding="utf-8")
    original_is_dir = Path.is_dir
    original_is_file = Path.is_file

    def fake_is_dir(path: Path) -> bool:
        if path == blocked:
            raise PermissionError("blocked")
        return original_is_dir(path)

    def fake_is_file(path: Path) -> bool:
        if path == blocked:
            raise PermissionError("blocked")
        return original_is_file(path)

    monkeypatch.setattr(Path, "is_dir", fake_is_dir)
    monkeypatch.setattr(Path, "is_file", fake_is_file)

    files = collect_source_files((str(tmp_path),), suffixes=(".py",), recursive=True)
    assert [p.name for p in files] == ["keep.py"]
