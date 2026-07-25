#!/usr/bin/env bash
# Pin and cache LadybugDB prebuilt liblbug for deterministic CI/local builds.
#
# Uses lbug's LBUG_LIBRARY_DIR / LBUG_INCLUDE_DIR path so build.rs skips both
# the in-crate downloader and the cmake source fallback (issue #239).
set -euo pipefail

if [[ "$(uname -s)" == "Darwin" ]]; then
  if [[ -n "${LBUG_BUILD_FROM_SOURCE:-}" || -n "${LBUG_RUST_BUILD_FROM_SOURCE:-}" ]]; then
    echo "Refusing lbug source build on macOS (issue #239)" >&2
    exit 1
  fi
fi

LBUG_VERSION="${LBUG_VERSION:-0.17.1}"
LIB_KIND="${LBUG_LIB_KIND:-static}"
CACHE_ROOT="${LBUG_PREBUILT_ROOT:-${CARGO_HOME:-$HOME/.cargo}/lbug-prebuilt}"
LIB_DIR="${CACHE_ROOT}/${LBUG_VERSION}/${LIB_KIND}/lib"

lib_name() {
  case "$LIB_KIND" in
    static) echo "liblbug.a" ;;
    shared)
      case "$(uname -s)" in
        Darwin) echo "liblbug.dylib" ;;
        Linux) echo "liblbug.so" ;;
        *) echo "liblbug.so" ;;
      esac
      ;;
    *) echo "unsupported LBUG_LIB_KIND: $LIB_KIND" >&2; exit 1 ;;
  esac
}

LIB_FILE="$(lib_name)"

if [[ ! -f "${LIB_DIR}/${LIB_FILE}" ]]; then
  mkdir -p "${LIB_DIR}"
  if ! cargo fetch --locked >/dev/null 2>&1; then
    cargo fetch
  fi
  LBUG_CRATE="$(find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -maxdepth 2 -type d -name "lbug-${LBUG_VERSION}" 2>/dev/null | head -1)"
  if [[ -z "${LBUG_CRATE}" ]]; then
    echo "lbug-${LBUG_VERSION} crate not found under \$CARGO_HOME/registry/src after cargo fetch" >&2
    exit 1
  fi
  LBUG_VERSION="${LBUG_VERSION}" LBUG_LIB_KIND="${LIB_KIND}" \
    LBUG_TARGET_DIR="${LIB_DIR}" \
    sh "${LBUG_CRATE}/scripts/download_lbug.sh"
fi

if [[ ! -f "${LIB_DIR}/${LIB_FILE}" ]]; then
  echo "Expected prebuilt ${LIB_FILE} missing under ${LIB_DIR}" >&2
  exit 1
fi

emit_env() {
  local key="$1"
  local value="$2"
  if [[ -n "${GITHUB_ENV:-}" ]]; then
    {
      echo "${key}=${value}"
    } >>"${GITHUB_ENV}"
  else
    printf 'export %s=%q\n' "${key}" "${value}"
  fi
}

emit_env "LBUG_LIBRARY_DIR" "${LIB_DIR}"
emit_env "LBUG_INCLUDE_DIR" "${LIB_DIR}"
emit_env "LBUG_VERSION" "${LBUG_VERSION}"
echo "lbug prebuilt: ${LIB_DIR}/${LIB_FILE}" >&2
