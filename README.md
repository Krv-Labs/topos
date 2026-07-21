<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/Krv-Labs/topos/main/docs/source/_static/topos-logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/Krv-Labs/topos/main/docs/source/_static/topos-logo.svg">
    <img src="https://raw.githubusercontent.com/Krv-Labs/topos/main/docs/source/_static/topos-logo.svg" alt="Topos" width="400">
  </picture>
</p>

<p align="center">
  <a href="https://github.com/Krv-Labs/topos/actions/workflows/ci.yml"><img src="https://github.com/Krv-Labs/topos/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://pypi.org/project/topos-mcp/"><img src="https://img.shields.io/pypi/v/topos-mcp.svg" alt="PyPI"></a>
  <a href="https://pypi.org/project/topos-mcp/"><img src="https://img.shields.io/pypi/pyversions/topos-mcp.svg" alt="Python versions"></a>
  <a href="https://github.com/Krv-Labs/topos/blob/main/LICENSE"><img src="https://img.shields.io/github/license/Krv-Labs/topos" alt="License"></a>
  <a href="https://glama.ai/mcp/servers/Krv-Labs/topos"><img src="https://glama.ai/mcp/servers/Krv-Labs/topos/badges/score.svg" alt="topos MCP server"></a>
</p>

<p align="center">
  <b>Structural code quality metrics for agent-written programs.</b><br>
  > [!TIP]
  > Install as an OpenClaw skill: `openclaw skills install Krv-Labs/topos`<br>
  > Install as a Hermes skill: `hermes skills tap add Krv-Labs/topos` then `hermes skills install topos`<br>
  <a href="https://docs.krv.ai/topos/">Docs</a> ·
  <a href="#quick-start">Quick Start</a> ·
  <a href="#mcp-server-for-agents">MCP Server</a> ·
  <a href="https://github.com/Krv-Labs/topos/issues">Issues</a>
</p>
<!-- mcp-name: io.github.Krv-Labs/topos -->

**Topos** is an _operating layer for AI agents_ that provides structural (geometric & topological) metrics computed over program graphs, surfacing deep
architectural debt that conventional linters can't compute. It delivers complexity, coupling, and security metrics for your agents to wield as tools,
establishing a precise, medal-scored (SLOP → GOLD) feedback loop to autonomously write clean, highly composable code.

---

## Quick Start

```bash
curl -fsSL https://docs.krv.ai/topos/install.sh | sh
topos evaluate src/ -r
```

`evaluate -r` scores every file in `src/` and prints a ranked digest: which pillars pass, the worst-scoring files, and the cheapest fixes to flip a failing pillar. Add `-h` to any command for help, or `--json` for CI.

