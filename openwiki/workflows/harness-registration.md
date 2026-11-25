---
type: workflow guide
title: Harness & Plugin Registration Workflow
description: Workflows and life cycle mechanics for installing Topos, registering its MCP server across agent harnesses, managing the VS Code extension, validating agent plugin manifests, and executing ledger-authorized uninstallation.
tags: [workflows, cli, mcp, agents, configuration, safety, installation, plugins, vscode]
verified:
  - by: openwiki/0.9.0
    at: 2026-11-25T22:07:40.661Z
sources:
  - id: openwiki-source-6b1d82c5b3e8f760360a25c5
    resource: repo://.mcp/server.json
  - id: openwiki-source-d34098371626e84da01d5b28
    resource: repo://agent-plugin/plugin.json
  - id: openwiki-source-a08c3147da3563400d745390
    resource: repo://extensions/vscode/package.json
  - id: openwiki-source-1ff55ce1c8213af5491772db
    resource: repo://extensions/vscode/src/extension.ts
  - id: openwiki-source-22f23eea30b1216d0f33ee43
    resource: repo://extensions/vscode/src/runtime.ts
  - id: openwiki-source-03ffc32a0ca502ab67c54b25
    resource: repo://install.sh
  - id: openwiki-source-cde23208842b0b3ea8e3c334
    resource: repo://scripts/check_agent_plugin.py
  - id: openwiki-source-7d7960e96e037fce64b922b5
    resource: repo://topos/cli/src/commands/install/artifact.rs
  - id: openwiki-source-bac29063004a8a9c033c632e
    resource: repo://topos/cli/src/commands/install/binary.rs
  - id: openwiki-source-d2749e43b9698bf39aaa4c5a
    resource: repo://topos/cli/src/commands/install/configure.rs
  - id: openwiki-source-0b18c906d4270531e957d439
    resource: repo://topos/cli/src/commands/install/fsops.rs
  - id: openwiki-source-9151a756a3b86237bb593dce
    resource: repo://topos/cli/src/commands/install/harness.rs
  - id: openwiki-source-008c92d84ac1fe8b75bbf143
    resource: repo://topos/cli/src/commands/install/json_entry.rs
  - id: openwiki-source-1acdcf52cfb4a8b45468a39a
    resource: repo://topos/cli/src/commands/install/mod.rs
  - id: openwiki-source-a9cdf114fd1c04081ae280e1
    resource: repo://topos/cli/src/commands/install/residue.rs
  - id: openwiki-source-888308d02f8dcccc2c448d06
    resource: repo://topos/cli/src/commands/install/skills_entry.rs
  - id: openwiki-source-8777a8141b03df9505221f28
    resource: repo://topos/cli/src/commands/install/state.rs
  - id: openwiki-source-aaba6aaa3b183e811e5ec4a8
    resource: repo://topos/cli/src/commands/install/status.rs
  - id: openwiki-source-81d4650fcc2dad19cb5c7639
    resource: repo://topos/cli/src/commands/install/toml_entry.rs
  - id: openwiki-source-d49b13ade378c85e29a02c95
    resource: repo://topos/cli/src/commands/install/uninstall.rs
  - id: openwiki-source-06d3c16386c87213458c954c
    resource: repo://topos/cli/tests/install_e2e.rs
  - id: openwiki-source-9bf1d8e64277056e6ccedf90
    resource: repo://topos/mcp/src/main.rs
  - id: openwiki-source-3812b1def9fbad0607404761
    resource: repo://topos/mcp/src/server.rs
generated: { by: "openwiki/0.9.0", at: "2026-11-25T22:07:40.661Z" }
---

# Harness & Plugin Registration Workflow

Topos integrates into AI agent workflows through binary distribution, agent plugin manifests, integrated editor extensions, and agent-harness Model Context Protocol (MCP) registration. `topos install`, `topos uninstall`, and `topos status` manage a user-scope MCP integration boundary. For each supported agent harness, Topos manages a single MCP server entry named `topos`, configured with an absolute executable `command` and exactly `args: ["mcp"]`. `topos install status` is an alias for the status workflow.

