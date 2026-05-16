"""crates.io: resolve version, download .crate, locate Rust sources."""

from __future__ import annotations

import json
import tarfile
import urllib.request
from pathlib import Path

USER_AGENT = "topos-calibration (https://github.com/krv-ai/topos; contact@krv.ai)"


def fetch_latest_version(crate: str) -> str:
    url = f"https://crates.io/api/v1/crates/{crate}"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    return str(data["crate"]["max_version"])


def download_crate(
    crate: str, version: str, dest_dir: Path, *, skip_download: bool
) -> Path:
    dest_dir.mkdir(parents=True, exist_ok=True)
    fname = f"{crate}-{version}.crate"
    dest = dest_dir / fname
    if dest.is_file() and skip_download:
        return dest
    url = f"https://static.crates.io/crates/{crate}/{fname}"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=120) as resp:
        dest.write_bytes(resp.read())
    return dest


def extract_crate(archive: Path, extract_to: Path) -> None:
    extract_to.mkdir(parents=True, exist_ok=True)
    if any(extract_to.iterdir()):
        return
    with tarfile.open(archive, mode="r:gz") as tf:
        tf.extractall(extract_to, filter="data")


def find_rust_source_root(extracted_root: Path) -> Path:
    """Prefer the shallowest directory containing ``Cargo.toml`` with ``*.rs`` files."""
    candidates: list[tuple[int, Path]] = []
    for cargo in extracted_root.rglob("Cargo.toml"):
        parent = cargo.parent
        if not any(parent.rglob("*.rs")):
            continue
        depth = len(parent.relative_to(extracted_root).parts)
        candidates.append((depth, parent))
    if not candidates:
        raise FileNotFoundError(
            f"No Cargo.toml with Rust sources under {extracted_root}"
        )
    candidates.sort(key=lambda t: (t[0], str(t[1])))
    return candidates[0][1]
