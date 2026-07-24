#!/usr/bin/env bash
# Verify workspace crates are ready for `cargo publish` (dependency order).
# topos-mcp and topos stay skipped until sighthound is on crates.io — git-only
# deps cannot ship on crates.io (#149).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> topos-engine"
cargo publish -p topos-engine --dry-run "$@"

if cargo search sighthound --limit 1 2>/dev/null | grep -q '^sighthound = '; then
  echo "==> topos-mcp"
  cargo publish -p topos-mcp --dry-run "$@"
  echo "==> topos"
  cargo publish -p topos --dry-run "$@"
else
  echo "SKIP: topos-mcp and topos dry-run until sighthound is published to crates.io" >&2
fi