The core safety rule is a strict single MCP server entry per harness; **pi is the single intentional exception**, as it may additionally receive a path reference to an already-installed Topos skill directory in its own `settings.json`. Topos never writes skill contents, instruction blocks, or `@import` directives. See [distribution surfaces](../integrations/distribution.md) for the ownership boundary between CLI tools, agent plugins, and skill packages.

## Installer Lifecycle and Shell Distribution

Topos supplies an automated shell installer (`install.sh`) for binary deployment alongside direct CLI execution. The installer verifies shell dependencies (`curl`, `mktemp`, `sed`, `grep`, `uname`, `mkdir`, `mv`, `chmod`), preflights existing installations (such as Homebrew or existing binary builds), deploys the `topos` binary into `INSTALL_DIR` (`~/.local/bin` by default), configures shell profile PATH entries when required (using `# BEGIN TOPOS INSTALLER PATH` markers), records an installation provenance record in `${XDG_STATE_HOME:-$HOME/.local/state}/topos/install-provenance`, and verifies execution.

```mermaid
flowchart TD
    Start["Execute install.sh"] --> CheckDeps["Check dependencies: curl, mktemp, sed, grep, uname, mkdir, mv, chmod"]
    CheckDeps --> ValidateDir["Validate INSTALL_DIR (default ~/.local/bin)"]
    ValidateDir --> Preflight["Preflight existing installs (Homebrew, PATH, previous versions)"]
    Preflight --> FetchBin["Download and extract Topos binary for target OS and Architecture"]
    FetchBin --> SetupPath["Setup shell profile PATH (# BEGIN TOPOS INSTALLER PATH)"]
    SetupPath --> WriteProv["Write provenance file (~/.local/state/topos/install-provenance)"]
    WriteProv --> Verify["Verify execution and print agent guidance (topos --version, agent setup)"]
```
*Installer lifecycle flow for shell-based binary distribution and environment setup.*

Topos also provides standardized plugin and package metadata manifests for registry discovery and dedicated MCP transport execution:
- `.mcp/server.json`: Standard Model Context Protocol server manifest (conforming to schema version `2025-12-11` at `https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json`), declaring server identity `io.github.Krv-Labs/topos`, package registry bindings for PyPI package `topos-mcp` (version `0.16.0`), `uvx` runtime hints, and stdio transport configurations.
- `agent-plugin/plugin.json`: Plugin manifest for agent plugin ecosystems (conforming to `agent-plugins.org` schema `1.0.0` at `https://agent-plugins.org/schemas/1.0.0/plugin.schema.json`), declaring plugin name `topos` (version `0.16.0`), ownership (`Krv Labs`, `https://krv.ai`), licensing (`BSD-3-Clause`), and repository links (`https://github.com/Krv-Labs/topos`).
- `topos-mcp`: Dedicated stdio binary (`topos/mcp/src/main.rs`) serving Model Context Protocol requests directly or launched via `topos mcp`. It honors `--version` (`-V`) and `--help` (`-h`) flags directly without initiating stdio MCP transport, preventing handshake stalls during diagnostic queries or host discovery inspect passes.

## Supported Agent Harnesses and Metadata Formats

The `HARNESSES` table defines the supported agent integration targets. Each entry declares the harness identifier, user configuration path, artifact format (`McpJson`, `McpToml`, `VsCodeJson`, `OpenCodeJson`), presence detector function, user messages, operational notes/caveats, and whether the pi skill reference applies. Command logic iterates this table rather than hardcoding harness-specific branches. Detection pre-checks interactive choices via `detect_*` predicates (`binary_installed` and `app_installed`), probing PATH, user bin candidates (`~/.local/bin`, `~/.cargo/bin`, `~/.opencode/bin`, `~/bin`, `%APPDATA%\npm`, `%LOCALAPPDATA%\npm`, `%LOCALAPPDATA%\Microsoft\WindowsApps`), and platform desktop app paths (`/Applications`, `%ProgramFiles%`, `/usr/share`, `/opt`), but detection never blocks an explicit command-line selection.

