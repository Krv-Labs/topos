#!/bin/sh
set -e

REPO="Krv-Labs/topos"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="topos"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() {
    printf "${GREEN}>>>${NC} %s\n" "$1"
}

warn() {
    printf "${YELLOW}>>>${NC} %s\n" "$1"
}

error() {
    printf "${RED}>>>${NC} %s\n" "$1" >&2
    exit 1
}

detect_platform() {
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    ARCH=$(uname -m)

    case "$OS" in
        linux)
            OS="linux"
            ;;
        darwin)
            OS="macos"
            ;;
        *)
            error "Unsupported operating system: $OS"
            ;;
    esac

    case "$ARCH" in
        x86_64|amd64)
            ARCH="amd64"
            ;;
        arm64|aarch64)
            ARCH="arm64"
            ;;
        *)
            error "Unsupported architecture: $ARCH"
            ;;
    esac

    PLATFORM="${OS}-${ARCH}"
    info "Detected platform: $PLATFORM"
}

get_latest_version() {
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$VERSION" ]; then
        error "Failed to fetch latest version"
    fi
    info "Latest version: $VERSION"
}

download_binary() {
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${BINARY_NAME}-${PLATFORM}"
    
    info "Downloading from: $DOWNLOAD_URL"
    
    TMPDIR=$(mktemp -d)
    TMPFILE="${TMPDIR}/${BINARY_NAME}"
    
    if ! curl -fsSL "$DOWNLOAD_URL" -o "$TMPFILE"; then
        rm -rf "$TMPDIR"
        error "Failed to download binary"
    fi
    
    chmod +x "$TMPFILE"
}

install_binary() {
    info "Installing to $INSTALL_DIR/$BINARY_NAME"
    
    if [ -w "$INSTALL_DIR" ]; then
        mv "$TMPFILE" "$INSTALL_DIR/$BINARY_NAME"
    else
        warn "Elevated permissions required to install to $INSTALL_DIR"
        sudo mv "$TMPFILE" "$INSTALL_DIR/$BINARY_NAME"
    fi
    
    rm -rf "$TMPDIR"
}

verify_installation() {
    if command -v "$BINARY_NAME" >/dev/null 2>&1; then
        info "Successfully installed $BINARY_NAME!"
        info "Run '$BINARY_NAME --help' to get started"
    else
        warn "Binary installed but not in PATH. Add $INSTALL_DIR to your PATH"
    fi
}

main() {
    info "Installing topos..."
    detect_platform
    get_latest_version
    download_binary
    install_binary
    verify_installation
}

main
