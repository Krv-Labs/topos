# Topos: Code Quality Targets for Agents

**Structural code-quality targets your coding agents can optimize toward.**

Passing unit tests proves your code works. Topos proves it is built to last. It measures program *structure* — not just syntax — and exposes those measurements as MCP tools so agent mode can evaluate its own output and iterate toward a concrete quality target on every pass. You set the bar; the agent does the work.

On supported platforms the extension bundles the Topos runtime, so there is no separate CLI install.

## The Quality Pillars

Topos scores each file along three independent pillars:

- **SIMPLE** — avoids unnecessary complexity (AST entropy and CFG cyclomatic complexity).
- **COMPOSABLE** — cleanly decoupled from other modules (module-dependency-graph Martin instability, via GitNexus).
- **SECURE** — free of dangerous-API reachability and taint paths (code-property-graph analysis).

It then awards a **Code Quality Medal** based on how many pillars pass:

| Medal | Criteria |
| :--- | :--- |
| GOLD | Passes all 3 (SIMPLE + COMPOSABLE + SECURE) |
| SILVER | Passes 2 of 3 |
| BRONZE | Passes 1 of 3 |
| SLOP | Passes 0 (or fails to parse) |

Set your **Preferences** (e.g. `simple,composable,secure`) to tell the agent which pillars to prioritize when it cannot reach Gold within a time or token budget.

## Features

- **Zero-config MCP server** — registers the Topos MCP server with the editor so agent mode can discover its tools automatically.
- **Bundled runtime** — platform-specific Marketplace packages include the Topos runtime; no manual CLI install on supported platforms.
- **Workspace-aware** — passes the active workspace root to Topos for repo-relative file evaluation.
- **Robust runtime resolution** — bundled binary, verified cache, `PATH`, active Python environment, or a SHA-256-verified download fallback.

## Supported Platforms

- macOS Apple Silicon (`darwin-arm64`)
- macOS Intel (`darwin-x64`)
- Linux x64 (`linux-x64`)
- Linux arm64 (`linux-arm64`)

Native Windows is not supported yet. Use WSL and install the Linux extension-host package through VS Code Remote - WSL.

An MCP-capable host is required (VS Code 1.120.0 or newer, or a compatible editor). If the host does not expose the MCP API, the extension reports it in the **Topos** output channel instead of failing silently.

## Quick Start

1. Install the extension.
2. Open a workspace.
3. Run **MCP: List Servers** and start **Topos** if needed.
4. Ask agent mode: "Use Topos to evaluate the code quality of this project."

Or use the Command Palette:

- **Topos: Evaluate Project** — scans the workspace for supported languages, then runs `topos evaluate -r -v` once per language found (python, rust, javascript, typescript, cpp).
- **Topos: Generate Dependency Graph** — creates `.gitnexus/` for the COMPOSABLE pillar.

## Enabling COMPOSABLE (GitNexus)

`SIMPLE` and `SECURE` work out of the box. `COMPOSABLE` additionally needs a dependency graph produced by [GitNexus](https://github.com/abhigyanpatwari/GitNexus):

1. If GitNexus is not installed, the extension offers a one-click guided install (`npm install -g gitnexus`).
2. Run **Topos: Generate Dependency Graph** from the Command Palette to produce the `.gitnexus/` store for the current workspace.
3. Re-run it when imports change (new modules, renames, restructures).

Until a dependency graph exists, any verdict that requires `COMPOSABLE` (including `GOLD`) is unreachable; `SIMPLE` and `SECURE` are unaffected.

## Settings

- `topos.executablePath`: optional custom path to a Topos executable. Overrides the bundled runtime.
- `topos.autoDiscover`: use the active Python environment when it can run `python -m topos.cli`. Compatibility fallback.
- `topos.autoDownload`: download a SHA-256-verified standalone binary if no bundled, cached, or local runtime is found.
- `topos.evaluatePath`: directory for **Evaluate Project** (default: `src/` if present, else workspace root).
- `topos.evaluateLanguage`: `auto` scans and evaluates every detected language (default); set one language to restrict the run.
- `topos.evaluatePreferences`: optional pillar ranking (e.g. `simple,composable,secure`).
- `topos.evaluateVerbose`: per-file medal breakdown in the evaluate terminal (default: `true`).

## Runtime Resolution

The extension starts Topos in this order:

1. explicit `topos.executablePath`
2. bundled platform runtime
3. verified cached runtime
4. `topos` on `PATH`
5. active Python environment
6. verified manifest download fallback

If startup fails, open the **Topos** output channel for the exact resolution trace.

For more information, visit the [Topos GitHub repository](https://github.com/Krv-Labs/topos).
