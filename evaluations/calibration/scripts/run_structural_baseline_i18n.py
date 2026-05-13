"""Download cohort sources and run ``topos evaluate --json`` per ecosystem.

See ``docs/calibration.md`` for cohort paths and flags.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
CALIBRATION_DIR = REPO_ROOT / "evaluations" / "calibration"
RESULTS_DIR = CALIBRATION_DIR / "results"
CACHE_DIR = CALIBRATION_DIR / ".cache" / "i18n"

_SCRIPTS = Path(__file__).resolve().parent
if str(_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS))

import ecosystem_crates  # noqa: E402
import ecosystem_npm  # noqa: E402
import ecosystem_vcpkg  # noqa: E402


def _safe_cache_name(name: str) -> str:
    return name.replace("/", "__").replace(" ", "_")


def run_topos_evaluate(path: Path, *, language: str) -> dict:
    env = dict(os.environ)
    src_path = str(REPO_ROOT / "src")
    existing = env.get("PYTHONPATH", "")
    env["PYTHONPATH"] = (
        src_path if not existing else f"{src_path}{os.pathsep}{existing}"
    )

    command = [
        sys.executable,
        "-m",
        "topos.main",
        "evaluate",
        str(path),
        "-r",
        "--json",
        "--priority",
        "balanced",
        "--language",
        language,
    ]
    completed = subprocess.run(
        command,
        cwd=REPO_ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"topos evaluate failed for {path}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    payload_text = completed.stdout
    marker = "\n\nOverall:"
    if marker in payload_text:
        payload_text = payload_text.split(marker, maxsplit=1)[0]
    return json.loads(payload_text.strip())


def process_rust(
    crate: str,
    *,
    skip_download: bool,
    download_dir: Path,
) -> tuple[str, str, Path]:
    version = ecosystem_crates.fetch_latest_version(crate)
    archive = ecosystem_crates.download_crate(
        crate, version, download_dir, skip_download=skip_download
    )
    extract_base = download_dir.parent / "extracted" / _safe_cache_name(crate) / version
    ecosystem_crates.extract_crate(archive, extract_base)
    children = [p for p in extract_base.iterdir() if p.is_dir()]
    extracted_root = children[0] if len(children) == 1 else extract_base
    src_dir = ecosystem_crates.find_rust_source_root(extracted_root)
    return crate, version, src_dir


def process_npm(
    package: str,
    *,
    skip_download: bool,
    download_dir: Path,
) -> tuple[str, str, Path]:
    version = ecosystem_npm.fetch_latest_version(package)
    archive = ecosystem_npm.download_npm_tarball(
        package, version, download_dir, skip_download=skip_download
    )
    extract_base = (
        download_dir.parent / "extracted" / _safe_cache_name(package) / version
    )
    extract_base.mkdir(parents=True, exist_ok=True)
    src_dir = ecosystem_npm.extract_npm_tarball(archive, extract_base)
    return package, version, src_dir


def process_vcpkg(
    port: str,
    *,
    vcpkg_root: Path,
    triplet: str,
    skip_download: bool,
) -> tuple[str, str, Path]:
    if not skip_download:
        ecosystem_vcpkg.run_vcpkg_download(vcpkg_root, port, triplet)
    src_dir = ecosystem_vcpkg.find_vcpkg_source_root(vcpkg_root, port)
    return port, "vcpkg", src_dir


def _default_cohort(ecosystem: str) -> Path:
    return {
        "rust": CALIBRATION_DIR / "top100_crates_io.txt",
        "npm_js": CALIBRATION_DIR / "top100_npm_js.txt",
        "npm_ts": CALIBRATION_DIR / "top100_npm_ts.txt",
        "vcpkg": CALIBRATION_DIR / "top100_vcpkg_ports.txt",
    }[ecosystem]


def _default_output(ecosystem: str) -> Path:
    return {
        "rust": RESULTS_DIR / "structural_scores_rust.jsonl",
        "npm_js": RESULTS_DIR / "structural_scores_npm_js.jsonl",
        "npm_ts": RESULTS_DIR / "structural_scores_npm_ts.jsonl",
        "vcpkg": RESULTS_DIR / "structural_scores_vcpkg.jsonl",
    }[ecosystem]


def _language_for(ecosystem: str) -> str:
    return {
        "rust": "rust",
        "npm_js": "javascript",
        "npm_ts": "typescript",
        "vcpkg": "cpp",
    }[ecosystem]


def _load_cohort(path: Path) -> list[str]:
    lines: list[str] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        lines.append(line)
    return lines


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Run topos evaluate on multilanguage calibration cohorts.",
    )
    parser.add_argument(
        "--ecosystem",
        choices=("rust", "npm_js", "npm_ts", "vcpkg"),
        required=True,
    )
    parser.add_argument(
        "--cohort",
        type=Path,
        default=None,
        help="Package list (one per line). Defaults per --ecosystem.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="JSONL output path (defaults per ecosystem under results/).",
    )
    parser.add_argument("--limit", type=int, default=None)
    parser.add_argument("--skip-download", action="store_true")
    parser.add_argument(
        "--vcpkg-root",
        type=Path,
        default=None,
        help="VCPKG_ROOT (required for ecosystem=vcpkg).",
    )
    parser.add_argument(
        "--vcpkg-triplet",
        default=None,
        help="Host triplet for vcpkg (default: auto-detect).",
    )
    args = parser.parse_args()
    eco = args.ecosystem
    cohort_path = args.cohort or _default_cohort(eco)
    if not cohort_path.is_file():
        parser.error(f"Cohort file not found: {cohort_path}")
    names = _load_cohort(cohort_path)
    if args.limit is not None:
        names = names[: args.limit]

    output_path = args.output or _default_output(eco)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    language = _language_for(eco)
    vcpkg_root = args.vcpkg_root
    if eco == "vcpkg":
        if vcpkg_root is None:
            env_root = os.environ.get("VCPKG_ROOT")
            if not env_root:
                parser.error(
                    "--vcpkg-root or VCPKG_ROOT is required for ecosystem=vcpkg"
                )
            vcpkg_root = Path(env_root)
        vcpkg_root = vcpkg_root.resolve()

    triplet = args.vcpkg_triplet or ecosystem_vcpkg.default_triplet()
    download_base = CACHE_DIR / eco

    total = len(names)
    print(f"Ecosystem={eco} language={language} packages={total} -> {output_path}")
    with output_path.open("w", encoding="utf-8") as out_fh:
        for idx, name in enumerate(names, start=1):
            print(f"[{idx}/{total}] {name} ...", end=" ", flush=True)
            try:
                if eco == "rust":
                    dl = download_base / "crates"
                    pkg, version, src = process_rust(
                        name, skip_download=args.skip_download, download_dir=dl
                    )
                elif eco in ("npm_js", "npm_ts"):
                    dl = download_base / "npm"
                    pkg, version, src = process_npm(
                        name, skip_download=args.skip_download, download_dir=dl
                    )
                else:
                    pkg, version, src = process_vcpkg(
                        name,
                        vcpkg_root=vcpkg_root,  # type: ignore[arg-type]
                        triplet=triplet,
                        skip_download=args.skip_download,
                    )
                print(f"version={version}", end=" ", flush=True)
                payload = run_topos_evaluate(src, language=language)
                results: list[dict] = payload.get("results", [])
                print(f"files={len(results)}")
                for file_result in results:
                    if isinstance(file_result, dict):
                        rec = {
                            "package": pkg,
                            "version": version,
                            "ecosystem": eco,
                            "language": language,
                            **file_result,
                        }
                    else:
                        rec = {
                            "package": pkg,
                            "version": version,
                            "ecosystem": eco,
                            "language": language,
                            "raw": file_result,
                        }
                    out_fh.write(json.dumps(rec) + "\n")
            except Exception as exc:  # noqa: BLE001
                err = str(exc).splitlines()[0][:240]
                print(f"ERROR: {err}")
                out_fh.write(
                    json.dumps(
                        {
                            "package": name,
                            "version": "unknown",
                            "ecosystem": eco,
                            "language": language,
                            "error": str(exc),
                        }
                    )
                    + "\n"
                )
    print(f"Done. Wrote {output_path}")


if __name__ == "__main__":
    main()
