# CLI startup benchmarks

This document tracks Topos CLI cold/warm startup times for trivial commands (`--version`, `--help`). The goal is to keep simple invocations competitive with other Python CLIs and avoid paying evaluation/MCP import costs until a subcommand actually runs.

## How to measure

### Dev interpreter (import cost only)

```bash
TOPOS_BENCHMARK=1 uv run pytest tests/benchmarks/test_cli_startup.py -s --no-cov
```

### Release binary (PyInstaller onefile + extraction)

Build the slim binary locally (see `extensions/vscode/workflow/publishing.md`), then:

```bash
TOPOS_BENCHMARK=1 TOPOS_BINARY=./dist/topos-macos-arm64 \
  uv run pytest tests/benchmarks/test_cli_startup.py -s --no-cov
```

Optional cold-start run (clears `_MEI*` temp dirs when possible):

```bash
TOPOS_BENCHMARK=1 TOPOS_BINARY=./dist/topos-macos-arm64 \
  TOPOS_VERSION_COLD_BUDGET_S=4.0 \
  uv run pytest tests/benchmarks/test_cli_startup.py -s --no-cov -k cold
```

### Import breakdown

```bash
python -X importtime -m topos.cli.main --help 2>&1 | tee importtime.log
```

### Wall clock (hyperfine)

```bash
hyperfine --warmup 2 --min-runs 5 \
  './dist/topos-macos-arm64 --version' \
  './dist/topos-macos-arm64 --help' \
  'uv run topos --version' \
  'rg --version'
```

## Artifacts

| Artifact | ECT deps | Typical size (macOS arm64) | Use |
|----------|----------|----------------------------|-----|
| `topos-{platform}` | No | ~38 MB | Default install (`install.sh`), VS Code extension |
| `topos-ect-{platform}` | Yes (onnxruntime, fastembed, trailed) | ~71 MB | Topological coverage offline; optional download |

Structural (UAST) coverage works in the default binary. Topological (ECT) coverage requires `topos-ect-*` or `pip install 'topos-mcp[ect-coverage]'`.

## Optimization strategy (implemented)

1. **Fast path** — `topos --version` and root `topos --help` exit before command registration.
2. **Deferred registration** — subcommands attach on first real CLI invocation.
3. **Lazy package exports** — `import topos` loads only `__version__`; library symbols load on first access.
4. **Lazy command imports** — evaluate/compare/inspect/coverage import heavy stacks inside handlers.
5. **Slim default binary** — ECT stack moved to `topos-ect-*` release artifacts.

## Budgets

CI (`cli-startup` job on `main` PRs) enforces warm medians on the **slim** Linux binary:

| Command | Warm budget |
|---------|-------------|
| `topos --version` | 2.5 s |
| `topos --help` | 3.5 s |

Adjust via `TOPOS_VERSION_BUDGET_S` / `TOPOS_HELP_BUDGET_S` when re-benchmarking.

## Baseline (post-optimization, dev path)

Run locally and paste results after each release that touches startup:

| Channel | `--version` warm | `--help` warm | Notes |
|---------|------------------|---------------|-------|
| `uv run topos` (dev, post-opt) | ~59 ms | ~57 ms | macOS arm64, 2026-07-02 |
| `topos evaluate --help` (dev) | — | ~130 ms | Loads evaluate command module |
| `topos-linux-amd64` (slim) | _CI job_ | _CI job_ | PR guardrail |
| `topos-macos-arm64` (slim, post-opt) | ~1078 ms | ~1062 ms | 69 MB local build; PyInstaller extraction |
| `topos evaluate --help` (slim binary) | — | ~2340 ms | Subcommand loads full stack |

Reference SOTA (typical):

| Tool | `--version` |
|------|-------------|
| `rg` | ~5 ms |
| `uv` | ~30 ms |
| `black` | ~200–400 ms |

## Related

- Harness: [`tests/benchmarks/test_cli_startup.py`](../tests/benchmarks/test_cli_startup.py)
- ECT size notes: [`ect-coverage-release-sizes.md`](ect-coverage-release-sizes.md)
- Release build: [`.github/workflows/release.yml`](../.github/workflows/release.yml)
