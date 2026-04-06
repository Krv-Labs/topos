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

# Detect platform
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Linux*)  os="linux" ;;
        Darwin*) os="darwin" ;;
        *)       error "Unsupported OS: $(uname -s)"; exit 1 ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64)  arch="x86_64" ;;
        aarch64|arm64) arch="aarch64" ;;
        *)             error "Unsupported architecture: $(uname -m)"; exit 1 ;;
    esac

    echo "${os}-${arch}"
}

# Get the latest release version
get_latest_version() {
    curl -sSL "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name":' \
        | sed -E 's/.*"([^"]+)".*/\1/'
}

# Download and install
install_topos() {
    local platform version download_url archive_name

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

    archive_name="topos-${platform}.tar.gz"
    download_url="https://github.com/${REPO}/releases/download/${version}/${archive_name}"

    # Create temp directory
    local tmp_dir
    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT

    info "Downloading ${download_url}..."
    if ! curl -sSL -o "${tmp_dir}/${archive_name}" "$download_url"; then
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

    info "Extracting archive..."
    tar -xzf "${tmp_dir}/${archive_name}" -C "${tmp_dir}"

    # Create install directory if needed
    mkdir -p "$INSTALL_DIR"

    # Install the binary
    info "Installing to ${INSTALL_DIR}/topos..."
    mv "${tmp_dir}/topos" "${INSTALL_DIR}/topos"
    chmod +x "${INSTALL_DIR}/topos"

    success "Topos ${version} installed successfully!"
}

# Add to PATH if needed
setup_path() {
    if [ "${TOPOS_NO_MODIFY_PATH:-0}" = "1" ]; then
        return
    fi

    # Check if already in PATH
    if [[ ":$PATH:" == *":$INSTALL_DIR:"* ]]; then
        return
    fi

    local shell_rc=""
    case "${SHELL:-}" in
        */bash) shell_rc="$HOME/.bashrc" ;;
        */zsh)  shell_rc="$HOME/.zshrc" ;;
        */fish) shell_rc="$HOME/.config/fish/config.fish" ;;
    esac

    if [ -n "$shell_rc" ] && [ -f "$shell_rc" ]; then
        echo "" >> "$shell_rc"
        echo "# Added by Topos installer" >> "$shell_rc"

        if [[ "$SHELL" == */fish ]]; then
            echo "set -gx PATH \"$INSTALL_DIR\" \$PATH" >> "$shell_rc"
        else
            echo "export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$shell_rc"
        fi

        warn "Added ${INSTALL_DIR} to PATH in ${shell_rc}"
        warn "Run 'source ${shell_rc}' or start a new shell to use topos"
    else
        warn "${INSTALL_DIR} is not in your PATH"
        warn "Add this to your shell profile:"
        echo ""
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    fi
}

# Verify installation
verify_install() {
    echo ""
    if command -v topos &> /dev/null; then
        success "Verification: topos is available in PATH"
        topos --version 2>/dev/null || true
    elif [ -x "${INSTALL_DIR}/topos" ]; then
        success "Verification: topos installed at ${INSTALL_DIR}/topos"
        "${INSTALL_DIR}/topos" --version 2>/dev/null || true
    else
        error "Verification failed: topos not found"
        exit 1
    fi

    echo ""
    echo "Get started:"
    echo ""
    echo "  topos evaluate src/         # Evaluate a directory"
    echo "  topos inspect module.py     # Detailed metrics"
    echo "  topos compare a.py b.py     # Structural diff"
    echo ""
    echo "Documentation: https://krv-labs.github.io/topos/"
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

    install_topos
    setup_path
    verify_install
}

main "$@"
