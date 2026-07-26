---
type: Operations Runbook
title: Testing, packaging, CI, and release operations
description: Runbook for validating the Rust workspace, MCP server, GitNexus-dependent COMPOSABLE checks, distribution builds, and release metadata.
resource: /.github/workflows/ci.yml
tags: [operations, testing, ci, release, packaging, rust]
---

# Testing, packaging, CI, and release operations

Topos ships a Rust engine, CLI, stdio MCP server, release binaries, an MCP bin wheel, container image, and VS Code extension. Validate the changed surface rather than relying on legacy Python tests. The [architecture overview](../architecture/overview.md) identifies runtime ownership; [integrations](../integrations/distribution.md) identifies shipped surfaces.

## Standard local checks

```bash
python3 scripts/check_versions.py
python3 scripts/check_skill.py
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

| Change area | Start with |
| --- | --- |
| Engine policies, UAST, CFG, PDG, CPG, or graph probes | Focused `cargo test -p topos-engine <filter>`, then `cargo test --workspace` |
| CLI command behavior | `cargo test -p topos` and a direct `cargo run -p topos -- …` smoke case |
| MCP schemas, routing, or server behavior | `cargo test -p topos-mcp`; pipe an `initialize` / `tools/list` request to the built `topos-mcp` binary |
| GitNexus/COMPOSABLE behavior | Build `topos`, generate a fixture `.gitnexus` store with `gitnexus@1.6.8`, then evaluate it |
| MCP bin wheel or container | `maturin build --release --bindings bin --manifest-path topos/mcp/Cargo.toml`; exercise the resulting entrypoint |
| Binary installer or cross-channel preflight | `pytest tests/packaging/test_install_sh_preflight.py`; covers Bash syntax, piped/non-interactive preflight, and advertised `| bash` commands |
| VS Code extension | Run `pnpm install --frozen-lockfile`, `pnpm run check-types`, `pnpm run lint`, and `pnpm run test:unit` in `extensions/vscode/` |

For CFG changes, keep `graphs/cfg/edge_contracts.rs` and deep-nesting tests passing. For UAST/CPG/PDG changes, include the anonymous-node endpoint and iterative-tree regressions where applicable.

## CI expectations

`.github/workflows/ci.yml` has four independent jobs:

- **rust:** version and skill checks, workspace format/Clippy/tests, plus an MCP stdio `tools/list` smoke test.
- **composable:** installs pinned GitNexus, creates a committed fixture repository, runs GitNexus, and asserts that CLI evaluation includes COMPOSABLE rather than falling back to a missing-store message.
- **wheel:** builds the `topos-mcp` Maturin `bin` wheel on manylinux 2.34 with OpenSSL/CMake dependencies required by Ladybug.
- **extension:** type-checks, lints, and unit-tests the VS Code package.

Run the job matching the contract you change; engine-only success does not validate packaging or editor behavior.

## Build and release contract

- **Version source of truth:** workspace `Cargo.toml`.
- **Parity check:** `scripts/check_versions.py` checks relevant distribution metadata.
- **CLI release artifact:** `.github/workflows/release.yml` builds `topos` for Linux amd64/arm64 and macOS arm64 directly with Cargo. It restores the Cargo cache before running `scripts/setup-lbug-prebuilt.sh`, which supplies the pinned LadybugDB static library through `LBUG_LIBRARY_DIR`; on macOS the script rejects source-build overrides to avoid the duplicate-symbol linker failure. Keep that order and fail-fast guard when changing the release build. The workflow also rejects unexpected non-system dynamic linkage before signing, smoke-tests the staged artifact, and on signed macOS releases smoke-tests again under hardened-runtime library validation; these checks protect the artifacts consumed by the [distribution surfaces](../integrations/distribution.md).
- **MCP artifact:** `topos-mcp` is packaged as a Maturin `bin` wheel; it ships a compiled server rather than a Python runtime.
- **VS Code:** release builds stage the matching native CLI into platform VSIX artifacts.
- **Homebrew:** after release, workflow automation renders a checksum-backed formula PR for the tap; tap CI gates its merge.

For release changes, inspect the complete workflow: it handles platform matrix artifacts, optional macOS signing/notarization, extension packaging, Homebrew formula publication, and trusted publishing. The installer is a standalone binary installer — not a Python or PyInstaller deployment path — and its documented curl invocation must pipe to `bash`, not `sh`, because `install.sh` intentionally uses Bash features. When changing the formula template, installer coexistence checks, or Homebrew install guidance, run `pytest tests/packaging/test_install_sh_preflight.py`; its channel-detection cases must continue to distinguish a declared Homebrew prefix from an ordinary local executable, including paths under Intel `/usr/local`, and its piped-install coverage must remain non-blocking and parse-safe. Never read or record secret values; workflow secret identifiers are sufficient for operational reasoning.

## Documentation and automation

Sphinx product docs build through their documentation workflow. `.github/workflows/openwiki.yml` is repository-maintained OpenWiki automation. The currently untracked `.github/workflows/openwiki-update.yml` is not part of the tracked runtime/release contract.

## Before merging

1. Run focused Cargo tests plus `cargo test --workspace`.
2. Run workspace format and Clippy checks.
3. Exercise a direct CLI or MCP stdio case when its public contract changed.
4. Run version/skill checks if metadata or agent assets changed.
5. Validate GitNexus-present and degradation behavior for COMPOSABLE changes, and release/wheel/extension paths when distribution changes.
