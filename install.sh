#!/usr/bin/env bash
#
# Topos Installation Script
# =========================
# Install Topos - category-theoretic code quality evaluation for Python
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/Krv-Labs/topos/main/install.sh | bash
#
# Options (via environment variables):
#   TOPOS_VERSION   - Specific version to install (default: latest)
#   TOPOS_INSTALL   - Installation directory (default: ~/.local/bin)
#   TOPOS_NO_MODIFY_PATH - Set to 1 to skip PATH modification

set -euo pipefail

# Configuration
REPO="Krv-Labs/topos"
INSTALL_DIR="${TOPOS_INSTALL:-$HOME/.local/bin}"
VERSION="${TOPOS_VERSION:-latest}"
TMP_DIR=""
PROVENANCE_FILE="${TOPOS_PROVENANCE_FILE:-${XDG_STATE_HOME:-$HOME/.local/state}/topos/install-provenance}"
PATH_HINT_BEGIN="# BEGIN TOPOS INSTALLER PATH"
PATH_HINT_END="# END TOPOS INSTALLER PATH"
PATH_HINT_FILE=""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

info() {
    echo -e "${BLUE}[info]${NC} $1"
}

success() {
    echo -e "${GREEN}[ok]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[warn]${NC} $1"
}

error() {
    echo -e "${RED}[error]${NC} $1" >&2
}

cleanup() {
    if [ -n "${TMP_DIR:-}" ] && [ -d "${TMP_DIR}" ]; then
        rm -rf -- "${TMP_DIR}"
    fi
}

trap cleanup EXIT INT TERM

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        error "Missing required dependency: $1"
        exit 1
    fi
}

check_dependencies() {
    require_command curl
    require_command mktemp
    require_command sed
    require_command grep
    require_command uname
    require_command mkdir
    require_command mv
    require_command chmod
}

validate_install_dir() {
    if [ -z "${INSTALL_DIR}" ]; then
        error "TOPOS_INSTALL resolved to an empty path"
        exit 1
    fi

    case "${INSTALL_DIR}" in
        *$'\n'*|*$'\r'*)
            error "INSTALL_DIR must not contain newlines"
            exit 1
            ;;
    esac
}

escape_for_double_quotes() {
    local value="$1"
    value="${value//\\/\\\\}"
    value="${value//\"/\\\"}"
    value="${value//\$/\\\$}"
    value="${value//\`/\\\`}"
    printf '%s' "${value}"
}

calculate_sha256() {
    local file="$1"

    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "${file}" | awk '{print $1}'
        return
    fi

    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "${file}" | awk '{print $1}'
        return
    fi

    error "Missing required dependency: sha256sum or shasum"
    exit 1
}

# Detect platform
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Linux*)  os="linux" ;;
        Darwin*) os="macos" ;;
        *)       error "Unsupported OS: $(uname -s)"; exit 1 ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64)  arch="amd64" ;;
        aarch64|arm64) arch="arm64" ;;
        *)             error "Unsupported architecture: $(uname -m)"; exit 1 ;;
    esac

    echo "${os}-${arch}"
}

# Get the latest release version
get_latest_version() {
    local latest_url version

    latest_url=$(curl --fail -sSL -o /dev/null -w '%{url_effective}' \
        "https://github.com/${REPO}/releases/latest") || return 1

    if [[ "${latest_url}" != *"/releases/tag/"* ]]; then
        return 1
    fi

    version=$(printf '%s\n' "${latest_url##*/}" | sed 's/[?#].*$//')
    # Accept versions like v1.2.3, 1.2.3, and prerelease/build suffixes.
    if [[ -z "${version}" || ! "${version}" =~ ^v?[0-9]+([.][0-9]+)*([-.][0-9A-Za-z]+)*$ ]]; then
        return 1
    fi

    printf '%s\n' "${version}"
}

# Download and install
install_topos() {
    local platform version download_url checksums_url asset_name tmp_binary checksums_file
    local expected_checksum actual_checksum expected_checksum_normalized actual_checksum_normalized

    platform=$(detect_platform)
    info "Detected platform: ${platform}"

    if [ "$VERSION" = "latest" ]; then
        version=$(get_latest_version)
        if [ -z "$version" ]; then
            error "Could not determine latest version"
            exit 1
        fi
        info "Latest version: ${version}"
    else
        version="$VERSION"
        info "Installing version: ${version}"
    fi

    asset_name="topos-${platform}"
    download_url="https://github.com/${REPO}/releases/download/${version}/${asset_name}"

    # Create temp directory
    TMP_DIR=$(mktemp -d)
    tmp_binary="${TMP_DIR}/topos"
    checksums_file="${TMP_DIR}/checksums.txt"

    info "Downloading ${download_url}..."
    if ! curl --fail -sSL -o "${tmp_binary}" "$download_url"; then
        error "Download failed. The release asset may not exist for this platform."
        echo ""
        echo "Alternative installation methods:"
        echo ""
        echo "  # Using uv (recommended for Python users)"
        echo "  uv pip install topos"
        echo ""
        echo "  # Using pip"
        echo "  pip install topos"
        echo ""
        echo "  # From source"
        echo "  git clone https://github.com/${REPO}.git"
        echo "  cd topos && uv sync"
        exit 1
    fi

    checksums_url="https://github.com/${REPO}/releases/download/${version}/checksums.txt"
    info "Verifying checksum..."
    if ! curl --fail -sSL -o "${checksums_file}" "${checksums_url}"; then
        error "Failed to download checksums.txt for ${version}"
        exit 1
    fi

    expected_checksum=$(
        grep -E "[[:space:]]${asset_name}$" "${checksums_file}" \
            | sed -E 's/^([A-Fa-f0-9]{64}).*/\1/' \
            | head -n 1
    )

    if [ -z "${expected_checksum}" ]; then
        error "Could not find checksum entry for ${asset_name}"
        exit 1
    fi

    actual_checksum=$(calculate_sha256 "${tmp_binary}")
    actual_checksum_normalized=$(printf '%s' "${actual_checksum}" | tr '[:upper:]' '[:lower:]')
    expected_checksum_normalized=$(printf '%s' "${expected_checksum}" | tr '[:upper:]' '[:lower:]')
    if [ "${actual_checksum_normalized}" != "${expected_checksum_normalized}" ]; then
        error "Checksum verification failed for ${asset_name}"
        exit 1
    fi

    # Create install directory if needed
    mkdir -p "$INSTALL_DIR"

    # Install the binary
    info "Installing to ${INSTALL_DIR}/topos..."
    mv "${tmp_binary}" "${INSTALL_DIR}/topos"
    chmod +x "${INSTALL_DIR}/topos"

    success "Topos ${version} installed successfully!"
}

