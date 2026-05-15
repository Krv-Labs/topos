from __future__ import annotations

import os
from pathlib import Path
from topos.cli.evaluation import collect_files, result_to_row
from topos.cli.installation import load_provenance, prune_path_hints, detect_install_method
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies import Priority

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
        "path_hint_end": "# END TOPOS INSTALLER PATH"
    }
    
    prune_path_hints(provenance, dry_run=False)
    
    new_content = rc.read_text(encoding="utf-8")
    assert "topos" not in new_content
    assert "export PATH=foo" in new_content
    assert "export PATH=bar" in new_content
    assert "# BEGIN TOPOS INSTALLER PATH" not in new_content

def test_detect_install_method_pip(monkeypatch):
    from unittest.mock import MagicMock
    import importlib.metadata
    
    mock_dist = MagicMock()
    mock_dist.read_text.return_value = "pip"
    
    with monkeypatch.context() as m:
        m.setattr("importlib.metadata.distribution", lambda name: mock_dist)
        m.setattr("topos.cli.installation.load_provenance", lambda: None)
        
        method, prov, cmd = detect_install_method()
        assert method == "package-manager"
        assert cmd == "pip uninstall topos"
