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
| `topos-{platform}` | No | ~45 MB | Default install (`install.sh`), VS Code extension |
| `topos-ect-{platform}` | Yes (onnxruntime, fastembed, trailed) | ~71 MB | Topological coverage offline; optional download |

Structural (UAST) coverage works in the default binary. Topological (ECT) coverage requires `topos-ect-*` or `pip install 'topos-mcp[ect-coverage]'`.

## Optimization strategy (implemented)

1. **Fast path** — `topos --version` and root `topos --help` exit before command registration.
2. **Deferred registration** — subcommands attach on first real CLI invocation.
3. **Lazy package exports** — `import topos` loads only `__version__`; library symbols load on first access.
4. **Lazy command imports** — evaluate/compare/inspect/coverage import heavy stacks inside handlers.
5. **Slim default binary** — ECT stack moved to `topos-ect-*` release artifacts.
6. **Targeted hidden-imports over `--collect-all topos`** — the blanket flag
   bundled the entire 131-file package regardless of reachability. Only the
   `topos/__init__.py` lazy-export table (`importlib.import_module` on a
   runtime-resolved string) is invisible to PyInstaller's static analyzer;
   everything else is already reachable from `topos/cli/main.py`. Replacing
   it with 11 explicit `--hidden-import` entries is a ~0.3% size change
   (removing genuinely dead code) with zero functional risk.
7. **Explicitly excluding the ECT dependency stack from the slim binary** —
   the `dev` dependency group (needed to run the test suite) installs
   `fastembed`, which transitively pulls in `onnxruntime`, `tokenizers`,
   `hf_xet`, and `huggingface_hub`. Because
   `topos/functors/profunctors/cpg/topological_coverage.py` does a lazy
   `from fastembed import ...` reachable via the `coverage` command,
   PyInstaller's static analysis bundled all ~27 MB of that stack into the
   "slim" binary even without `--collect-all onnxruntime` — the slim binary
   was never actually excluding ECT deps in practice. Adding explicit
   `--exclude-module` flags for `onnxruntime`, `fastembed`, `trailed`,
   `tokenizers`, `hf_xet`, and `huggingface_hub` to the slim variant fixes
   this; `ect_coverage_available()` already degrades gracefully when these
   are absent, so `coverage --topological` still reports a clean "install
   the ect-coverage extra" message instead of crashing. Combined with item
   6, this took the slim binary from 72.14 MB to 44.95 MB (-37.7%) with no
   loss of functionality — the real lever behind the warm-startup wins
   below, not the hidden-imports change on its own.
8. **`--noupx`** — defensive: guards against a CI runner incidentally having
   `upx` on `PATH` and silently UPX-compressing the executable, which trades
   size for CPU-bound decompression at every startup (the wrong tradeoff for
   a latency-sensitive CLI).

See [`pyinstaller-onefile-vs-onedir.md`](pyinstaller-onefile-vs-onedir.md) for
why we're keeping onefile as the sole shipped format rather than switching to
onedir for a much bigger startup win.

**`fastmcp`/`ladybug` audit** (tracked in #108): `ladybug` (4.11 MB) backs
depgraph/coupling metrics used outside MCP, so it can't be split out the way
the ECT stack was. `fastmcp` itself is small (0.69 MB across 268 files), but
its own dependency footprint (`pydantic_core`, `cryptography`, `watchfiles`,
`rpds`, `PIL`, `certifi`, `yaml` ≈ 7.6 MB total, ~17% of the slim binary) is
needed for a functional MCP server — and `topos mcp` is the CLI's primary
agent-facing entrypoint (`install.sh` itself recommends `claude mcp add topos
topos mcp`). Splitting fastmcp into a separate variant would fragment that
entrypoint to save under 9 MB; not a good tradeoff. Both stay bundled by
default.

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
| `topos-macos-arm64` (slim, pre this PR) | ~854 ms | ~791 ms | 72.14 MB local build; `--collect-all topos` + leaked ECT deps |
| `topos-macos-arm64` (slim, post this PR) | ~610 ms | ~615 ms | 44.95 MB local build (-37.7% size, see items 6-7 above) |
| `topos evaluate --help` (slim binary, post this PR) | — | ~1616 ms | Subcommand loads full stack |
| `topos-macos-arm64` (onedir, benchmark only, not shipped) | ~70 ms | ~70 ms | See [`pyinstaller-onefile-vs-onedir.md`](pyinstaller-onefile-vs-onedir.md) |

Reference SOTA (typical):

| Tool | `--version` |
|------|-------------|
| `rg` | ~5 ms |
| `uv` | ~30 ms |
| `black` | ~200–400 ms |

## Related

- Harness: [`tests/benchmarks/test_cli_startup.py`](../tests/benchmarks/test_cli_startup.py)
- ECT size notes: [`ect-coverage-release-sizes.md`](ect-coverage-release-sizes.md)
- onefile vs. onedir tradeoff: [`pyinstaller-onefile-vs-onedir.md`](pyinstaller-onefile-vs-onedir.md)
- Release build: [`.github/workflows/release.yml`](../.github/workflows/release.yml)
