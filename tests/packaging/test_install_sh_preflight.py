"""Unit tests for install.sh multi-install preflight helpers."""

from __future__ import annotations

import os
import stat
import subprocess
import textwrap
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
INSTALL_SH = REPO_ROOT / "install.sh"


def _write_executable(path: Path, body: str = "#!/bin/sh\necho topos\n") -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


def _run_install_helpers(
    script: str,
    *,
    env: dict[str, str] | None = None,
    timeout: float = 10.0,
) -> subprocess.CompletedProcess[str]:
    """Source install.sh helpers (no main) and run a bash snippet."""
    full = textwrap.dedent(
        f"""\
        set -euo pipefail
        TOPOS_SKIP_MAIN=1
        # shellcheck source=/dev/null
        source "{INSTALL_SH}"
        {script}
        """
    )
    # Minimal env so host PATH/Homebrew installs do not leak into discovery.
    merged = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(Path.home()),
        "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
        "TOPOS_SKIP_MAIN": "1",
    }
    if env:
        merged.update(env)
    return subprocess.run(
        ["bash", "-c", full],
        capture_output=True,
        text=True,
        cwd=str(REPO_ROOT),
        env=merged,
        check=False,
        timeout=timeout,
    )


def test_install_sh_bash_syntax() -> None:
    result = subprocess.run(
        ["bash", "-n", str(INSTALL_SH)],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr


def test_is_homebrew_path_and_channel_hints(tmp_path: Path) -> None:
    brew_prefix = tmp_path / "opt" / "homebrew"
    brew_bin = _write_executable(brew_prefix / "bin" / "topos")
    local_bin = _write_executable(tmp_path / "local" / "topos")

    script = textwrap.dedent(
        f"""\
        export HOMEBREW_PREFIX="{brew_prefix}"
        if is_homebrew_path "{brew_bin}"; then echo HB_YES; else echo HB_NO; fi
        if is_homebrew_path "{local_bin}"; then echo LOCAL_HB; else echo LOCAL_OK; fi
        channel_hint_for_path "{brew_bin}"
        echo
        channel_hint_for_path "{local_bin}"
        echo
        """
    )
    result = _run_install_helpers(script, env={"PATH": "/usr/bin:/bin"})
    assert result.returncode == 0, result.stderr + result.stdout
    out = result.stdout
    assert "HB_YES" in out
    assert "LOCAL_OK" in out
    assert "brew upgrade topos" in out
    assert "docs.krv.ai/topos/install.sh" in out


def test_preflight_same_path_is_in_place_upgrade(tmp_path: Path) -> None:
    install_dir = tmp_path / "bin"
    target = _write_executable(install_dir / "topos")

    script = textwrap.dedent(
        f"""\
        export INSTALL_DIR="{install_dir}"
        export PATH="{install_dir}:/usr/bin:/bin"
        export HOME="{tmp_path / "home"}"
        export TOPOS_FORCE=0
        export TOPOS_UPDATE=0
        preflight_existing_installs
        """
    )
    result = _run_install_helpers(script)
    assert result.returncode == 0, result.stderr + result.stdout
    assert "upgrading in place" in result.stdout
    assert "Another Topos installation" not in result.stdout
    assert target.exists()


def test_preflight_foreign_noninteractive_continues(tmp_path: Path) -> None:
    install_dir = tmp_path / "localbin"
    install_dir.mkdir()
    brew_prefix = tmp_path / "hb"
    foreign = _write_executable(brew_prefix / "bin" / "topos")

    script = textwrap.dedent(
        f"""\
        export INSTALL_DIR="{install_dir}"
        export HOMEBREW_PREFIX="{brew_prefix}"
        export PATH="{brew_prefix / "bin"}:/usr/bin:/bin"
        export HOME="{tmp_path / "home"}"
        unset TOPOS_FORCE TOPOS_YES || true
        export TOPOS_UPDATE=0
        # No TTY in subprocess: should warn and continue.
        preflight_existing_installs
        echo CONTINUED
        """
    )
    result = _run_install_helpers(script)
    assert result.returncode == 0, result.stderr + result.stdout
    assert "Another Topos installation is already present" in result.stdout
    assert str(foreign.resolve()) in result.stdout or str(foreign) in result.stdout
    assert "brew upgrade topos" in result.stdout
    assert "Non-interactive install; continuing" in result.stdout
    assert "CONTINUED" in result.stdout


def test_preflight_force_skips_confirm_message(tmp_path: Path) -> None:
    install_dir = tmp_path / "localbin"
    install_dir.mkdir()
    foreign = _write_executable(tmp_path / "other" / "topos")

    script = textwrap.dedent(
        f"""\
        export INSTALL_DIR="{install_dir}"
        export PATH="{foreign.parent}:/usr/bin:/bin"
        export HOME="{tmp_path / "home"}"
        export TOPOS_FORCE=1
        export TOPOS_UPDATE=0
        preflight_existing_installs
        echo CONTINUED
        """
    )
    result = _run_install_helpers(script)
    assert result.returncode == 0, result.stderr + result.stdout
    assert "TOPOS_FORCE/TOPOS_YES set; continuing" in result.stdout
    assert "CONTINUED" in result.stdout


def test_homebrew_formula_template_has_foreign_detection() -> None:
    template = (REPO_ROOT / "packaging" / "homebrew" / "topos.rb.template").read_text(
        encoding="utf-8"
    )
    assert "def foreign_topos_binaries" in template
    assert "~/.local/bin/topos" in template
    assert "opoo" in template
    assert "brew upgrade topos" in template
    # Prefer a behavioral smoke beyond --version alone.
    assert 'assert_match "evaluate"' in template
    # Cross-channel only: no formula-to-formula conflicts_with stanza.
    assert not any(
        line.lstrip().startswith("conflicts_with") for line in template.splitlines()
    )


def test_docs_prefer_fully_qualified_homebrew_install() -> None:
    readme = (REPO_ROOT / "README.md").read_text(encoding="utf-8")
    install_rst = (REPO_ROOT / "docs" / "source" / "installation.rst").read_text(
        encoding="utf-8"
    )
    assert "brew install krv-labs/tap/topos" in readme
    assert "brew trust --formula krv-labs/tap/topos" in readme
    assert "brew install krv-labs/tap/topos" in install_rst
    assert "brew trust --formula krv-labs/tap/topos" in install_rst
    # Docs must not recommend disabling tap trust.
    assert "export HOMEBREW_NO_REQUIRE_TAP_TRUST" not in readme
    assert "export HOMEBREW_NO_REQUIRE_TAP_TRUST" not in install_rst


@pytest.mark.skipif(not INSTALL_SH.is_file(), reason="install.sh missing")
def test_install_sh_documents_force_env() -> None:
    text = INSTALL_SH.read_text(encoding="utf-8")
    assert "TOPOS_FORCE" in text
    assert "preflight_existing_installs" in text
    assert "TOPOS_SKIP_MAIN" in text
    # Must not block agent/CI shells that have a controlling tty but no stdin TTY.
    assert "read -r reply < /dev/tty" not in text
    assert "< /dev/tty" not in text


def test_install_sh_recommends_supported_evaluate_flags() -> None:
    text = INSTALL_SH.read_text(encoding="utf-8")
    direct_cli = text.index('echo "Direct CLI (SIMPLE + COMPOSABLE + SECURE):"')
    repo_cd = text.index('echo "  cd <YOUR_REPO_HERE>"', direct_cli)
    evaluate = text.index('echo "  topos evaluate <YOUR_REPO_SRC_HERE> -r"', direct_cli)
    assert repo_cd < evaluate
    assert "topos evaluate <YOUR_REPO_SRC_HERE> -r" in text
    assert "topos evaluate <YOUR_REPO_SRC_HERE> -r --preferences" not in text
    assert "topos evaluate <YOUR_REPO_SRC_HERE> -r --no-composable" not in text


def test_preflight_piped_stdin_does_not_block(tmp_path: Path) -> None:
    """Simulate curl|sh: stdin is a pipe, not a TTY — must warn and continue."""
    install_dir = tmp_path / "localbin"
    install_dir.mkdir()
    foreign = _write_executable(tmp_path / "other" / "topos")

    full = textwrap.dedent(
        f"""\
        set -euo pipefail
        TOPOS_SKIP_MAIN=1
        # shellcheck source=/dev/null
        source "{INSTALL_SH}"
        export INSTALL_DIR="{install_dir}"
        export PATH="{foreign.parent}:/usr/bin:/bin"
        export HOME="{tmp_path / "home"}"
        unset TOPOS_FORCE TOPOS_YES || true
        export TOPOS_UPDATE=0
        preflight_existing_installs
        echo CONTINUED
        """
    )
    # Explicitly pipe empty stdin so -t 0 is false even if the runner has a TTY.
    result = subprocess.run(
        ["bash", "-c", full],
        input="",
        capture_output=True,
        text=True,
        cwd=str(REPO_ROOT),
        env={
            "PATH": "/usr/bin:/bin",
            "HOME": str(tmp_path / "home"),
            "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
            "TOPOS_SKIP_MAIN": "1",
        },
        check=False,
        timeout=10.0,
    )
    assert result.returncode == 0, result.stderr + result.stdout
    assert "Another Topos installation is already present" in result.stdout
    assert "Non-interactive install; continuing" in result.stdout
    assert "CONTINUED" in result.stdout
    assert "Install cancelled" not in result.stdout
