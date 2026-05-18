"""npm registry: resolve latest version, download tarball, ``package/`` root."""

from __future__ import annotations

import json
import tarfile
import urllib.error
import urllib.request
from pathlib import Path

USER_AGENT = "topos-calibration (https://github.com/krv-ai/topos; contact@krv.ai)"


def fetch_latest_version(package: str) -> str:
    url = f"https://registry.npmjs.org/{package.replace('/', '%2F')}"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    return str(data["dist-tags"]["latest"])


def download_npm_tarball(
    package: str, version: str, dest_dir: Path, *, skip_download: bool
) -> Path:
    meta_url = f"https://registry.npmjs.org/{package.replace('/', '%2F')}"
    req = urllib.request.Request(meta_url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    dist = data["versions"][version].get("dist") or {}
    tarball_url = dist.get("tarball")
    if not tarball_url:
        raise RuntimeError(f"No tarball URL for {package}@{version}")

    dest_dir.mkdir(parents=True, exist_ok=True)
    safe = package.replace("/", "-")
    dest = dest_dir / f"{safe}-{version}.tgz"
    if dest.is_file() and skip_download:
        return dest
    t_req = urllib.request.Request(tarball_url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(t_req, timeout=180) as t_resp:
        dest.write_bytes(t_resp.read())
    return dest


def extract_npm_tarball(archive: Path, extract_to: Path) -> Path:
    extract_to.mkdir(parents=True, exist_ok=True)
    pkg = extract_to / "package"
    if pkg.is_dir():
        return pkg
    with tarfile.open(archive, mode="r:gz") as tf:
        tf.extractall(extract_to, filter="data")
    if not pkg.is_dir():
        raise FileNotFoundError(f"Expected package/ under {extract_to}")
    return pkg
