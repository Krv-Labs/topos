# Publishing Topos to the VS Code MCP Gallery

High-level goal: make Topos appear in VS Code Extensions search as `@mcp topos`.

This is separate from publishing the VS Code extension. The VS Code extension is
a normal Marketplace extension that contributes an MCP provider after install.
The `@mcp` search path is for standalone MCP servers listed through VS Code's
MCP server gallery, which is populated from the official MCP Registry.

## Current State

- `KrvLabs.topos-vscode` is the VS Code extension.
- The extension can register an MCP server after it is installed.
- That does not automatically list Topos in `@mcp` search.
- The public PyPI package name `topos` is already occupied by an unrelated
  project, so `uvx topos` is not a valid registry install path for this project.
- The local Topos CLI already has the right server command: `topos mcp`.
- Comparable gallery entries such as Microsoft's MarkItDown use a dedicated
  MCP package name (`markitdown-mcp`) plus a direct MCP command.

## Recommended Path

Publish a standalone PyPI package named `topos-mcp`, and publish its registry metadata
to the official MCP Registry. The official MCP Registry (hosted by the Model Context Protocol organization) is what directly powers the `@mcp` search in VS Code.

Target user install behavior:

```json
{
  "servers": {
    "topos": {
      "type": "stdio",
      "command": "uvx",
      "args": ["topos-mcp"]
    }
  }
}
```

## Required Package Changes

1. Publish a PyPI package named `topos-mcp`.
2. Add a direct console script:

   ```toml
   [project.scripts]
   topos-mcp = "topos.mcp.server:main"
   ```

3. Keep the existing `topos = "topos.cli.main:cli"` script if the full CLI should
   remain available after install.
4. Add this README marker before publishing to PyPI:

   ```html
   <!-- mcp-name: io.github.Krv-Labs/topos -->
   ```

The MCP Registry uses that marker to verify that the PyPI package belongs to the
registry server entry.

## Registry Metadata

Create `.mcp/server.json`. Keep the square product icon in
`extensions/vscode/icon.png`; VS Code packages need the package-local file, and
the MCP Registry can reference the same committed PNG over HTTPS without copying
it elsewhere.

```json
{
  "$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
  "name": "io.github.Krv-Labs/topos",
  "title": "Topos",
  "description": "Structural code-quality tools for AI coding agents.",
  "icons": [
    {
      "src": "https://raw.githubusercontent.com/Krv-Labs/topos/main/extensions/vscode/icon.png",
      "mimeType": "image/png",
      "sizes": ["958x958"]
    }
  ],
  "version": "0.3.6",
  "websiteUrl": "https://github.com/Krv-Labs/topos",
  "repository": {
    "url": "https://github.com/Krv-Labs/topos",
    "source": "github"
  },
  "packages": [
    {
      "registryType": "pypi",
      "registryBaseUrl": "https://pypi.org",
      "identifier": "topos-mcp",
      "version": "0.3.6",
      "transport": {
        "type": "stdio"
      }
    }
  ]
}
```

## Publishing (automated)