| Id | Harness | User configuration | Topos-owned MCP location | Format / Structure |
| --- | --- | --- | --- | --- |
| `claude` | Claude Code | `~/.claude.json` | `mcpServers.topos` | `McpJson` (`command`, `args`) |
| `claude-desktop` | Claude Desktop | platform-specific Claude Desktop configuration | `mcpServers.topos` | `McpJson` (`command`, `args`) |
| `codex` | Codex CLI | `~/.codex/config.toml` | `[mcp_servers.topos]` | `McpToml` (TOML table) |
| `gemini` | Gemini CLI | `~/.gemini/settings.json` | `mcpServers.topos` | `McpJson` (`command`, `args`) |
| `copilot` | GitHub Copilot CLI | `~/.copilot/mcp-config.json` | `mcpServers.topos` | `McpJson` (`command`, `args`) |
| `cursor` | Cursor | `~/.cursor/mcp.json` | `mcpServers.topos` | `McpJson` (`command`, `args`) |
| `vscode` | VS Code | platform-specific `Code/User/mcp.json` | `servers.topos` | `VsCodeJson` (`type: "stdio"`, `command`, `args`) |
| `antigravity` | Google Antigravity | `~/.gemini/config/mcp_config.json` | `mcpServers.topos` | `McpJson` (`command`, `args`) |
| `pi` | pi | `~/.pi/agent/mcp.json` | `mcpServers.topos` | `McpJson` (`command`, `args`) + pi skill ref |
| `opencode` | OpenCode | `~/.config/opencode/opencode.jsonc` (or `opencode.json` / `config.json`) | `mcp.topos` | `OpenCodeJson` (`type: "local"`, array `command`) |

Platform paths adapt across environments: Claude Desktop and VS Code target `~/Library/Application Support/...` on macOS, `~/.config/...` on Linux, and `%APPDATA%/...` on Windows. OpenCode configuration resides in `~/.config/opencode/`, probing `opencode.jsonc`, `opencode.json`, or `config.json` (defaulting to `opencode.jsonc`). Path resolution functions accept the home directory, enabling end-to-end testing against isolated scratch directories without affecting developer configurations.

Plain JSON registrations specify `command` and `args`; Codex uses `toml_edit` to preserve comments and formatting; VS Code requires `"type": "stdio"`; OpenCode uses `mcp` with `"type": "local"` and an array `command` containing `["<binary>", "mcp"]`. The resolved command is stored as an absolute path, while command comparison uses file identity (inode/file-id) so stable `$PATH` symlinks can be recorded without version churn. Automated configuration via `topos install` manages these registrations across all supported harnesses including VS Code, Claude Desktop, Cursor, and Gemini CLI.

## VS Code Extension Integration (`topos-vscode`)

The VS Code extension (`extensions/vscode`) provides native MCP server provider registration and command workflows directly within VS Code and compatible editor hosts. Contributed under provider ID `topos-mcp`, the extension registers an `McpServerDefinitionProvider` via `vscode.lm.registerMcpServerDefinitionProvider` to expose the Topos stdio MCP server to VS Code's Language Model features and agent tools.

