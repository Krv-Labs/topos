# Topos Code Quality for VS Code

Install Topos code-quality tools for VS Code agent mode through Model Context Protocol (MCP). On supported platforms, no separate Topos CLI install is required.

## Features

- **MCP server registration:** The extension registers the Topos MCP server with VS Code so Copilot Chat agent mode can discover its tools.
- **Agentic quality loops:** Agents can evaluate Simple, Composable, and Secure quality signals while iterating on code.
- **Bundled runtime:** Platform-specific Marketplace packages include the Topos runtime used by the MCP server.
- **Workspace-aware paths:** The extension passes the active workspace root to Topos for repo-relative file evaluation.

## Supported Platforms

- macOS Apple Silicon (`darwin-arm64`)
- macOS Intel (`darwin-x64`)
- Linux x64 (`linux-x64`)
- Linux arm64 (`linux-arm64`)

Native Windows is not supported yet. Use WSL and install the Linux extension host package through VS Code Remote - WSL.

VS Code 1.120.0 or newer is required for the stable MCP server definition provider APIs.

## Quick Start

1. Install the extension.
2. Open a workspace in VS Code.
3. Run **MCP: List Servers** and start **Topos Code Quality** if needed.
4. Ask Copilot Chat agent mode: "Use Topos to evaluate the code quality of this project."

## Settings

- `topos.executablePath`: Optional custom path to a Topos executable. This overrides the bundled runtime.
- `topos.autoDiscover`: Use the active Python environment when it can run `python -m topos.cli`. This is a compatibility fallback.
- `topos.autoDownload`: Download a verified standalone binary if no bundled, cached, or local runtime is available.

## Runtime Resolution

The extension starts Topos in this order:

1. explicit `topos.executablePath`
2. bundled platform runtime
3. verified cached runtime
4. `topos` on `PATH`
5. active Python environment
6. verified manifest download fallback

If startup fails, open the **Topos Code Quality** output channel for the exact resolution trace.

For more information, visit the [Topos GitHub repository](https://github.com/Krv-Labs/topos).
