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

# Derive hidden imports from the lazy-export table. Capture into a variable
# first: a command substitution failure aborts under `set -e`, and the
# emptiness guard catches a helper that exits 0 but prints nothing (a renamed
# `_LAZY_EXPORTS`, an import error). Process substitution would hide both,
# silently shipping a binary with no hidden imports.
hidden_imports="$(uv run python scripts/lazy_exports.py)"
if [[ -z "${hidden_imports}" ]]; then
  echo "error: scripts/lazy_exports.py produced no hidden imports" >&2
  exit 1
fi
while IFS= read -r module; do
  PYINSTALLER_ARGS+=(--hidden-import "${module}")
done <<< "${hidden_imports}"

if [[ "$(uname -s)" == "Darwin" && -n "${CODESIGN_IDENTITY:-}" ]]; then
  PYINSTALLER_ARGS+=(
    --codesign-identity "${CODESIGN_IDENTITY}"
    --osx-entitlements-file packaging/macos-entitlements.plist
  )
fi

uv run --with pyinstaller pyinstaller "${PYINSTALLER_ARGS[@]}" topos/cli/main.py