```mermaid
flowchart TD
    Activate["Extension activation"] --> CheckPlatform{"Platform check"}
    CheckPlatform -- Native Windows --> WarnWin["Show warning advising WSL or pip install topos"]
    CheckPlatform -- macOS / Linux / WSL --> CheckApi{"Check vscode.lm & McpStdioServerDefinition"}
    CheckApi -- Unsupported Host --> WarnApi["Show host warning and abort provider setup"]
    CheckApi -- API Available --> RegProvider["Register topos-mcp ServerDefinitionProvider"]
    
    RegProvider --> ResolveExec["Resolve Topos executable path via Fallback Hierarchy"]
    ResolveExec --> Step1["1. Custom executablePath override"]
    Step1 -- Found --> BuildDef
    Step1 -- Empty --> Step2["2. Bundled binary (bin/topos)"]
    Step2 -- Found --> BuildDef
    Step2 -- Missing --> Step3["3. Cached binary (globalStorage/topos-cli)"]
    Step3 -- Found --> BuildDef
    Step3 -- Missing --> Step4["4. Global $PATH binary"]
    Step4 -- Found --> BuildDef
    Step4 -- Missing --> Step5{"5. autoDiscover enabled?"}
    Step5 -- Yes --> TryVenv["Try active Python venv executable"]
    TryVenv -- Found --> BuildDef
    TryVenv -- Not Found --> Step6{"6. autoDownload enabled?"}
    Step5 -- No --> Step6
    Step6 -- Yes --> DownloadBin["Fetch release manifest & SHA-256 verify binary"]
    DownloadBin -- Success --> BuildDef
    
    BuildDef["Set TOPOS_MCP_FILE_ROOT env to workspace root & launch command with args: ['mcp']"]
```
*VS Code extension activation, host compatibility checks, and executable resolution fallback hierarchy.*

### Host Preflight and Platform Support

1. **Platform Guard**: Topos native binaries target macOS, Linux, and WSL environments. When activated on native Windows (`win32`), activation displays an actionable warning advising the user to open the workspace in WSL (Windows Subsystem for Linux) or install the Python CLI via `pip install topos`.
2. **MCP Feature Detection**: Before registering the provider, `isMcpApiAvailable(vscode)` verifies that `vscode.lm.registerMcpServerDefinitionProvider` and `vscode.McpStdioServerDefinition` are present on the host. If missing (e.g., on older VS Code host builds or derivative editors), activation halts gracefully without throwing errors and surfaces a warning prompt pointing to documentation.

### Executable Resolution Fallback Hierarchy

When resolving the executable path for the MCP server definition, `resolveToposExecutable` evaluates a strict priority sequence:
1. **Custom Path Override**: Setting `topos.executablePath` explicitly points to a local Topos binary or Python interpreter path.
2. **Bundled Runtime**: Checks for a pre-bundled binary in the extension directory (`bin/topos`).
3. **Cached Standalone Binary**: Checks extension storage (`globalStorageUri/topos-cli`) for a previously downloaded binary.
4. **Global `$PATH` Executable**: Probes system `$PATH` for an executable named `topos`.
5. **Python Virtual Environment Discovery**: When `topos.autoDiscover` is enabled (default `true`), probes active Python virtual environment paths for `topos` or Python module invocations (`python -m topos.cli mcp`).
6. **Release Manifest Download**: When `topos.autoDownload` is enabled (default `true`), fetches the release manifest (`releases.json`), selects the binary for the detected platform (`darwin-arm64`, `linux-x64`, or `linux-arm64`), downloads the binary, and verifies its SHA-256 hash before placing it into extension storage.

### Workspace Environment and Invocation

Upon successful resolution, `provideMcpServerDefinitions` and `resolveMcpServerDefinition` construct a `vscode.McpStdioServerDefinition` with the resolved command, `args: ["mcp"]`, and environment variable `TOPOS_MCP_FILE_ROOT` bound to the active workspace folder path (`workspaceFolders[0].uri.fsPath`). This scoping ensures relative tool parameters evaluate within the active VS Code project directory.

The extension also registers Command Palette actions (`topos.generateDependencyGraph` and `topos.evaluateProject`) and performs a non-blocking background check for GitNexus availability (`checkGitNexusAvailability`). If GitNexus is missing, it displays a dismissible guidance prompt and offers guided installation using `resolveGitNexusInstallCommand` (preferring `pnpm add -g gitnexus` when `pnpm` is on `$PATH`, falling back to `npm install -g gitnexus@1.6.8`).

