"""Channel-aware update checks and upgrades for the Topos CLI."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from datetime import UTC, datetime, timedelta
from pathlib import Path
from urllib.error import URLError
from urllib.request import Request, urlopen

import click

from topos import __version__
from topos.cli.installation import InstallInfo, detect_install_info

_INSTALL_SCRIPT_URL = "https://docs.krv.ai/topos/install.sh"
_GITHUB_REPO = "Krv-Labs/topos"
_PYPI_PACKAGE = "topos-mcp"
_FETCH_TIMEOUT = 3
_CACHE_TTL = timedelta(hours=24)
_VERSION_RE = re.compile(r"^v?(?P<major>\d+)(?:\.(?P<minor>\d+))?(?:\.(?P<patch>\d+))?")


def update_check_cache_file() -> Path:
    override = os.environ.get("TOPOS_UPDATE_CHECK_FILE")
    if override:
        return Path(override).expanduser()
    state_home = Path(os.environ.get("XDG_STATE_HOME", "~/.local/state")).expanduser()
    return state_home / "topos" / "update-check.json"


def normalize_version(tag: str) -> tuple[int, ...]:
    match = _VERSION_RE.match(tag.strip())
    if not match:
        return (0,)
    parts: list[int] = []
    for key in ("major", "minor", "patch"):
        value = match.group(key)
        parts.append(int(value) if value is not None else 0)
    return tuple(parts)


def is_outdated(current: str, latest: str) -> bool:
    return normalize_version(current) < normalize_version(latest)


def fetch_latest_github_tag(repo: str = _GITHUB_REPO) -> str | None:
    request = Request(
        f"https://github.com/{repo}/releases/latest",
        method="HEAD",
        headers={"User-Agent": "topos-cli"},
    )
    try:
        with urlopen(request, timeout=_FETCH_TIMEOUT) as response:
            effective_url = response.geturl()
    except (URLError, OSError, TimeoutError):
        return None

    if "/releases/tag/" not in effective_url:
        return None
    tag = effective_url.rsplit("/", 1)[-1].split("?")[0].split("#")[0]
    if not tag or not _VERSION_RE.match(tag):
        return None
    return tag.lstrip("v") if tag.startswith("v") else tag


def fetch_latest_pypi_version(package: str = _PYPI_PACKAGE) -> str | None:
    request = Request(
        f"https://pypi.org/pypi/{package}/json",
        headers={"User-Agent": "topos-cli"},
    )
    try:
        with urlopen(request, timeout=_FETCH_TIMEOUT) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except (URLError, OSError, TimeoutError, json.JSONDecodeError, KeyError):
        return None
    version = payload.get("info", {}).get("version")
    return str(version) if version else None


def latest_version_for_channel(method: str) -> str | None:
    if method in {"package-manager", "source"}:
        return fetch_latest_pypi_version()
    return fetch_latest_github_tag()


def load_update_check_cache() -> dict[str, str] | None:
    path = update_check_cache_file()
    if not path.exists():
        return None
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    if not isinstance(data, dict):
        return None
    return {str(k): str(v) for k, v in data.items()}


def save_update_check_cache(latest: str, channel: str) -> None:
    path = update_check_cache_file()
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "checked_at": datetime.now(UTC).isoformat(),
        "latest": latest,
        "channel": channel,
    }
    path.write_text(json.dumps(payload), encoding="utf-8")


def cache_is_fresh(cache: dict[str, str] | None) -> bool:
    if not cache or "checked_at" not in cache:
        return False
    try:
        checked_at = datetime.fromisoformat(cache["checked_at"])
    except ValueError:
        return False
    if checked_at.tzinfo is None:
        checked_at = checked_at.replace(tzinfo=UTC)
    return datetime.now(UTC) - checked_at < _CACHE_TTL


def _run_binary_update(info: InstallInfo, pin_version: str | None) -> None:
    provenance = info.provenance or {}
    install_path = provenance.get("install_path", "").strip()
    if not install_path:
        click.echo("Installer provenance is missing install_path.", err=True)
        sys.exit(1)

    install_dir = str(Path(install_path).expanduser().parent)
    env = os.environ.copy()
    env["TOPOS_UPDATE"] = "1"
    env["TOPOS_INSTALL"] = install_dir
    env["TOPOS_NO_MODIFY_PATH"] = "1"
    if pin_version:
        env["TOPOS_VERSION"] = pin_version

    click.echo("Updating Topos via install.sh...")
    curl_proc = subprocess.Popen(
        ["curl", "-fsSL", _INSTALL_SCRIPT_URL],
        stdout=subprocess.PIPE,
    )
    try:
        proc = subprocess.run(
            ["sh", "-s"],
            stdin=curl_proc.stdout,
            env=env,
            check=False,
        )
    finally:
        if curl_proc.stdout is not None:
            curl_proc.stdout.close()
        curl_proc.wait()

    if proc.returncode != 0:
        sys.exit(proc.returncode)


def _run_package_update(info: InstallInfo) -> None:
    installer = info.installer or "pip"
    if installer == "uv":
        cmd = ["uv", "pip", "install", "-U", _PYPI_PACKAGE]
    else:
        cmd = ["pip", "install", "-U", _PYPI_PACKAGE]

    click.echo(f"Running: {' '.join(cmd)}")
    proc = subprocess.run(cmd, check=False)
    if proc.returncode != 0:
        sys.exit(proc.returncode)


def _print_unknown_upgrade_paths() -> None:
    click.echo("Could not determine install channel. Supported upgrade paths:")
    click.echo("")
    click.echo("  # Binary (recommended)")
    click.echo("  curl -fsSL https://docs.krv.ai/topos/install.sh | sh")
    click.echo("")
    click.echo("  # PyPI")
    click.echo("  uv pip install -U topos-mcp")
    click.echo("")
    click.echo("  # Source checkout")
    click.echo("  git pull && uv pip install -e .")


def run_update(*, check_only: bool, pin_version: str | None) -> None:
    info = detect_install_info()
    current = __version__
    latest = latest_version_for_channel(info.method)

    if check_only:
        if latest is None:
            click.echo(f"Could not determine latest version (channel: {info.method}).")
            sys.exit(2)
        if is_outdated(current, latest):
            click.echo(f"Outdated: {current} → {latest}")
            sys.exit(1)
        click.echo(f"Up to date: {current}")
        return

    if pin_version and info.method != "binary-installer":
        click.echo("--version is only supported for binary installs.", err=True)
        sys.exit(1)

    if info.method == "binary-installer":
        _run_binary_update(info, pin_version)
        return

    if info.method == "package-manager":
        _run_package_update(info)
        return

    if info.method == "source":
        click.echo("Detected editable/source installation.")
        click.echo(f"Run: {info.update_cmd or 'git pull && uv pip install -e .'}")
        return

    _print_unknown_upgrade_paths()


def should_skip_passive_notice(
    *,
    invoked_subcommand: str | None,
    help_requested: bool,
) -> bool:
    if os.environ.get("CI", "").lower() == "true":
        return True
    if os.environ.get("TOPOS_NO_UPDATE_NOTICES") == "1":
        return True
    if not sys.stderr.isatty():
        return True
    if help_requested:
        return True
    return invoked_subcommand in {"mcp", "update"}


def maybe_show_update_notice(
    *,
    invoked_subcommand: str | None,
    help_requested: bool,
) -> None:
    if should_skip_passive_notice(
        invoked_subcommand=invoked_subcommand,
        help_requested=help_requested,
    ):
        return

    info = detect_install_info()
    cache = load_update_check_cache()
    if cache_is_fresh(cache) and cache.get("latest"):
        latest = cache["latest"]
    else:
        latest = latest_version_for_channel(info.method)
        if latest is None:
            return
        save_update_check_cache(latest, info.method)

    current = __version__
    if not is_outdated(current, latest):
        return

    click.echo(
        f"Update available: {current} → {latest}. Run: topos update",
        err=True,
    )
