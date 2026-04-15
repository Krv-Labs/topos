from __future__ import annotations

from pathlib import Path

import pytest

from topos import server


@pytest.fixture
def isolated_file_root(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> Path:
    """Fixture to isolate the FILE_ACCESS_ROOT for testing path safety."""
    monkeypatch.setattr(server, "FILE_ACCESS_ROOT", tmp_path.resolve())
    return tmp_path


# --- Metadata & Basic Info ---


def test_server_version() -> None:
    assert isinstance(server.__version__, str)


# --- Code Evaluation Tools ---


def test_evaluate_code_happy_path() -> None:
    code = "def foo(): pass"
    res = server.evaluate_code(code)
    assert "dimensions" in res
    assert "lattice_element" in res
    assert "scores" in res
    assert "guidance" in res
    assert res.get("is_parseable") is True


def test_evaluate_code_error_on_unknown_language() -> None:
    res = server.evaluate_code("x = 1", language="unknown")
    assert "error" in res


def test_inspect_code_happy_path() -> None:
    code = "def add(a, b): return a + b"
    res = server.inspect_code(code)
    assert "dimensions" in res
    assert "raw_metrics" in res
    assert "functions" in res
    assert "entropy_details" in res


# --- Structural Comparison Tools ---


def test_compare_code_happy_path() -> None:
    res = server.compare_code(source_code="x = 1", target_code="y = 2")
    assert "raw_distance" in res
    assert "operations" in res
    assert res["source_valid"] is True
    assert res["target_valid"] is True


def test_compare_code_reports_validity_flags_on_parse_error() -> None:
    response = server.compare_code(
        source_code="x = 1\n",
        target_code="def broken(:\n    pass\n",
    )
    assert "error" in response
    assert response["source_valid"] is True
    assert response["target_valid"] is False


def test_assess_improvement_regression() -> None:
    curr = "def f(x):\n    pass"
    prop = "def f(x"  # Syntactically broken

    res = server.assess_improvement(curr, prop)
    assert "status" in res
    assert "REGRESSION" in res["status"]
    assert res["analysis"]["distance_computed"] is False
    assert res["analysis"]["structural_distance"] is None


# --- File-Based Tools & Safety ---


def test_evaluate_file_happy_path(isolated_file_root: Path) -> None:
    p = isolated_file_root / "test.py"
    p.write_text("x = 1", encoding="utf-8")

    res = server.evaluate_file(str(p))
    assert "dimensions" in res


def test_evaluate_file_rejects_path_outside_allowed_root(
    isolated_file_root: Path,
) -> None:
    # Attempt to read a file outside the tmp_path-based root
    response = server.evaluate_file(str(Path(__file__).resolve()))

    assert "error" in response
    assert "Access denied" in response["error"]


def test_evaluate_file_errors(isolated_file_root: Path) -> None:
    # Test Not Found
    res_nf = server.evaluate_file(str(isolated_file_root / "missing.py"))
    assert "File not found" in res_nf["error"]

    # Test Directory
    directory = isolated_file_root / "dir"
    directory.mkdir()
    res_dir = server.evaluate_file(str(directory))
    assert "not a file" in res_dir["error"]

    # Test Decode Error
    bad_file = isolated_file_root / "bad.py"
    bad_file.write_bytes(b"\xff\xfe")
    res_dec = server.evaluate_file(str(bad_file))
    assert "not valid UTF-8" in res_dec["error"]


def test_compare_files_happy_path(isolated_file_root: Path) -> None:
    p1 = isolated_file_root / "1.py"
    p2 = isolated_file_root / "2.py"
    p1.write_text("x = 1", encoding="utf-8")
    p2.write_text("y = 2", encoding="utf-8")

    res = server.compare_files(str(p1), str(p2))
    assert "raw_distance" in res


def test_compare_files_reports_file_errors(isolated_file_root: Path) -> None:
    source_file = isolated_file_root / "source.py"
    source_file.write_text("x = 1\n", encoding="utf-8")

    # Source error
    res_src = server.compare_files("missing.py", "target.py")
    assert "Source file error" in res_src["error"]

    # Target error
    res_tgt = server.compare_files(str(source_file), "missing.py")
    assert "Target file error" in res_tgt["error"]
