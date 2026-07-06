"""
CLI startup benchmarks — opt-in via TOPOS_BENCHMARK=1.

Local usage:

  # Dev interpreter (import cost only)
  TOPOS_BENCHMARK=1 uv run pytest tests/benchmarks/test_cli_startup.py -s --no-cov

  # Release binary (includes PyInstaller onefile extraction)
  TOPOS_BENCHMARK=1 TOPOS_BINARY=./dist/topos-macos-arm64 \\
    uv run pytest tests/benchmarks/test_cli_startup.py -s --no-cov

  # Import breakdown
  python -X importtime -m topos.cli.main --help 2>&1 | tee importtime.log

  # Wall clock (requires hyperfine)
  hyperfine --warmup 2 --min-runs 5 \\
    './dist/topos-macos-arm64 --version' \\
    './dist/topos-macos-arm64 --help' \\
    'uv run topos --version'
"""

from __future__ import annotations

import os
import shutil
import statistics
import subprocess
import sys
import time

import pytest

BENCHMARK = os.environ.get("TOPOS_BENCHMARK") == "1"
WARM_RUNS = max(3, int(os.environ.get("TOPOS_BENCHMARK_RUNS", "5")))
VERSION_BUDGET_S = float(os.environ.get("TOPOS_VERSION_BUDGET_S", "2.0"))
HELP_BUDGET_S = float(os.environ.get("TOPOS_HELP_BUDGET_S", "3.0"))
_SKIP_BENCHMARK = "Set TOPOS_BENCHMARK=1 to run startup benchmarks"


def _resolve_topos() -> list[str]:
    binary = os.environ.get("TOPOS_BINARY")
    if binary:
        return [binary]
    return [sys.executable, "-m", "topos.cli.main"]


def _median_runtime(
    argv_suffix: list[str], *, env: dict[str, str] | None = None
) -> float:
    cmd = _resolve_topos() + argv_suffix
    timings: list[float] = []
    for _ in range(WARM_RUNS):
        start = time.perf_counter()
        completed = subprocess.run(
            cmd, check=True, capture_output=True, text=True, env=env
        )
        timings.append(time.perf_counter() - start)
        assert completed.returncode == 0
    return statistics.median(timings)


def _single_runtime(
    argv_suffix: list[str], *, env: dict[str, str] | None = None
) -> float:
    cmd = _resolve_topos() + argv_suffix
    start = time.perf_counter()
    completed = subprocess.run(cmd, check=True, capture_output=True, text=True, env=env)
    assert completed.returncode == 0
    return time.perf_counter() - start


def _clear_mei_dirs(directory: str) -> None:
    try:
        entries = os.listdir(directory)
    except OSError:
        return
    for entry in entries:
        if entry.startswith("_MEI"):
            path = os.path.join(directory, entry)
            if os.path.isdir(path):
                shutil.rmtree(path, ignore_errors=True)


@pytest.mark.skipif(not BENCHMARK, reason=_SKIP_BENCHMARK)
def test_cli_startup_version_warm() -> None:
    median = _median_runtime(["--version"])
    print(f"topos --version warm median: {median * 1000:.1f} ms ({WARM_RUNS} runs)")
    assert median < VERSION_BUDGET_S, f"topos --version too slow: {median:.3f}s"


@pytest.mark.skipif(not BENCHMARK, reason=_SKIP_BENCHMARK)
def test_cli_startup_help_warm() -> None:
    median = _median_runtime(["--help"])
    print(f"topos --help warm median: {median * 1000:.1f} ms ({WARM_RUNS} runs)")
    assert median < HELP_BUDGET_S, f"topos --help too slow: {median:.3f}s"


@pytest.mark.skipif(not BENCHMARK, reason=_SKIP_BENCHMARK)
def test_cli_startup_evaluate_help_warm() -> None:
    median = _median_runtime(["evaluate", "--help"])
    print(
        f"topos evaluate --help warm median: {median * 1000:.1f} ms ({WARM_RUNS} runs)"
    )
    assert median < HELP_BUDGET_S * 2, f"topos evaluate --help too slow: {median:.3f}s"


@pytest.mark.skipif(not BENCHMARK, reason=_SKIP_BENCHMARK)
@pytest.mark.skipif(
    not os.environ.get("TOPOS_BINARY"),
    reason="Set TOPOS_BINARY to benchmark PyInstaller cold start",
)
def test_cli_startup_version_cold(tmp_path) -> None:
    """Cold start: clear PyInstaller _MEI dirs before each timed run."""
    private_tmp = tmp_path / "pyinstaller-tmp"
    private_tmp.mkdir()
    env = os.environ.copy()
    env["TMPDIR"] = str(private_tmp)

    timings: list[float] = []
    for _ in range(WARM_RUNS):
        _clear_mei_dirs(str(private_tmp))
        timings.append(_single_runtime(["--version"], env=env))

    cold = max(timings)
    cold_budget = float(os.environ.get("TOPOS_VERSION_COLD_BUDGET_S", "3.0"))
    print(f"topos --version cold max: {cold * 1000:.1f} ms ({WARM_RUNS} runs)")
    assert cold < cold_budget, f"topos --version cold too slow: {cold:.3f}s"
