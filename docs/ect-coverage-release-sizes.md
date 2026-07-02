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
| macOS arm64 (local) | 2026-07-02 | **69 MB** | No onnxruntime/fastembed/trailed; warm `--version` ~1.1 s |

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
