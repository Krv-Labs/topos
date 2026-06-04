# Topos VS Code MCP Extension Publishing Guide

This guide covers local testing, VSIX packaging, Marketplace setup, GitHub secrets, release publishing, and the runtime design for the Topos VS Code MCP extension.

> [!IMPORTANT]
> The production install goal is "install the VS Code extension and the MCP tools work." Users should not have to install the Topos CLI manually on supported platforms.

Extension root:

```bash
cd <your-local-topos-repo>/extensions/vscode
```

- [Runtime Resolution](#runtime-resolution)
- [Local Verification](#local-verification)
- [Local Install Without Bundled Runtime](#local-install-without-bundled-runtime)
- [Local Install With Bundled Runtime](#local-install-with-bundled-runtime)
- [Cursor Compatibility](#cursor-compatibility)
- [VS Code Marketplace Setup](#vs-code-marketplace-setup)
- [Required GitHub Secrets](#required-github-secrets)
- [GitHub Actions Flow](#github-actions-flow)
- [Publishing A New Version](#publishing-a-new-version)
- [Manual Marketplace Publish](#manual-marketplace-publish)
- [File Inclusion Rules](#file-inclusion-rules)
- [Size Gate](#size-gate)
- [Troubleshooting](#troubleshooting)
- [Release Checklist](#release-checklist)


The extension is published as platform-specific VSIX packages:

- `darwin-arm64`
- `darwin-x64`
- `linux-arm64`
- `linux-x64`

We do this because the extension should work after install without asking the user to install the Topos CLI separately. Each target VSIX includes exactly one platform runtime at:

```text
extension/bin/topos
```

VS Code Marketplace selects the VSIX matching the user's platform.

Native Windows is not currently supported. Windows users should use VS Code Remote - WSL and install the Linux extension host package.

> [!NOTE]
> Platform-specific VSIXs are intentional. A single universal package would either omit the runtime or bundle every runtime, making the package larger and more brittle.

> [!WARNING]
> macOS packages are only production-grade when the bundled binary is signed and notarized. The workflow can continue without Apple credentials, but macOS users may hit Gatekeeper warnings or blocked execution.

## Runtime Resolution

At activation time, the extension registers a VS Code MCP server definition provider. When VS Code resolves the MCP server, the extension resolves the Topos runtime in this order:

1. `topos.executablePath`
2. bundled platform runtime inside the VSIX
3. verified cached runtime from prior download
4. `topos` on `PATH`
5. active Python environment via `python -m topos.cli`
6. verified manifest download fallback

The intended user experience is: install the VS Code extension, open a workspace, and the MCP tools are available to VS Code agents.

The PATH/Python/download paths are compatibility fallbacks, not the primary production path.

<details>
<summary>Why this fallback order exists</summary>

- `topos.executablePath` lets power users override everything.
- The bundled runtime is the normal Marketplace path.
- The verified cache avoids repeated downloads.
- `PATH` and Python environments support developers who already have Topos installed.
- Manifest download is last because users with a valid local runtime should not trigger network work.

</details>

## Local Verification

Run the extension checks:

```bash
cd <your-local-topos-repo>/extensions/vscode
npm install
npm run check-types
npm run lint
npm test
npm run package
```

What these do:

- `check-types`: TypeScript API/type validation.
- `lint`: ESLint pass over extension source.
- `test`: binary staging behavior plus `test:unit` (pure runtime logic — platform mapping, path resolution, SHA-256, manifest selection, redirect/timeout handling, MCP API detection).
- `package`: production esbuild bundle into `dist/extension.js`.

To run the VS Code integration smoke tests (downloads a full VS Code build, so it is local-first / opt-in):

```bash
npm run test:integration
```

> [!TIP]
> Run these checks before packaging a local VSIX. They catch the common failure modes faster than installing into VS Code.

## Local Install Without Bundled Runtime

This tests extension registration and fallback behavior, but does not test the target VSIX bundled-runtime path.

```bash
cd <your-local-topos-repo>/extensions/vscode
npx --yes @vscode/vsce package --out topos-vscode-local.vsix
code --install-extension topos-vscode-local.vsix --force
code <your-local-topos-repo>
```

Then in VS Code:

```text
Developer: Reload Window
MCP: List Servers
Output: Topos
```

> [!NOTE]
> This path is useful for extension-host smoke testing, but it is not the production Marketplace path because the VSIX does not include `extension/bin/topos`.

## Local Install With Bundled Runtime

Build a local Topos binary from repo root:

```bash
cd <your-local-topos-repo>

uv run --with pyinstaller pyinstaller --name topos --onefile --clean \
  --collect-all tree_sitter \
  --collect-all tree_sitter_python \
  --collect-all tree_sitter_rust \
  --collect-all tree_sitter_javascript \
  --collect-all tree_sitter_cpp \
  --collect-all tree_sitter_typescript \
  --collect-all topos \
  --collect-all fastmcp \
  --collect-all real-ladybug \
  --copy-metadata fastmcp \
  topos/cli/main.py
```

Rename the binary for your local platform:

```bash
# Apple Silicon macOS
mv dist/topos dist/topos-macos-arm64

# Intel macOS
mv dist/topos dist/topos-macos-amd64

# Linux x64
mv dist/topos dist/topos-linux-amd64

# Linux arm64
mv dist/topos dist/topos-linux-arm64
```

Package and install the matching target:

```bash
cd <your-local-topos-repo>/extensions/vscode

# Apple Silicon macOS
npm run package:darwin-arm64
code --install-extension topos-vscode-darwin-arm64.vsix --force

# Intel macOS
npm run package:darwin-x64
code --install-extension topos-vscode-darwin-x64.vsix --force

# Linux x64
npm run package:linux-x64
code --install-extension topos-vscode-linux-x64.vsix --force

# Linux arm64
npm run package:linux-arm64
code --install-extension topos-vscode-linux-arm64.vsix --force
```

After install:

```bash
code <your-local-topos-repo>
```

Then in VS Code:

```text
Developer: Reload Window
MCP: List Servers
Output: Topos
```

Expected log signal:

```text
Using bundled Topos runtime: .../extension/bin/topos
Resolved MCP server command: .../extension/bin/topos mcp
```

<details>
<summary>Quick bundled-runtime verification</summary>

Check the VSIX contents:

```bash
unzip -l topos-vscode-darwin-arm64.vsix | rg "extension/bin/topos|extension/dist/extension.js|extension/package.json"
```

Expected:

```text
extension/bin/topos
extension/dist/extension.js
extension/package.json
```

</details>

## Host version policy

The extension uses two independent gates:

| Gate | Value | Purpose |
| --- | --- | --- |
| **`engines.vscode`** | `^1.105.0` | Marketplace / host install floor. Matches Cursor 2.1+ (About: VS Code API 1.105.1+). |
| **MCP runtime** | `isMcpApiAvailable()` | Agent MCP tools. Upstream API ships in VS Code 1.120; Cursor may backport it while still reporting 1.105.x. |

Do **not** bump `engines.vscode` to ^1.120 to satisfy Copilot-style reviews: that blocks Cursor installs where the reported API level is 1.105.x even when `vscode.lm` is present.

**Cursor 2.0.x** (reports 1.99.x) is out of scope for Marketplace install. Forcing a VSIX install will activate with a runtime MCP warning.

**PR reply template (Copilot / reviewers):** We keep `engines.vscode` at ^1.105.0 so Cursor 2.1+ can install; MCP is gated at runtime via `isMcpApiAvailable()` because the MCP provider API landed in VS Code 1.120 while Cursor reports 1.105.x. README documents the split. Adopted: `resolveHomePath` fix, `didChangeEmitter` disposal, host logging in the output channel.

## Cursor Compatibility

Cursor is built on a VS Code base that can lag the upstream API. The extension is written to fail safe there: if the host does not expose the MCP API, activation does not throw — it logs to the **Topos** output channel and shows a single actionable warning. Use this checklist to verify Cursor before ticking the PR's "Cursor Compatibility" boxes.

Install the matching platform VSIX into Cursor:

```bash
cursor --install-extension topos-vscode-darwin-arm64.vsix --force
```

> [!NOTE]
> If the `cursor` CLI is not on `PATH`, install via the Cursor UI: Extensions view -> `...` menu -> **Install from VSIX**.

### 1. Cursor loads the extension from the `.vsix`

- After install, open the Extensions view and confirm **Topos: Code Quality Targets for Agents** is listed and enabled.
- Open the **Topos** output channel (Output panel -> "Topos" in the dropdown).
- Expected: `Topos extension activating...` and `Host: Cursor <version>` (or similar). No activation error notification.

### 2. MCP server registration works (`registerMcpServerDefinitionProvider`)

- Expected output-channel line: `Topos MCP Server Provider registered successfully.`
- If instead you see `This host does not expose the MCP server definition API` with `vscode.lm` / `McpStdioServerDefinition` flags, the Cursor build lacks the MCP provider API. This is the expected fail-safe path — the extension warns and does not crash. Note the `Host:` line and Cursor version and stop here; the remaining boxes require an MCP-capable host.

### 3. CLI resolution / install flow works

- Trigger MCP server resolution (open the MCP servers panel and start **Topos**, or invoke a Topos tool from the agent).
- Expected output-channel trace ends with `Resolved MCP server command: .../bin/topos mcp` (bundled runtime) or another resolved step.
- For the GitNexus path, run **Topos: Generate Dependency Graph** from the Command Palette and confirm the guided-install prompt appears when `gitnexus` is absent, and that a `.gitnexus/` store is written when it is present.

### 4. Cursor agent can discover and use the MCP tools

- In the Cursor agent, ask: "Use Topos to evaluate the code quality of this file."
- Expected: the agent lists/invokes Topos tools and returns a verdict (e.g. `{simple, secure}`, plus `composable` once a dependency graph exists).

> [!TIP]
> The output channel is the source of truth. Every resolution step and every fail-safe branch is logged there, which makes Cursor-specific behavior easy to confirm without a debugger.

## VS Code Marketplace Setup

Marketplace publishing uses `vsce`.

Official docs:

- https://code.visualstudio.com/api/working-with-extensions/publishing-extension
- https://code.visualstudio.com/api/working-with-extensions/continuous-integration

One-time setup:

1. Create or verify the Visual Studio Marketplace publisher.
2. Confirm the publisher ID exactly matches `package.json`.
3. Current expected publisher:

```text
KrvLabs
```

The Marketplace extension identity is:

```text
KrvLabs.topos-vscode
```

If the publisher ID is wrong, fix `publisher` in `extensions/vscode/package.json` before publishing. Publisher/name identity is user-facing and hard to unwind after first publish.

> [!IMPORTANT]
> Verify the publisher ID before the first public publish. The extension identity is `<publisher>.<name>`, currently `KrvLabs.topos-vscode`.

## Required GitHub Secrets

### `VSCE_PAT`

Required for Marketplace publishing from GitHub Actions.

This is an Azure DevOps PAT, not a GitHub PAT.

PAT settings:

```text
Organization: All accessible organizations
Scopes: Custom defined
Marketplace: Manage
```

GitHub secret:

```text
Name: VSCE_PAT
Value: <Azure DevOps Marketplace Manage PAT>
```

Why it exists:

- `vsce publish` authenticates against Visual Studio Marketplace through Azure DevOps.
- GitHub Actions exposes the token only to the publish step.
- The workflow uses the `vscode-marketplace` environment so the repo can require manual approval before publishing.

Recommended GitHub setup:

```text
Settings -> Environments -> New environment -> vscode-marketplace
Settings -> Environments -> vscode-marketplace -> Required reviewers
```

> [!CAUTION]
> `VSCE_PAT` should be an Azure DevOps PAT with only `Marketplace: Manage`. Do not use a broad GitHub PAT, and do not grant source-code or build scopes.

### Apple Signing Secrets

These are optional for the workflow to complete, but required for production-quality macOS distribution.

```text
APPLE_DEVELOPER_ID_CERTIFICATE_P12_BASE64
APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD
APPLE_DEVELOPER_IDENTITY
APPLE_ID
APPLE_TEAM_ID
APPLE_APP_SPECIFIC_PASSWORD
```

Why they exist:

- `APPLE_DEVELOPER_ID_CERTIFICATE_P12_BASE64`: Developer ID certificate used by `codesign`.
- `APPLE_DEVELOPER_ID_CERTIFICATE_PASSWORD`: password for the `.p12` certificate.
- `APPLE_DEVELOPER_IDENTITY`: signing identity passed to `codesign`.
- `APPLE_ID`: Apple account used by `notarytool`.
- `APPLE_TEAM_ID`: Apple Developer team identifier.
- `APPLE_APP_SPECIFIC_PASSWORD`: app-specific password for notarization.

Current behavior if missing:

- macOS build continues.
- GitHub Actions emits a warning.
- macOS binaries are unsigned/unnotarized.
- macOS users may see Gatekeeper warnings or blocked execution.

Linux builds are unaffected.

<details>
<summary>Apple credential status during early releases</summary>

Until Apple credentials are configured:

- release jobs continue
- GitHub Actions emits warnings
- Linux packages are unaffected
- macOS packages may install but fail to execute the bundled runtime under Gatekeeper

Once Apple credentials are configured, the same workflow signs and notarizes macOS binaries automatically.

For PyInstaller `onefile` builds, macOS signing must happen **during** collection (`--codesign-identity` / `--osx-entitlements-file`), not only on the outer executable afterward. Post-hoc signing of the wrapper alone leaves embedded `libpython` adhoc-signed and causes runtime `different Team IDs` errors under hardened runtime.

</details>

## GitHub Actions Flow

Workflow:

```text
.github/workflows/release.yml
```

### Pull Requests

Triggered on PRs to `main`.

What runs:

1. Build Topos binaries for all target platforms.
2. Package target VSIXs.
3. Check VSIX size.
4. Upload VSIX artifacts.

What does not run:

- GitHub Release creation.
- Marketplace publish.
- macOS signing/notarization.

Purpose:

- Validate that the extension can build and package before merge.

> [!NOTE]
> PRs validate build and package behavior only. They do not publish to Marketplace and do not create GitHub Releases.

### Tag Releases

Triggered by tags:

```text
v*
```

Example:

```bash
git tag v0.3.0
git push origin v0.3.0
```

What runs:

1. Build platform binaries.
2. Sign/notarize macOS binaries if Apple secrets exist.
3. Package platform VSIXs.
4. Size-check VSIXs.
5. Create GitHub Release with binaries, VSIXs, and checksums.
6. Publish all platform VSIXs to VS Code Marketplace using `VSCE_PAT`.

> [!IMPORTANT]
> Tag releases are the normal production path. The tag version must match `pyproject.toml`, `topos/__init__.py`, and `extensions/vscode/package.json` (CI enforces these are equal).

### Manual Releases

Triggered through GitHub Actions `workflow_dispatch`.

Input:

```text
version: v0.3.0
```

Use this only when intentionally publishing a release outside the tag-push path.

> [!WARNING]
> Manual releases still publish to Marketplace when `VSCE_PAT` is configured and the `vscode-marketplace` environment is approved.

## Publishing A New Version

> [!IMPORTANT]
> The VS Code extension version is locked to the Topos version. `pyproject.toml`, `topos/__init__.py`, and `extensions/vscode/package.json` must all carry the same version, and CI fails the build if they diverge. A single release tag `vX.Y.Z` drives both the Topos binary and the VSIX.

Bump all three to the same version. From repo root:

```bash
# 1. Topos (pyproject.toml + topos/__init__.py) -> set to X.Y.Z
# 2. VS Code extension package.json + lockfile
cd extensions/vscode
npm version X.Y.Z --no-git-tag-version
cd ../..
```

Then commit and open a PR to `main`:

```bash
git add pyproject.toml topos/__init__.py extensions/vscode/package.json extensions/vscode/package-lock.json
git commit -m "Bump version to X.Y.Z"
git push origin <branch>
```

Wait for CI:

- version-consistency check passes (pyproject == __init__ == extension package.json)
- binary builds pass
- target VSIX packaging passes
- size checks pass

After merge:

```bash
git checkout main
git pull
git tag v0.3.0
git push origin v0.3.0
```

Use the version that matches `pyproject.toml` (and therefore `extensions/vscode/package.json`).

Do not publish the same extension version twice. Marketplace will reject duplicate versions.

<details>
<summary>Versioning checklist</summary>

- Bump `pyproject.toml`, `topos/__init__.py`, and `extensions/vscode/package.json` to the same version.
- Commit the matching `package-lock.json` update.
- Use a release tag that matches that version, for example `v0.3.0`.
- Never reuse a Marketplace version.

</details>

## Manual Marketplace Publish

Use this only for recovery or first-time testing.

Login:

```bash
cd <your-local-topos-repo>/extensions/vscode
npx --yes @vscode/vsce login KrvLabs
```

Publish existing VSIXs:

```bash
npx --yes @vscode/vsce publish --packagePath topos-vscode-darwin-arm64.vsix
npx --yes @vscode/vsce publish --packagePath topos-vscode-darwin-x64.vsix
npx --yes @vscode/vsce publish --packagePath topos-vscode-linux-arm64.vsix
npx --yes @vscode/vsce publish --packagePath topos-vscode-linux-x64.vsix
```

Preferred path remains GitHub Actions.

> [!CAUTION]
> Manual publishing can bypass the GitHub release checklist. Prefer Actions unless recovering from a failed publish.

## File Inclusion Rules

Packaging is controlled by:

```text
extensions/vscode/.vscodeignore
```

Important rules:

- `src/**` is excluded; production code ships from `dist/extension.js`.
- `scripts/**` is excluded; release helper scripts are not shipped.
- `node_modules/**` is excluded; the extension bundle is produced by esbuild.
- `bin/topos` is intentionally not excluded; the target runtime must be included in each target VSIX.
- `*.vsix` is excluded to avoid packaging prior packages inside new packages.

The staging script copies exactly one runtime binary into:

```text
extensions/vscode/bin/topos
```

Then it is removed after packaging.

<details>
<summary>Why `bin/topos` is not ignored</summary>

The staging script writes the platform runtime to `extensions/vscode/bin/topos` immediately before `vsce package`.

If `.vscodeignore` excludes `bin/**`, the Marketplace package installs successfully but the MCP server cannot use the bundled runtime.

</details>

## Size Gate

The release workflow checks each VSIX using:

```bash
node scripts/check-vsix-size.js <vsix>
```

Default limit:

```text
200 MiB
```

Override:

```bash
TOPOS_VSIX_SIZE_LIMIT_BYTES=<bytes> node scripts/check-vsix-size.js <vsix>
```

If platform binaries grow too large, the VSIX size gate should fail before Marketplace publish.

> [!NOTE]
> The size gate is a release safety check, not an optimization target. If a binary crosses the limit, inspect the PyInstaller build before raising the threshold.

## Troubleshooting

<details open>
<summary>Marketplace publish fails with 401 or 403</summary>

Check:

- `VSCE_PAT` exists in GitHub Actions secrets.
- PAT is an Azure DevOps PAT, not a GitHub PAT.
- PAT scope is `Marketplace: Manage`.
- PAT organization is `All accessible organizations`.
- PAT owner can publish to the `KrvLabs` publisher.
- `package.json` publisher matches the Marketplace publisher ID exactly.

</details>

<details>
<summary>Marketplace publish fails with duplicate version</summary>

Bump all three version sources to the same new value (`pyproject.toml`, `topos/__init__.py`, and the extension):

```bash
cd extensions/vscode
npm version patch --no-git-tag-version
```

Commit and release again with a matching new tag. CI will reject the PR if the three versions diverge.

</details>

<details>
<summary>macOS extension installs but Topos will not run</summary>

Check the workflow logs for:

```text
Unsigned macOS binary
Unnotarized macOS binary
```

If present, Apple signing/notarization secrets were missing. Add them and republish a new version.

</details>

<details>
<summary>MCP server does not appear</summary>

In VS Code:

```text
Developer: Reload Window
MCP: List Servers
Output: Topos
```

The output channel should show the runtime resolution trace.

</details>

<details>
<summary>Bundled runtime was not used</summary>

Check the target VSIX contents:

```bash
unzip -l topos-vscode-darwin-arm64.vsix | rg "extension/bin/topos|extension/dist/extension.js|extension/package.json"
```

Expected:

```text
extension/bin/topos
extension/dist/extension.js
extension/package.json
```

If `extension/bin/topos` is missing, the staging step did not run or `.vscodeignore` excluded the binary.

</details>

## Release Checklist

Before release:

- `extensions/vscode/package.json` version bumped.
- Publisher ID is correct.
- PR checks passed.
- `VSCE_PAT` exists.
- `vscode-marketplace` environment approval is configured if desired.
- Apple secrets exist if shipping production macOS binaries.

Release:

```bash
git checkout main
git pull
git tag vX.Y.Z
git push origin vX.Y.Z
```

After release:

- Confirm GitHub Release has binaries, VSIXs, and checksums.
- Confirm Marketplace page shows the new version.
- Install from Marketplace on at least one supported platform.
- Run `MCP: List Servers`.
- Check `Output: Topos`.