Other install paths (PyPI, source checkout) and the full command tour live at **[docs.krv.ai/topos/installation](https://docs.krv.ai/topos/installation.html)**.

## What you get

Topos checks three independent pillars and awards a **Code Quality Medal** for how many pass:

- **SIMPLE** — avoids unnecessary complexity (AST entropy & CFG cyclomatic complexity)
- **COMPOSABLE** — cleanly decoupled from other modules (MDG Martin instability via [GitNexus](https://github.com/abhigyanpatwari/GitNexus))
- **SECURE** — free of dangerous API reachability and taint paths (CPG analysis; optionally powered by [Sighthound](https://github.com/Corgea/Sighthound))

Topos is the **operator** over those graphs — not another one-off [tree-sitter](https://tree-sitter.github.io/tree-sitter/) script. Specialist engines (GitNexus for the module graph, Sighthound for SAST) feed one medal lattice agents can optimize toward.

| Medal         | Criteria                                    |
| :------------ | :------------------------------------------ |
| 🥇 **GOLD**   | Passes all 3 (SIMPLE + COMPOSABLE + SECURE) |
| 🥈 **SILVER** | Passes 2 of 3                               |
| 🥉 **BRONZE** | Passes 1 of 3                               |
| ❌ **SLOP**   | Passes 0 (or fails to parse)                |

`COMPOSABLE` needs a cross-file dependency graph, which the CLI does not build automatically:

```bash
pnpm add -g gitnexus  # or: npm install -g gitnexus
topos depgraph generate
topos evaluate src/ -r --gitnexus-dir .gitnexus
```

Put [Sighthound](https://github.com/Corgea/Sighthound) on `PATH` to deepen `SECURE` with Corgea's ruleset (auto-detected; local CPG probes still run without it).

Other commands: `topos inspect` for per-file metrics, `topos compare` for AST edit distance between two versions, `topos coverage` for structural test coverage, and `--preferences simple,composable,secure` to tell agents which pillar to protect first when 🥇 GOLD isn't reachable. Full reference: **[docs.krv.ai/topos/cli](https://docs.krv.ai/topos/cli.html)**.

## MCP server (for agents)

Give any MCP-compatible agent — Claude Code, Cursor, Gemini CLI, Windsurf — a live feed of Topos verdicts so it can evaluate and iterate on its own output.

```bash
claude mcp add --transport stdio topos -- topos mcp
```

Setup for Cursor, VS Code, Gemini CLI, Codex, and Windsurf, plus troubleshooting and the full MCP tool list: **[docs.krv.ai/topos/agents](https://docs.krv.ai/topos/agents.html)**.

---

## How it works

Topos measures code along the three pillars above and maps the result to an 8-element evaluation lattice — the three pillars are pairwise incomparable, and 🥇 GOLD is their intersection.

<details>
<summary>Evaluation lattice diagram</summary>

```mermaid
graph BT
    SLOP["❌ SLOP<br/>No Medal"]
    SIMPLE["🥉 BRONZE<br/>Simple"]
    COMPOSABLE["🥉 BRONZE<br/>Composable"]
    SECURE["🥉 BRONZE<br/>Secure"]
    SC["🥈 SILVER<br/>S ∧ C"]
    SSc["🥈 SILVER<br/>S ∧ Sc"]
    CSc["🥈 SILVER<br/>C ∧ Sc"]
    IDEAL["🥇 GOLD<br/>Quality Code"]

    SLOP --> SIMPLE
    SLOP --> COMPOSABLE
    SLOP --> SECURE
    SIMPLE --> SC
    SIMPLE --> SSc
    COMPOSABLE --> SC
    COMPOSABLE --> CSc
    SECURE --> SSc
    SECURE --> CSc
    SC --> IDEAL
    SSc --> IDEAL
    CSc --> IDEAL

    style SLOP       fill:#f8d7da,stroke:#842029,color:#000
    style SIMPLE     fill:#cd7f32,stroke:#5c3a1e,color:#fff
    style COMPOSABLE fill:#cd7f32,stroke:#5c3a1e,color:#fff
    style SECURE     fill:#cd7f32,stroke:#5c3a1e,color:#fff
    style SC         fill:#c0c0c0,stroke:#4a4a4a,color:#000
    style SSc        fill:#c0c0c0,stroke:#4a4a4a,color:#000
    style CSc        fill:#c0c0c0,stroke:#4a4a4a,color:#000
    style IDEAL      fill:#ffd700,stroke:#856404,color:#000
```

</details>

Set your **Preferences** (e.g. `simple,composable,secure`) to tell your coding agent which pillars to prioritize when aiming for GOLD under token and time budgets, and how to relax that goal when GOLD isn't reachable. Details: [docs.krv.ai/topos/preferences](https://docs.krv.ai/topos/preferences.html) · [docs.krv.ai/topos/measures](https://docs.krv.ai/topos/measures.html) · [docs.krv.ai/topos/concepts](https://docs.krv.ai/topos/concepts.html).

## Contributing

Topos is used internally at [Krv Labs](https://krv.ai) to manage AI agent code output. We welcome bugs, ideas, and contributions.

- **Bug?** Open an [Issue](https://github.com/Krv-Labs/topos/issues)
- **Idea?** Start a [Discussion](https://github.com/Krv-Labs/topos/discussions) or open a PR
- **Collaborate?** [team@krv.ai](mailto:team@krv.ai)

---

[Full Documentation](https://docs.krv.ai/topos/) · [Measures & Metrics](https://docs.krv.ai/topos/measures.html) · [Category Theory Concepts](https://docs.krv.ai/topos/concepts.html) · [Engineering notes](docs/)

_Built with ❤️ by [Krv Labs](https://krv.ai)_
