import pytest
from pathlib import Path

from topos.server import (
    evaluate_code,
    evaluate_file,
    compare_code,
    compare_files,
    assess_improvement,
    inspect_code,
    __version__
)

def test_server_version():
    assert isinstance(__version__, str)

def test_evaluate_code():
    code = "def foo(): pass"
    res = evaluate_code(code)
    assert "evaluation" in res
    assert "symbol" in res
    assert res.get("is_valid") is True

def test_evaluate_code_error():
    # evaluate_code won't raise Exception on parse since tree_sitter is robust,
    # but let's test language mismatch or something to trigger error.
    res = evaluate_code("x = 1", language="unknown")
    assert "error" in res

def test_evaluate_file(tmp_path):
    p = tmp_path / "test.py"
    p.write_text("x = 1", encoding="utf-8")
    
    res = evaluate_file(str(p))
    assert "evaluation" in res

def test_evaluate_file_not_found():
    res = evaluate_file("does_not_exist.py")
    assert "error" in res

def test_compare_code():
    source = "x = 1"
    target = "y = 2"
    res = compare_code(source, target)
    assert "raw_distance" in res
    assert "operations" in res

def test_compare_code_error():
    res = compare_code("x=1", "y=2", language="unknown")
    assert "error" in res

def test_compare_files(tmp_path):
    p1 = tmp_path / "1.py"
    p2 = tmp_path / "2.py"
    p1.write_text("x = 1", encoding="utf-8")
    p2.write_text("y = 2", encoding="utf-8")
    
    res = compare_files(str(p1), str(p2))
    assert "raw_distance" in res

def test_compare_files_not_found(tmp_path):
    res1 = compare_files("missing.py", "target.py")
    assert "error" in res1
    
    p = tmp_path / "exist.py"
    p.write_text("x", encoding="utf-8")
    res2 = compare_files(str(p), "missing.py")
    assert "error" in res2

def test_assess_improvement():
    curr = "def f(x):\n    pass"
    prop = "def f(x"
    
    res = assess_improvement(curr, prop)
    assert "status" in res
    assert "current" in res
    assert "proposed" in res
    assert "REGRESSION" in res["status"]

def test_assess_improvement_error():
    res = assess_improvement("x", "y", language="unknown")
    assert "error" in res

def test_inspect_code():
    code = "def add(a, b): return a + b"
    res = inspect_code(code)
    assert "evaluation" in res
    assert "ast_metrics" in res
    assert "functions" in res
    assert "entropy_details" in res

def test_inspect_code_error():
    res = inspect_code("x", language="unknown")
    assert "error" in res
