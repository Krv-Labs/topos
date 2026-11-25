"""scripts/render_homebrew_formula.py renders the tap formula from the template."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "render_homebrew_formula.py"

_spec = importlib.util.spec_from_file_location("render_homebrew_formula", SCRIPT)
render_mod = importlib.util.module_from_spec(_spec)
assert _spec.loader is not None
_spec.loader.exec_module(render_mod)

HASHES = {
    "topos-macos-arm64": "a" * 64,
    "topos-linux-amd64": "b" * 64,
    "topos-linux-arm64": "c" * 64,
}


def _checksums(tmp_path: Path, rows: dict[str, str]) -> Path:
    path = tmp_path / "checksums.txt"
    path.write_text(
        "".join(f"{sha}  {name}\n" for name, sha in rows.items()), encoding="utf-8"
    )
    return path


def test_renders_each_url_with_its_assets_hash(tmp_path: Path) -> None:
    out = tmp_path / "topos.rb"
    code = render_mod.main(
        [
            "--version",
            "1.2.3",
            "--checksums",
            str(_checksums(tmp_path, HASHES)),
            "-o",
            str(out),
        ]
    )
    assert code == 0
    formula = out.read_text(encoding="utf-8")
    assert "{{" not in formula
    lines = formula.splitlines()
    for asset, sha in HASHES.items():
        url = (
            f'url "https://github.com/Krv-Labs/topos/releases/download/v1.2.3/{asset}"'
        )
        at = next(i for i, line in enumerate(lines) if line.strip() == url)
        assert lines[at + 1].strip() == f'sha256 "{sha}"', asset


def test_placeholder_render_fills_everything(tmp_path: Path) -> None:
    out = tmp_path / "topos.rb"
    assert render_mod.main(["--placeholder", "-o", str(out)]) == 0
    assert "{{" not in out.read_text(encoding="utf-8")


def test_an_unknown_placeholder_is_refused(tmp_path: Path) -> None:
    template = tmp_path / "t.rb"
    template.write_text(
        'url "{{VERSION}}"\nsha256 "{{SHA_NEW_TARGET}}"\n', encoding="utf-8"
    )
    code = render_mod.main(
        ["--placeholder", "--template", str(template), "-o", str(tmp_path / "out.rb")]
    )
    assert code == 1


def test_a_missing_asset_in_checksums_is_refused(tmp_path: Path) -> None:
    rows = dict(HASHES)
    del rows["topos-linux-arm64"]
    with pytest.raises(SystemExit):
        render_mod.main(
            [
                "--version",
                "1.2.3",
                "--checksums",
                str(_checksums(tmp_path, rows)),
                "-o",
                str(tmp_path / "o.rb"),
            ]
        )
