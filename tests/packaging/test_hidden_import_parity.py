"""PyInstaller build script parity with topos._LAZY_EXPORTS."""

from __future__ import annotations

import importlib
import subprocess
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
BUILD_SCRIPT = REPO_ROOT / "scripts" / "build-binary.sh"
LAZY_EXPORTS_SCRIPT = REPO_ROOT / "scripts" / "lazy_exports.py"


def _lazy_export_modules() -> set[str]:
    init = importlib.import_module("topos")
    lazy_exports: dict[str, tuple[str, str]] = init._LAZY_EXPORTS  # noqa: SLF001
    return {module for module, _ in lazy_exports.values()}


def test_lazy_exports_script_matches_package() -> None:
    completed = subprocess.run(
        ["uv", "run", "python", str(LAZY_EXPORTS_SCRIPT)],
        check=True,
        capture_output=True,
        text=True,
        cwd=REPO_ROOT,
    )
    from_script = {
        line.strip() for line in completed.stdout.splitlines() if line.strip()
    }
    assert from_script == _lazy_export_modules()


def test_build_script_derives_hidden_imports_from_lazy_exports() -> None:
    text = BUILD_SCRIPT.read_text(encoding="utf-8")
    assert "scripts/lazy_exports.py" in text
    assert "--copy-metadata topos-mcp" in text
    assert "--add-data topos/mcp/resources/content:topos/mcp/resources/content" in text
