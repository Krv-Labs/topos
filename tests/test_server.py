from __future__ import annotations

from pathlib import Path

import pytest

from topos import server


def test_compare_code_reports_validity_flags_on_parse_error() -> None:
    response = server.compare_code(
        source_code="x = 1\n",
        target_code="def broken(:\n    pass\n",
    )

    assert "error" in response
    assert response["source_valid"] is True
    assert response["target_valid"] is False


def test_assess_improvement_skips_distance_for_invalid_code() -> None:
    response = server.assess_improvement(
        current_code="x = 1\n",
        proposed_code="def broken(:\n    pass\n",
    )

    assert "analysis" in response
    assert response["analysis"]["distance_computed"] is False
    assert response["analysis"]["structural_distance"] is None
    assert response["analysis"]["similarity"] is None


@pytest.fixture
def isolated_file_root(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> Path:
    monkeypatch.setattr(server, "FILE_ACCESS_ROOT", tmp_path.resolve())
    return tmp_path


def test_evaluate_file_rejects_path_outside_allowed_root(
    isolated_file_root: Path,
) -> None:
    response = server.evaluate_file(str(Path(__file__).resolve()))

    assert "error" in response
    assert "Access denied" in response["error"]


def test_evaluate_file_rejects_non_file_path(isolated_file_root: Path) -> None:
    directory = isolated_file_root / "dir"
    directory.mkdir()

    response = server.evaluate_file(str(directory))

    assert "error" in response
    assert "not a file" in response["error"]


def test_evaluate_file_handles_decode_error(isolated_file_root: Path) -> None:
    bad_text_file = isolated_file_root / "bad.py"
    bad_text_file.write_bytes(b"\xff\xfe")

    response = server.evaluate_file(str(bad_text_file))

    assert "error" in response
    assert "not valid UTF-8" in response["error"]


def test_compare_files_reports_target_file_error(isolated_file_root: Path) -> None:
    source_file = isolated_file_root / "source.py"
    source_file.write_text("x = 1\n", encoding="utf-8")

    response = server.compare_files(
        source=str(source_file),
        target=str(isolated_file_root / "missing.py"),
    )

    assert "error" in response
    assert response["error"].startswith("Target file error:")