Registry publishing runs automatically in `.github/workflows/release.yml` via the
`publish-mcp-registry` job (see [#66](https://github.com/Krv-Labs/topos/issues/66)).
On a release (`v*` tag push or manual dispatch) it:

1. Confirms `.mcp/server.json` version matches the release tag.
2. Waits for `topos-mcp==<version>` to be live on PyPI (`needs: publish-pypi`).
3. Installs `mcp-publisher`, authenticates with **GitHub OIDC** (`mcp-publisher login
   github-oidc`, no dedicated secret), then `validate`s and `publish`es `.mcp/server.json`.

The job is skipped on pull requests and does not touch the VS Code Marketplace or
GitHub release assets. To ship a registry update, bump `.mcp/server.json` and tag a
release — nothing manual is required.

> **OIDC fallback:** if OIDC can't satisfy the `io.github.Krv-Labs` org namespace,
> add an `MCP_GITHUB_TOKEN` repo secret (registry-documented `read:org` + `read:user`
> scopes) and switch the auth step to `./mcp-publisher login github --token
> "$MCP_GITHUB_TOKEN"`.

## Publish Steps (manual fallback)

1. Verify locally:

   ```bash
   uv run topos --version
   uv run topos mcp --help
   ```

2. **Trigger the Automated PyPI Release:** Building and publishing the `topos-mcp` package (including compiling its PyO3 Rust binary wheels) is fully automated via GitHub Actions (`.github/workflows/release.yml`). To trigger a PyPI release:
   - **Tag Push:** Push a version tag matching your `Cargo.toml` and `.mcp/server.json` version:
     ```bash
     git tag v0.3.6
     git push origin v0.3.6
     ```
   - **Manual Trigger:** Manually dispatch the **Build and Release** workflow through the GitHub repository Actions panel with the target version input.
3. **Verify PyPI is Live:** Confirm that the package is successfully published to PyPI as `topos-mcp` and that the live package description (README) includes the required verification marker:
   ```html
   <!-- mcp-name: io.github.Krv-Labs/topos -->
   ```
4. Install the `mcp-publisher` CLI tool. On macOS and Linux, the recommended method is Homebrew:

   ```bash
   brew install mcp-publisher
   ```

   *Alternatively, for manual binary installation, you can download the latest release:*
   ```bash
   curl -L "https://github.com/modelcontextprotocol/registry/releases/latest/download/mcp-publisher_$(uname -s | tr '[:upper:]' '[:lower:]')_$(uname -m | sed 's/x86_64/amd64/;s/aarch64/arm64/').tar.gz" | tar xz mcp-publisher
   ```

5. Log in with GitHub to claim your namespace:

   ```bash
   mcp-publisher login github
   ```

   *(Prefix with `./` if using the manual fallback binary in the current directory).*

6. Run the preflight validation check locally to ensure schema and registry compliance:

   ```bash
   mcp-publisher validate .mcp/server.json
   ```

7. Publish the server metadata:

   ```bash
   mcp-publisher publish .mcp/server.json
   ```

8. Verify through the official registry API:

   ```bash
   curl "https://registry.modelcontextprotocol.io/v0.1/servers?search=io.github.Krv-Labs/topos"
   ```

## Glama Release

Glama is a separate directory surface from the official MCP Registry and the VS
Code Marketplace. Keep `glama.json` limited to the documented maintainer list;
do not add undocumented `logo` or `icon` fields to that file. After the GitHub
and PyPI metadata are current, use the claimed server's Glama admin UI to sync
the repository, configure the Dockerfile build, and publish the Glama release.
If Glama exposes a profile image override, point it at the same committed product
icon used by `.mcp/server.json`.

## VS Code Verification

1. Open VS Code.
2. Enable the MCP gallery:

   ```json
   "chat.mcp.gallery.enabled": true
   ```

3. Open Extensions.
4. Search:

   ```text
   @mcp topos
   ```

5. Install the Topos MCP server result.
6. Run `MCP: List Servers`.
7. Start `Topos`.
8. Trust the server when prompted.
9. In Agent mode, test:

   ```text
   Use Topos to evaluate this workspace.
   ```

## Gotchas

- Registry metadata is immutable per version. Fixes require publishing a new
  version.
- The official MCP Registry is still marked preview, so indexing behavior can
  change.
- VS Code's `@mcp` gallery is directly populated by the official MCP Registry. No manual inclusion request is required.
- `@mcp` search requires `chat.mcp.gallery.enabled`.
- Topos file access depends on the server starting from a workspace root or
  having `TOPOS_MCP_FILE_ROOT` set.
- Raw GitHub release binaries are not enough for the current registry path unless
  they are packaged as MCPB artifacts.
- Glama may show the GitHub namespace avatar until its own server profile or
  release metadata is updated; the MCP Registry `icons` field and Glama profile
  image are separate surfaces.
