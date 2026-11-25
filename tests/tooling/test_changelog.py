"""scripts/changelog.py: fragment checks and the release fold."""

from __future__ import annotations

import importlib.util
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "changelog.py"

_spec = importlib.util.spec_from_file_location("changelog", SCRIPT)
changelog = importlib.util.module_from_spec(_spec)
assert _spec.loader is not None
_spec.loader.exec_module(changelog)

HEAD = "# Changelog\n\nPreamble.\n\n## [Unreleased]\n\n"
OLD = "## [0.1.0] - 2026-01-01\n\n### Added\n\n- First.\n"


def _repo(
    tmp_path: Path, unreleased: str = "", fragments: dict[str, str] | None = None
):
    changes = tmp_path / "changes"
    changes.mkdir(parents=True)
    (changes / "README.md").write_text("how to\n", encoding="utf-8")
    for name, body in (fragments or {}).items():
        (changes / name).write_text(body, encoding="utf-8")
    log = tmp_path / "CHANGELOG.md"
    log.write_text(HEAD + unreleased + OLD, encoding="utf-8")
    return changes, log


def test_the_repository_passes_its_own_check() -> None:
    assert changelog.check(changelog.CHANGES, changelog.CHANGELOG) == []


def test_an_entry_under_unreleased_fails_and_names_the_fix(tmp_path: Path) -> None:
    changes, log = _repo(tmp_path, unreleased="### Fixed\n\n- A fix.\n\n")
    errors = changelog.check(changes, log)
    assert len(errors) == 1
    assert "[Unreleased]` has entries" in errors[0]
    assert "changes/<slug>.<kind>.md" in errors[0]


def test_malformed_fragments_fail(tmp_path: Path) -> None:
    changes, log = _repo(
        tmp_path,
        fragments={
            "no-kind.md": "- x\n",
            "bad.kind.md": "- x\n",
            "not-a-bullet.fixed.md": "Just prose.\n",
            "fine.added.md": "- ok\n",
        },
    )
    errors = changelog.check(changes, log)
    assert len(errors) == 3
    assert any("no-kind.md" in e for e in errors)
    assert any("unknown kind 'kind'" in e for e in errors)
    assert any("not-a-bullet.fixed.md" in e for e in errors)


def test_release_folds_fragments_by_heading_and_deletes_them(tmp_path: Path) -> None:
    changes, log = _repo(
        tmp_path,
        fragments={
            "zeta.fixed.md": "- Fixed one.\n",
            "alpha.added.md": "- Added one.\n  Wrapped line.\n",
            "beta.added.md": "- Added two.\n",
            "gamma.security.md": "- Secured.\n",
        },
    )
    folded = changelog.release(changes, log, "0.2.0", "2026-02-02")
    assert len(folded) == 4
    assert sorted(p.name for p in changes.iterdir()) == ["README.md"]
    assert log.read_text(encoding="utf-8") == (
        HEAD
        + "## [0.2.0] - 2026-02-02\n\n"
        + "### Added\n\n- Added one.\n  Wrapped line.\n- Added two.\n\n"
        + "### Fixed\n\n- Fixed one.\n\n"
        + "### Security\n\n- Secured.\n\n"
        + OLD
    )
    # The result still passes: [Unreleased] is empty again.
    assert changelog.check(changes, log) == []


def test_release_refuses_nothing_to_release_and_a_repeat_version(
    tmp_path: Path,
) -> None:
    changes, log = _repo(tmp_path)
    assert (
        changelog.main(
            ["--changes", str(changes), "--changelog", str(log), "release", "0.2.0"]
        )
        == 1
    )
    changes2, log2 = _repo(tmp_path / "b", fragments={"x.added.md": "- x\n"})
    assert (
        changelog.main(
            ["--changes", str(changes2), "--changelog", str(log2), "release", "0.1.0"]
        )
        == 1
    )
