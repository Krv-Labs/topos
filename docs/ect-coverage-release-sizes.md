# ECT coverage binary size verification

Measured on the `ect-coverage` branch with PyInstaller args from `.github/workflows/release.yml` (includes `--collect-all onnxruntime`, `fastembed`, `trailed`). Embedding model is **not** bundled; it downloads on first topological coverage use.

## v0.3.4 baseline (no ECT)

| Artifact | Size |
|----------|------|
| `topos-macos-arm64` | 38.1 MB |
| `topos-macos-amd64` | 40.3 MB |
| `topos-linux-arm64` | 54.3 MB |
| `topos-linux-amd64` | 57.6 MB |

Source: [GitHub release v0.3.4](https://github.com/Krv-Labs/topos/releases/tag/v0.3.4).

From v0.3.8 onward, default release artifacts exclude ECT; use `topos-ect-*` for topological coverage offline.

## Slim default (no ECT, post startup optimization)

| Platform | Build date | Size | Notes |
|----------|------------|------|-------|
| macOS arm64 (local) | 2026-07-02 | 72 MB | Regression: `--collect-all onnxruntime` etc. were correctly omitted, but the `dev`-group `fastembed` install plus a lazy `from fastembed import ...` reachable via the `coverage` command meant PyInstaller's static analysis bundled the full ECT stack anyway. Not actually slim; see below. |
| macOS arm64 (local) | 2026-07-04 | **45 MB** | Fixed with explicit `--exclude-module` for `onnxruntime`/`fastembed`/`trailed`/`tokenizers`/`hf_xet`/`huggingface_hub`; combined with dropping `--collect-all topos` for targeted hidden-imports. warm `--version` ~610 ms. See [`cli-startup-benchmarks.md`](cli-startup-benchmarks.md). |

## ECT-enabled build (`topos-ect-*`)

| Platform | Build date | Size | Delta vs v0.3.4 |
|----------|------------|------|-----------------|
| macOS arm64 (local) | 2026-06-16 | **71.0 MB** | **+32.9 MB** |

Build command: see `extensions/vscode/workflow/publishing.md` (local PyInstaller section).

## VSIX gate

The VS Code extension size gate is **200 MiB** (`extensions/vscode/scripts/check-vsix-size.js`). Projected ECT-enabled VSIX (~60–95 MB) remains well under the limit.

## Notes

- Size increase is dominated by `onnxruntime` native libraries bundled into the onefile binary.
- First-run model download (`snowflake-arctic-embed-xs`, quantized ~23 MB) is stored under `~/.cache/fastembed` and is not part of the binary size above.
- The slim/ECT split is enforced via `--exclude-module` on the slim variant, not merely by omitting `--collect-all` for the ECT packages — omission alone doesn't stop PyInstaller from bundling a package that's still statically reachable and installed in the build environment.