# Add to PATH if needed
setup_path() {
    local shell_rc="" shell_name path_line escaped_install_dir escaped_install_dir_regex

    if [ "${TOPOS_NO_MODIFY_PATH:-0}" = "1" ]; then
        return
    fi

    # Check if already in PATH
    if [[ ":${PATH:-}:" == *":$INSTALL_DIR:"* ]]; then
        return
    fi

    shell_name="${SHELL:-}"
    case "${shell_name}" in
        */bash) shell_rc="$HOME/.bashrc" ;;
        */zsh)  shell_rc="$HOME/.zshrc" ;;
        */fish) shell_rc="$HOME/.config/fish/config.fish" ;;
    esac

    escaped_install_dir=$(escape_for_double_quotes "${INSTALL_DIR}")
    escaped_install_dir_regex="${INSTALL_DIR//\//\\/}"
    if [[ "${shell_name}" == */fish ]]; then
        path_line="set -gx PATH \"${escaped_install_dir}\" \$PATH"
    else
        path_line="export PATH=\"${escaped_install_dir}:\$PATH\""
    fi

    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
        if grep -Eq "PATH=.*${escaped_install_dir_regex}" "${shell_rc}"; then
            return
        fi

        echo "" >> "$shell_rc"
        echo "${PATH_HINT_BEGIN}" >> "$shell_rc"
        echo "# Added by Topos installer" >> "$shell_rc"
        echo "${path_line}" >> "$shell_rc"
        echo "${PATH_HINT_END}" >> "$shell_rc"
        PATH_HINT_FILE="${shell_rc}"

        warn "Added ${INSTALL_DIR} to PATH in ${shell_rc}"
        warn "Run 'source ${shell_rc}' or start a new shell to use topos"
    else
        warn "${INSTALL_DIR} is not in your PATH"
        warn "Add this to your shell profile:"
        echo ""
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    fi
}

write_provenance() {
    local provenance_dir
    provenance_dir=$(dirname "${PROVENANCE_FILE}")
    mkdir -p "${provenance_dir}"
    cat > "${PROVENANCE_FILE}" <<EOF
install_method=binary-installer
install_path=${INSTALL_DIR}/topos
install_version=${VERSION}
path_hint_file=${PATH_HINT_FILE}
path_hint_begin=${PATH_HINT_BEGIN}
path_hint_end=${PATH_HINT_END}
EOF
}

# Verify installation
verify_install() {
    local path_topos resolved_topos
    path_topos="${INSTALL_DIR}/topos"

    echo ""
    if [ ! -x "${path_topos}" ]; then
        error "Verification failed: topos not found"
        exit 1
    fi

    success "Verification: topos installed at ${path_topos}"
    "${path_topos}" --version 2>/dev/null || true

    resolved_topos="$(command -v topos 2>/dev/null || true)"
    if [ -n "${resolved_topos}" ] && [ "${resolved_topos}" != "${path_topos}" ]; then
        warn "Another topos is earlier on PATH: ${resolved_topos}"
        warn "Use ${path_topos} directly or update your PATH order."
    elif [ -n "${resolved_topos}" ]; then
        success "Verification: topos is available in PATH"
    fi

    echo ""
    echo "Get started:"
    echo ""
    echo "  topos evaluate src/         # Evaluate a directory"
    echo "  topos inspect module.py     # Detailed metrics"
    echo "  topos compare a.py b.py     # Structural diff"
    echo ""
    echo "Documentation: https://docs.krv.ai/topos"
}

# Main
main() {
    echo ""
    echo "  ╔════════════════════════════════════════════════════════════╗"
    echo "  ║                                                            ║"
    echo "  ║   Topos - Category-theoretic code quality evaluation       ║"
    echo "  ║                                                            ║"
    echo "  ║   Treating programs as morphisms in a world of             ║"
    echo "  ║   commodity code.                                          ║"
    echo "  ║                                                            ║"
    echo "  ╚════════════════════════════════════════════════════════════╝"
    echo ""

    check_dependencies
    validate_install_dir
    install_topos
    setup_path
    write_provenance
    verify_install
}

main "$@"
