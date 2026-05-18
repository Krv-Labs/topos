# Topos Code Quality for VS Code

Bring structural code quality metrics (Simple, Composable, Secure) directly into your agentic coding sessions.

## Features

- **Automated MCP Server Registration:** Topos automatically registers its MCP server with VS Code, making its tools and resources available to Copilot Chat and other agents.
- **Agentic Quality Loops:** Allow your coding agents to query Topos metrics (AST entropy, Martin instability, etc.) to optimize your code structure on every iteration.
- **Zero Configuration:** Simply install the extension. If the Topos CLI is missing, the extension will help you install it with one click.

## Prerequisites

- **Topos CLI:** This extension requires the `topos` CLI. If not found, you will be prompted to install it.
- **VS Code 1.90.0+**: Uses the native Model Context Protocol (MCP) support.

## Quick Start

1. Install the extension.
2. If prompted, click **Install Topos** to set up the CLI.
3. Open a workspace.
4. Ask Copilot Chat: *"Use Topos to evaluate the code quality of this project."*

## Metrics Included

- **🥇 GOLD**: Ideal code (Simple + Composable + Secure)
- **🥈 SILVER**: High quality (2 of 3 pillars)
- **🥉 BRONZE**: Solid foundation (1 of 3 pillars)

For more information, visit [topos.ai](https://topos.ai) or the [GitHub repository](https://github.com/Krv-Labs/topos).
