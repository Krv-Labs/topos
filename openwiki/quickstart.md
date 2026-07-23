---
type: Repository Guide
title: Topos code wiki quickstart
description: Entry point for maintaining Topos, a hybrid Python and Rust structural code-quality evaluator for AI agents, its analysis model, interfaces, integrations, and release checks.
resource: /README.md
tags: [topos, code-quality, static-analysis, agents]
---

# Topos code wiki

Topos evaluates source code as a structural object for coding agents. It produces independent **SIMPLE**, **COMPOSABLE**, and **SECURE** outcomes, then maps their combination to a medal from SLOP to GOLD. The Python package owns parsing orchestration, graph construction, policies, CLI, and MCP; a Rust/PyO3 extension supplies performance-sensitive graph and metric kernels.

Start with this wiki when changing behavior rather than reading the repository as a file inventory.

## What Topos delivers

- `topos evaluate` scores source files and can emit terminal or JSON results.
- `topos mcp` exposes the same evaluation and iterative-assessment loop over stdio for MCP clients.
- `topos depgraph generate` builds the GitNexus state required for cross-file COMPOSABLE evaluation.
- `topos refactor` provides advisory cycle, dependency, and process hotspots; these do **not** change the three-pillar medal.

The central [quality model](domain/quality-model.md) explains what each verdict means. The [architecture overview](architecture/overview.md) follows source code from parsing through graph metrics to the lattice verdict.

## Practical starting points

```bash
# Basic source evaluation (recursive)
topos evaluate src/ -r

# Install the prebuilt CLI through Homebrew (macOS arm64; Linux amd64/arm64)
brew install krv-labs/tap/topos

# Enable inter-module COMPOSABLE analysis
pnpm add -g gitnexus  # or: npm install -g gitnexus
topos depgraph generate
topos evaluate src/ -r --gitnexus-dir .gitnexus

# Run tests and the hybrid-language checks used by CI
uv sync --group dev
uv run pytest -v
cargo test
```

`evaluate` accepts `--language` from the centrally supported set (Python, Rust, JavaScript, TypeScript, C++, and Go), `--preferences`, and `--json`. The [agent and CLI workflows](workflows/agent-and-cli.md) cover usage boundaries, MCP iteration, and advisory refactoring.

## Read by task

| If you need to… | Read |
| --- | --- |
| Understand parsing, graph construction, native code, and the evaluation flow | [Architecture overview](architecture/overview.md) |
| Change a pillar, threshold, security acknowledgement, or role exception | [Quality model](domain/quality-model.md) |
| Change a CLI command, MCP tool contract, agent loop, or refactor output | [Agent and CLI workflows](workflows/agent-and-cli.md) |
| Work on GitNexus, Sighthound, Docker, VS Code, or MCP packaging | [Integrations and distribution](integrations/distribution.md) |
| Run focused verification, modify CI/release code, or ship a build | [Testing and release operations](operations/testing-and-release.md) |
| Find the code and test area for a maintenance task | [Source map](source-map.md) |

## Recent direction that affects changes

Recent releases consolidated gate definitions and agent-routing contracts, added Go support, and introduced advisory refactor analyses. Optional Corgea/Sighthound finding ingestion lets CPG SECURE metrics count usable Sighthound JSON findings, falling back to local dangerous-API and taint probes otherwise (`topos/graphs/cpg/object.py`). The current release workflow also publishes the Homebrew channel: `brew install krv-labs/tap/topos` installs the prebuilt CLI on macOS arm64 or Linux amd64/arm64, while the workflow renders its formula from release checksums and submits it to the tap for CI-gated merge. See [testing and release operations](operations/testing-and-release.md#build-and-release-contract) when changing binaries, checksums, or packaging.

COMPOSABLE scoring also evolved from a raw instability band toward Martin’s main-sequence distance when abstractness and actual coupling signals exist. Its thresholds are explicitly provisional in `CHANGELOG.md`; change scoring and its regression tests together.

## Repository boundaries

- Source and tests are authoritative. Sphinx product docs live under `docs/source/`; this wiki is the maintained home for engineering rationale, including UAST/MDG limits, refactor-method boundaries, and frozen-binary performance decisions.
- `Cargo.toml` owns the release version. Python packaging, MCP registry metadata, and VS Code package metadata are checked for parity.
- `openwiki/INSTRUCTIONS.md` is a user-authored scope brief; do not rewrite it during normal wiki maintenance.
- The working tree contains untracked OpenWiki workflows and repository agent instruction files at initialization time. They are operational metadata, not Topos runtime code.

## Backlog

- **Formal lattice ordering** — `topos/core/omega.py`: verify the explanatory mathematical ordering against the implemented bitmask/order helpers before expanding theory documentation.
- **Parser backend semantics** — `topos/graphs/ast/dispatch.py`: `hybrid` is a public default but currently has a true native parser only for Python; document the intended future contract once confirmed.
