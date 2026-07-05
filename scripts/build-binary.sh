#!/usr/bin/env bash
# Build the Topos PyInstaller onefile binary.
#
# Usage (from repo root):
#   ./scripts/build-binary.sh
#
# Optional environment:
#   CODESIGN_IDENTITY — macOS Developer ID for in-collection signing (release CI)
#
# Hidden imports are derived from topos._LAZY_EXPORTS via scripts/lazy_exports.py.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

PYINSTALLER_ARGS=(
  --name topos
  --onefile
  --noupx
  --clean
  --collect-all tree_sitter
  --collect-all tree_sitter_python
  --collect-all tree_sitter_rust
  --collect-all tree_sitter_javascript
  --collect-all tree_sitter_cpp
  --collect-all tree_sitter_typescript
  --collect-all fastmcp
  --collect-all ladybug
  --copy-metadata fastmcp
  --copy-metadata topos-mcp
  --add-data Cargo.toml:.
  --add-data topos/mcp/resources/content:topos/mcp/resources/content
)

while IFS= read -r module; do
  PYINSTALLER_ARGS+=(--hidden-import "${module}")
done < <(uv run python scripts/lazy_exports.py)

if [[ "$(uname -s)" == "Darwin" && -n "${CODESIGN_IDENTITY:-}" ]]; then
  PYINSTALLER_ARGS+=(
    --codesign-identity "${CODESIGN_IDENTITY}"
    --osx-entitlements-file packaging/macos-entitlements.plist
  )
fi

uv run --with pyinstaller pyinstaller "${PYINSTALLER_ARGS[@]}" topos/cli/main.py