## Agent Plugin Manifest Validation (`scripts/check_agent_plugin.py`)

Topos maintains an Agent Plugins 1.0 package under `agent-plugin/`. The validation script `scripts/check_agent_plugin.py` enforces Agent Plugins 1.0.0 and MCP 1.0.0 specification compliance, ensuring all published plugin files are schema-closed and cross-platform safe.

### Manifest and Schema Contracts

- **`agent-plugin/plugin.json`**: Evaluated against schema `https://agent-plugins.org/schemas/1.0.0/plugin.schema.json`. Must declare:
  - `name`: Must match pattern `^(?!.*(?:--|\.\.))[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$` and be at most 64 characters long.
  - `version`: Must match the workspace version declared in `Cargo.toml`.
  - Top-level strictness: Only allowed fields (`$schema`, `name`, `version`, `description`, `author`, `homepage`, `repository`, `license`, `keywords`, `extensions`) are permitted.
  - `author`: Must be an object with valid string fields (`name`, `email`, `url`).
- **`agent-plugin/mcp.json`**: Evaluated against schema `https://agent-plugins.org/schemas/1.0.0/mcp.schema.json`.
  - Supported transport types: `stdio`, `streamable-http`, `sse`.
  - Stdio `command` constraints: Single-token string (no internal whitespace), must be a bare executable name or `./`-prefixed plugin-relative path, and must not contain any `..` path traversal segments.
  - Stdio `cwd` constraints: Must begin with `./`, `${PLUGIN_ROOT}`, or `${PLUGIN_DATA}`, and must not contain `..` path traversal segments.
  - Reserved environment variables: Stdio server `env` objects must not contain `PLUGIN_ROOT` or `PLUGIN_DATA`, as client runtimes supply these dynamically.

### Package Integrity and Symlink Policy

To ensure clean checkout and extraction across all platforms (particularly Windows where git symlinks require elevated privileges or non-default flags), `scripts/check_agent_plugin.py` enforces strict structural rules:
1. **Symlink Prohibition**: No file under `agent-plugin/` may be a symbolic link. Every file must be a regular file.
2. **Skill Byte Synchronization**: The packaged skill document `agent-plugin/skills/topos/SKILL.md` must exist, be a regular file, and be byte-identical to the canonical source file `skills/topos/SKILL.md`.

## State Machine, Field-Level Ownership, and Non-Mutation

```mermaid
stateDiagram-v2
    [*] --> Absent
    Absent --> Active: install adds owned entry
    Incomplete --> Active: install repairs owned entry
    Active --> Active: install makes no write
    Active --> Absent: uninstall removes owned entry
    Incomplete --> Absent: uninstall removes owned entry
    Absent --> Absent: uninstall has nothing to remove
    Conflict --> Conflict: report and preserve
```
*Per-harness MCP Artifact lifecycle. Conflict deliberately has no mutation transition; pi's separate skill-reference outcome is reported alongside.*

- **Active**: The `topos` entry is recognized as Topos-owned, possesses required configuration fields, and points to an executable binary matching the running executable.
- **Incomplete**: The entry is owned by Topos but exhibits path drift (pointing to an old executable), missing `type: stdio` (VS Code), or missing `type: local` / array `command` formatting (OpenCode). Re-running `topos install <id>` repairs the entry.
- **Conflict**: The configuration file is unparsable JSON/TOML, a required container object has an incompatible type, the `topos` key is not recognized as Topos-owned, or a necessary VS Code or OpenCode JSONC update would strip comments or trailing commas. `install` refuses modification, and `uninstall` leaves the file untouched.
- **Absent**: No Topos entry exists. `uninstall` is idempotent and does not create ledger records to record absence.

