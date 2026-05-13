"""vcpkg: download-only install and locate extracted upstream sources."""

from __future__ import annotations

import os
import platform
import subprocess
from pathlib import Path


def default_triplet() -> str:
    sys = platform.system().lower()
    mach = platform.machine().lower()
    if sys == "darwin":
        return "arm64-osx" if mach in ("arm64", "aarch64") else "x64-osx"
    if sys == "linux":
        return "x64-linux"
    if sys == "windows":
        return "x64-windows"
    return "x64-linux"


def run_vcpkg_download(
    vcpkg_root: Path,
    port: str,
    triplet: str,
) -> None:
    vcpkg_exe = vcpkg_root / (
        "vcpkg.exe" if platform.system().lower() == "windows" else "vcpkg"
    )
    if not vcpkg_exe.is_file():
        raise FileNotFoundError(f"vcpkg executable not found at {vcpkg_exe}")
    env = dict(os.environ)
    env.setdefault("VCPKG_ROOT", str(vcpkg_root))
    proc = subprocess.run(
        [
            str(vcpkg_exe),
            "install",
            port,
            "--only-downloads",
            f"--triplet={triplet}",
        ],
        cwd=vcpkg_root,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"vcpkg download failed for {port} (exit {proc.returncode})\n"
            f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )


def find_vcpkg_source_root(vcpkg_root: Path, port: str) -> Path:
    src = vcpkg_root / "buildtrees" / port / "src"
    if not src.is_dir():
        raise FileNotFoundError(f"Missing buildtrees/{port}/src under {vcpkg_root}")
    subdirs = [p for p in src.iterdir() if p.is_dir()]
    if not subdirs:
        raise FileNotFoundError(f"No subdir under {src}")

    # Prefer the tree with the most C++ sources (handles versioned unpack dirs).
    def score(p: Path) -> int:
        return sum(1 for _ in p.rglob("*.cpp")) + sum(1 for _ in p.rglob("*.cc"))

    return max(subdirs, key=lambda p: (score(p), -len(str(p))))
