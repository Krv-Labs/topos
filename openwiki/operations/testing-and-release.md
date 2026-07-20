---
type: Operations Runbook
title: Testing, packaging, CI, and release operations
description: Runbook for validating Python and Rust changes, testing public CLI/MCP/extension contracts, building frozen binaries, and maintaining release version parity.
resource: /.github/workflows/ci.yml
tags: [operations, testing, ci, release, packaging]
---

# Testing, packaging, CI, and release operations

Topos ships a hybrid Python/Rust package plus CLI, stdio MCP server, frozen binaries, container image, and VS Code extension. Validation must cover the code path being changed—not only Python unit tests. The [architecture overview](../architecture/overview.md) identifies the Python/Rust boundary; [integrations](../integrations/distribution.md) identifies the shipped surfaces.

## Standard local checks

```bash
uv sync --group dev
uv run pytest -v
cargo test
uv run ruff check topos tests
uv run ruff format --check topos tests
cargo clippy -- -D warnings
cargo fmt --check
```

Pytest defaults target `tests/`, add `topos` coverage, and exclude fixtures. Prefer focused test directories during iteration, then run the checks relevant to the modified surface.

| Change area | Start with |
| --- | --- |
| Evaluation policies, roles, suppression | `tests/evaluation/` and `tests/mcp/` contract tests |
| Graph construction/parsers | `tests/graphs/`, `tests/functors/`, language fixtures |
| Rust-backed algorithms | `cargo test`, related `tests/functors/`, and `tests/parity/` if a cross-language adapter changes |
| CLI behavior/startup | `tests/cli/`; preserve registration/startup checks |
| MCP schemas/routing/resources | `tests/mcp/`, including context-budget and contract invariants |
| Frozen distribution | `tests/packaging/` with `TOPOS_BINARY` after a build |
| VS Code extension | `npm ci`, `npm run check-types`, `npm run lint`, `npm run test:unit` in `extensions/vscode/` |

## CI expectations

`.github/workflows/ci.yml` runs Python tests and Rust tests across Python 3.11–3.13. On 3.13 it additionally runs Clippy, Rust formatting, Ruff lint, and Ruff formatting. It has separate jobs that build a frozen binary, dogfood it through packaging tests, enforce warm root-command budgets, and validate the VS Code extension’s type/lint/unit checks.

The startup design is deliberate: root `--version` and root help bypass heavy imports; `evaluate --help` is correctness-tested but not part of the documented fast-path budget. Preserve lazy imports when adjusting `topos/cli/main.py`.

## CLI startup and frozen-binary guardrails

Root `topos --version` and `topos --help` are deliberate fast paths: command registration and evaluation stacks remain deferred until a substantive command runs. The opt-in benchmark harness is `tests/benchmarks/test_cli_startup.py`:

```bash
TOPOS_BENCHMARK=1 uv run pytest tests/benchmarks/test_cli_startup.py -s --no-cov
TOPOS_BENCHMARK=1 TOPOS_BINARY=./dist/topos-macos-arm64 \
  uv run pytest tests/benchmarks/test_cli_startup.py -s --no-cov
```

It measures warm medians (five runs by default) and can measure a onefile cold start with a private temporary directory. Current default budgets are **2.0 s** for `--version`, **3.0 s** for root help, and twice the help budget for `evaluate --help`; override them only when intentionally rebenchmarking. `python -X importtime -m topos.cli.main --help` and optional `hyperfine` runs help diagnose regressions. Do not carry old local timings forward as release guarantees.

Frozen distribution is intentionally a single PyInstaller **onefile** binary. `scripts/build-binary.sh` uses `--onefile`, `--noupx`, selected parser/runtime collections, and hidden imports derived from the lazy-export table. The single-file contract is shared by `install.sh`, CLI installation/uninstall discovery, VS Code binary staging/download validation, release checksums, and macOS signing/notarization in `.github/workflows/release.yml`. An onedir artifact could improve startup by avoiding per-run extraction, but it is a coordinated product/distribution migration—not a packaging-flag substitution. If revisiting it, update each consumer and exercise the [distribution surfaces](../integrations/distribution.md#container-and-editor-surfaces) together.

## Build and release contract

- **Version source of truth:** `Cargo.toml`.
- **Parity check:** `scripts/check_versions.py` compares it to Python/MCP/VS Code metadata.
- **Python build:** Maturin creates the PyO3 extension wheel according to `pyproject.toml`.
- **Frozen build:** `scripts/build-binary.sh` creates the PyInstaller artifact and includes required dynamic imports/resources.
- **Release workflow:** `.github/workflows/release.yml` builds Linux amd64/arm64 and macOS arm64 binaries, dogfoods artifacts, packages platform VSIX files, and publishes release/PyPI artifacts under workflow conditions.

For release changes, inspect the complete workflow rather than assuming a generic Python publish: it handles platform matrix artifacts, optional macOS signing/notarization, extension packaging, and trusted publishing. Never read or record secret values; workflow secret identifiers are sufficient for operational reasoning.

## Documentation and automation

Sphinx docs build through `.github/workflows/docs.yml`. `.github/workflows/openwiki.yml` is the canonical OpenWiki automation: it runs after non-documentation pushes to `main` or manually, invokes `openwiki code --update --print`, deletes the stock scheduled `.github/workflows/openwiki-update.yml` regenerated by the CLI, and opens a documentation PR without workflow files. Generated pages belong under `openwiki/`; do not alter `openwiki/INSTRUCTIONS.md` as part of routine generation.

## Before merging

1. Run focused tests for changed logic and cross-surface tests where contracts are shared.
2. Run formatting/linting for touched Python/Rust/TypeScript code.
3. If packaging, MCP startup, or resource loading changed, build and exercise the frozen binary.
4. If versioning or distribution metadata changed, run `python scripts/check_versions.py`.
5. If changing GitNexus/Sighthound behavior, validate both integration-present and graceful-degradation paths.
