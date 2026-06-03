# Topos: Code Quality Targets for Agents

**Give your coding agent a concrete quality bar to optimize toward — right inside your editor.**

Passing unit tests proves your code *works*. Topos proves it is *built to last*. It measures program **structure** — not just syntax — and exposes those measurements to agent mode as MCP tools. Your agent can then grade its own output and keep iterating until it hits the quality target you set.

On supported platforms the extension bundles the Topos runtime, so there's nothing else to install.

## Try it in 60 seconds

1. **Install** this extension.
2. **Open** a workspace (Python, Rust, JavaScript, TypeScript, or C++).
3. **Ask agent mode:** *"Use Topos to evaluate the code quality of this project."*

That's it. The agent discovers the Topos tools automatically and reports a quality medal per file. To run it yourself instead, open the Command Palette and pick **Topos: Evaluate Project**.

## How scoring works

Topos grades each file on three independent pillars:

| Pillar | What it checks |
| :--- | :--- |
| **SIMPLE** | Avoids unnecessary complexity (AST entropy + cyclomatic complexity). |
| **COMPOSABLE** | Cleanly decoupled from other modules (dependency-graph instability, via GitNexus). |
| **SECURE** | No dangerous-API reachability or taint paths (code-property-graph analysis). |

It then awards a **Code Quality Medal** based on how many pillars pass:

| Medal | Pillars passed |
| :--- | :--- |
| 🥇 GOLD | All 3 (SIMPLE + COMPOSABLE + SECURE) |
| 🥈 SILVER | 2 of 3 |
| 🥉 BRONZE | 1 of 3 |
| ❌ SLOP | 0 (or the file fails to parse) |

When the agent can't reach Gold within your time or token budget, set **Preferences** (e.g. `simple,composable,secure`) to tell it which pillars to prioritize.

> **Note:** SIMPLE and SECURE work out of the box. COMPOSABLE needs a dependency graph — see below — so until you generate one, any verdict requiring COMPOSABLE (including GOLD) is unreachable. SIMPLE and SECURE are unaffected.

## Commands

Open the Command Palette and type "Topos":

- **Topos: Evaluate Project** — scans the workspace for supported languages and runs `topos evaluate -r -v` once per language found.
- **Topos: Generate Dependency Graph** — creates the `.gitnexus/` store that the COMPOSABLE pillar needs.

You can also drive everything conversationally through agent mode — no commands required.

## Enabling COMPOSABLE (GitNexus)

COMPOSABLE additionally needs a dependency graph produced by [GitNexus](https://github.com/abhigyanpatwari/GitNexus):

1. If GitNexus isn't installed, the extension offers a one-click guided install (`npm install -g gitnexus`).
2. Run **Topos: Generate Dependency Graph** to build the `.gitnexus/` store for your workspace.
3. Re-run it when imports change (new modules, renames, restructures).

## Requirements

An MCP-capable host: **VS Code 1.120.0 or newer**, or a compatible editor. If the host doesn't expose the MCP API, the extension says so in the **Topos** output channel rather than failing silently.

**Supported platforms** (the runtime is bundled on each):

- macOS Apple Silicon (`darwin-arm64`)
- macOS Intel (`darwin-x64`)
- Linux x64 (`linux-x64`)
- Linux arm64 (`linux-arm64`)

Native Windows isn't supported yet — use WSL and install the Linux extension-host package through VS Code Remote - WSL.

## Settings

| Setting | Purpose |
| :--- | :--- |
| `topos.executablePath` | Custom path to a Topos executable. Overrides the bundled runtime. |
| `topos.autoDiscover` | Use the active Python environment if it can run `python -m topos.cli`. Compatibility fallback. |
| `topos.autoDownload` | Download a SHA-256-verified standalone binary if no other runtime is found. |
| `topos.evaluatePath` | Directory for **Evaluate Project** (default: `src/` if present, else workspace root). |
| `topos.evaluateLanguage` | `auto` evaluates every detected language (default); set one to restrict the run. |
| `topos.evaluatePreferences` | Optional pillar ranking, e.g. `simple,composable,secure`. |
| `topos.evaluateVerbose` | Per-file medal breakdown in the evaluate terminal (default: `true`). |

## Troubleshooting

The extension resolves the Topos runtime in this order:

1. explicit `topos.executablePath`
2. bundled platform runtime
3. verified cached runtime
4. `topos` on `PATH`
5. active Python environment
6. verified manifest download fallback

If startup fails, open the **Topos** output channel for the exact resolution trace.

---

For more, visit the [Topos GitHub repository](https://github.com/Krv-Labs/topos).
