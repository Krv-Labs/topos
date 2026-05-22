# Topos VS Code MCP Extension Publishing Guide

This guide covers local testing, VSIX packaging, Marketplace setup, GitHub secrets, release publishing, and the runtime design for the Topos VS Code MCP extension.

Extension root:

```bash
cd <your-local-topos-repo>/extensions/vscode
```

## Current Publishing Model

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
- `test`: verifies binary staging behavior.
- `package`: production esbuild bundle into `dist/extension.js`.

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
Output: Topos Code Quality
```

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
Output: Topos Code Quality
```

Expected log signal:

```text
Using bundled Topos runtime: .../extension/bin/topos
Resolved MCP server command: .../extension/bin/topos mcp
```

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

### Tag Releases

Triggered by tags:

```text
v*
```

Example:

```bash
git tag v0.1.1
git push origin v0.1.1
```

What runs:

1. Build platform binaries.
2. Sign/notarize macOS binaries if Apple secrets exist.
3. Package platform VSIXs.
4. Size-check VSIXs.
5. Create GitHub Release with binaries, VSIXs, and checksums.
6. Publish all platform VSIXs to VS Code Marketplace using `VSCE_PAT`.

### Manual Releases

Triggered through GitHub Actions `workflow_dispatch`.

Input:

```text
version: v0.1.1
```

Use this only when intentionally publishing a release outside the tag-push path.

## Publishing A New Version

From a clean branch:

```bash
cd <your-local-topos-repo>/extensions/vscode
npm version patch --no-git-tag-version
```

Then from repo root:

```bash
cd <your-local-topos-repo>
git add extensions/vscode/package.json extensions/vscode/package-lock.json
git commit -m "Bump VS Code extension version"
git push origin <branch>
```

Open a PR to `main`.

Wait for CI:

- binary builds pass
- target VSIX packaging passes
- size checks pass

After merge:

```bash
git checkout main
git pull
git tag v0.1.1
git push origin v0.1.1
```

Use the version that matches `extensions/vscode/package.json`.

Do not publish the same extension version twice. Marketplace will reject duplicate versions.

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

## Troubleshooting

### Marketplace publish fails with 401 or 403

Check:

- `VSCE_PAT` exists in GitHub Actions secrets.
- PAT is an Azure DevOps PAT, not a GitHub PAT.
- PAT scope is `Marketplace: Manage`.
- PAT organization is `All accessible organizations`.
- PAT owner can publish to the `KrvLabs` publisher.
- `package.json` publisher matches the Marketplace publisher ID exactly.

### Marketplace publish fails with duplicate version

Bump:

```bash
cd extensions/vscode
npm version patch --no-git-tag-version
```

Commit and release again with a matching new tag.

### macOS extension installs but Topos will not run

Check the workflow logs for:

```text
Unsigned macOS binary
Unnotarized macOS binary
```

If present, Apple signing/notarization secrets were missing. Add them and republish a new version.

### MCP server does not appear

In VS Code:

```text
Developer: Reload Window
MCP: List Servers
Output: Topos Code Quality
```

The output channel should show the runtime resolution trace.

### Bundled runtime was not used

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
- Check `Output: Topos Code Quality`.