Topos uses field-level ownership rules rather than whole-object comparison. An entry is recognized as Topos-owned when its executable filename is `topos` or `topos.exe` and its arguments are exactly `["mcp"]` (either as a standalone `args` array or as the second element in OpenCode's combined `command` array), even if the path no longer exists. This enables repairing or removing stale paths while preventing accidental modification of hand-authored `topos` entries. Writers modify only Topos-owned keys (`command`, `args`, and `type`) while preserving sibling server definitions, foreign keys, and existing formatting.

```mermaid
flowchart TD
    Req["topos install [harness]"] --> ResolveBin["Resolve binary path (absolute file path)"]
    ResolveBin --> InspectState["Inspect harness config (Artifact state and field-level check)"]
    InspectState --> CheckState{"Config State"}
    CheckState -- Active --> NoWrite["No write needed (idempotent report)"]
    CheckState -- Conflict --> AbortConflict["Refuse write (preserve foreign JSONC and comments)"]
    CheckState -- Absent or Incomplete --> CreateBackup["Create .topos.backup (only if pre-existing file)"]
    CreateBackup --> AtomicWrite["Atomic write via temp file (field-level update, preserve foreign keys)"]
    AtomicWrite --> RecordLedger["Record created file and dirs in topos-install-ledger.json"]
    RecordLedger --> CheckPi{"Harness is pi?"}
    CheckPi -- No --> Done["Registration Complete"]
    CheckPi -- Yes --> PiSkill["Check pi skill source (~/.pi/agent/skills or candidate dirs)"]
    PiSkill --> PiRef{"Skill Source"}
    PiRef -- Discovered --> PiDone["Report discovered in native dir"]
    PiRef -- Missing --> PiAdvice["Report openclaw install advice"]
    PiRef -- Referenceable --> PiWrite["Append candidate dir to skills array and record addedSkillRef in ledger"]
    PiWrite --> Done
```
*Detailed harness registration pathway illustrating binary resolution, inspection, backup creation, atomic updates, ledger authorization, and pi skill handling.*

## Install Flow and Pi's Skill Directory Reference

When modifying a pre-existing configuration file, `topos install` writes a pristine backup snapshot to `<config>.topos.backup` prior to modification. No backup is created when Topos creates a new configuration file from scratch, and existing pristine backups are preserved untouched during subsequent repair operations. Re-running `install` on an already active configuration performs no file writes.

pi lacks a native MCP client: its MCP registration is inactive until the user installs the `pi-mcp-adapter` extension, so `topos status` and `topos install` output include adapter caveats. pi also supports a direct skill directory integration pathway through `~/.pi/agent/settings.json`:

| Skill source outcome | `topos install pi` behavior |
| --- | --- |
| Topos skill present in `~/.pi/agent/skills` | No reference written; pi discovers it natively (`Discovered`). |
| Topos skill in `~/.agents/skills` or `~/.claude/skills` | Append candidate directory to `skills` array (`Referenceable`). |
| No installed Topos skill located | Do not create `settings.json`; report `openclaw skills install @Krv-Labs/topos` guidance (`Missing`). |

This mechanism references an external skill search directory rather than copying skill files. Insertion is idempotent, appends to the existing `skills` array without overwriting foreign entries, and preserves unrelated settings. `topos status --json` reports pi's `skillRef` status independently (`active`, `conflict`, `discovered`, or `unavailable`) so that MCP server status and skill reference status remain distinct.

## Ledger-Authorized Cleanup and Uninstallation

Topos uses a persistent state ledger file—`install.json` (also referenced as the `topos-install-ledger.json` state ledger)—located at `${XDG_STATE_HOME:-$HOME/.local/state}/topos/install.json` (or `%APPDATA%\topos\install.json` on Windows) to track ownership authority. The ledger records:
- Files created by `install` per harness (`createdFiles`).
- Directory paths created by `install` writes (`createdDirs`).
- Pi skill references appended by `install` (`addedSkillRef`).

```json
{
  "harnesses": {
    "pi": {
      "createdFiles": ["/home/user/.pi/agent/mcp.json"],
      "addedSkillRef": true
    }
  },
  "createdDirs": ["/home/user/.pi", "/home/user/.pi/agent"]
}
```

The ledger accepts legacy flat schema formats for backward compatibility and upgrades them to the current schema on write.

`topos uninstall` removes only recognized, Topos-owned MCP entries. An empty configuration file is deleted only if the ledger confirms Topos created it. For pi, the skill array reference is removed only if recorded in `addedSkillRef`. If the `skills` array becomes empty, the key is removed, but external skill directories and content are never deleted.

Uninstallation performs cleanup in a strict sequence to preserve authorization evidence:
1. Purge `.topos.backup` files when requested (`--purge-backups`) for selected harnesses only.
2. Remove Topos MCP entries and clear harness ledger records.
3. If all managed MCP registrations and ledger-owned skill references have been removed, prune recorded directories deepest-first if effectively empty (allowing minor system artifacts like `.DS_Store`).
4. Delete `install.json` (the installer state ledger) and prune the state directory last.

Protected system roots—including `$HOME`, `~/.local`, `~/.local/state`, `~/.config`, `~/Library`, `~/Library/Application Support`, and `%APPDATA%`—are excluded from directory pruning. Shared cache indexing data (`~/.local/state/topos/cache-index/`) is preserved across uninstallation and reported in human-readable output.

## Status Inspection and Read-Only Residue

`topos status` inspects the executable path, checks all entries in `HARNESSES`, scans for legacy residue, and prints structured console output or machine-readable JSON (`topos status --json`). The JSON output includes resolved binary paths, active count totals, per-harness status details, residue records, and pi skill reference state.

Residue scanning is strictly read-only. `topos status` identifies draft-era Copilot instruction blocks, Gemini `@import` directives, copied skill files, and foreign-key duplicate MCP registrations. Topos never modifies or deletes residue artifacts during install or uninstall operations because these files may be shared across tools or managed by external distribution channels (such as OpenClaw, ClawHub, or Hermes).

Google Antigravity includes a specific migration warning: prior to migration, a legacy data-directory `mcp_config.json` can overwrite `~/.gemini/config/mcp_config.json` on application launch. Migration markers or back-compatibility symlinks suppress the warning. Topos never writes directly to legacy data directories.

## Invocation Safety and Verification Suite

Harness arguments are case-insensitive and deduplicated; `--all` processes harnesses in `HARNESSES` table order. In non-interactive environments, `install` requires explicit harness identifiers or `--all`. Headless `uninstall` operations without explicit identifiers select non-absent integrations and execute automatically without prompting. If `stderr` is redirected while `stdin` or `stdout` remains a TTY (ambiguous interactivity), destructive uninstall requires `--yes` or `--dry-run`; interactive runs display planned actions and default to cancellation.

The end-to-end integration test suite (`cargo test -p topos --test install_e2e`) verifies system contracts using the compiled binary against isolated scratch `$HOME` environments with full directory and file snapshotting. Key assertions verified by the suite include:
- Execution of absolute command paths and exact `["mcp"]` arguments across all 10 harnesses.
- Idempotency of repeated `install` runs with zero file rewrites.
- Drift detection and command repair without clobbering pristine pre-install backups.
- Scoped backup purging (`--purge-backups`) that leaves unselected harness backups intact.
- Headless uninstallation and leave-no-trace directory pruning.
- Read-only preservation of residue artifacts, foreign settings, and manually added pi skill references.
- Byte-identical refusal of commented VS Code JSONC files.

Detailed CLI command structures are covered in [Agent and CLI workflows](agent-and-cli.md), and release testing procedures are detailed in [testing and release](../operations/testing-and-release.md).
