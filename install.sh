#!/usr/bin/env bash
#
# Topos Installation Script
# =========================
# Install Topos - structural code quality metrics for AI coding agents
#
# Usage:
#   curl -fsSL https://docs.krv.ai/topos/install.sh | sh
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

info() {
    printf '> %s\n' "$1"
}

success() {
    if [ -t 1 ]; then
        printf '\033[32m✓\033[0m %s\n' "$1"
    else
        printf '✓ %s\n' "$1"
    fi
}

warn() {
    if [ -t 2 ]; then
        printf '\033[33m!\033[0m %s\n' "$1"
    else
        printf '! %s\n' "$1"
    fi
}

error() {
    if [ -t 2 ]; then
        printf '\033[31mx\033[0m %s\n' "$1" >&2
    else
        printf 'x %s\n' "$1" >&2
    fi
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

print_header() {
    if [ -t 1 ]; then
        printf '\n\n'
        printf '%b\n' '  ⣀⣤⣄⡀        ⢀⣤⣤⣀        ⢀⣠⣤⣀'
        printf '%b\n' ' ⢰⠃  ⢻⣶⣶⣶⣶⣶⣶⣶⣶⡟  ⢸⣶⣶⣶⣶⣶⣶⣶⣶⣾  ⠈'
        printf '%b\n' ' ⠈⠳⣤⡴⠿⣍⠉⠉⠉⠉⠉⠉⣩⠿⣦⣤⠾⣍⡉⠉⠉⠉⠉⠉\033[38;2;182;90;34m⣩⣿\033[0m⣦⣤⠴'
        printf '%b\n' '      ⠈⠳⣤⣀⣀⣤⠞⠁ ⣿⣿ ⠈⠛⢦⣀⣠\033[38;2;182;90;34m⣴⡿⠟⠁\033[0m'
        printf '%b\n' '        ⠸⡁⢈⡇   ⣿⣿\033[38;2;182;90;34m⣤⣤⣶⣾\033[0m⣇⢀⡏           █▄'
        printf '%b\n' '         ⠈⠁⠙⢦\033[38;2;182;90;34m⣴⣾⣿⣿⠉⢉\033[0m⣴⠟⠉⠉           ▄██▄'
        printf '%b\n' '           \033[38;2;182;90;34m⢠⣿⠟\033[0m⣶⠟⠻⢶⡟⠁               ██ ▄███▄ ████▄ ▄███▄ ▄██▀█'
        printf '%b\n' '           \033[38;2;182;90;34m⣿⡏\033[0m⠐⣇  ⢠⠇                ██ ██ ██ ██ ██ ██ ██ ▀███▄'
        printf '%b\n' '          \033[38;2;182;90;34m⠰⣿⡇\033[0m ⠈⣿⣿⠋                ▄██▄▀███▀▄████▀▄▀███▀█▄▄██▀'
        printf '%b\n' '           \033[38;2;182;90;34m⣿⣇\033[0m  ⣿⣿                           ██'
        printf '%b\n' '           \033[38;2;182;90;34m⠘⣿⡄\033[0m ⣿⣿                           ▀'
        printf '%b\n' '            \033[38;2;182;90;34m⠘⣿⣆\033[0m⣿⣿'
        printf '%b\n' '             \033[38;2;182;90;34m⢈\033[0m⡟⠉⠉⠹⡄'
        printf '%b\n' '              ⢧⣀⣀⡴⠃'
        printf '%b\n' '               ⠈⠁'
        printf '\n\n'
        return
    fi

    cat <<'EOF'


  ⣀⣤⣄⡀        ⢀⣤⣤⣀        ⢀⣠⣤⣀
 ⢰⠃  ⢻⣶⣶⣶⣶⣶⣶⣶⣶⡟  ⢸⣶⣶⣶⣶⣶⣶⣶⣶⣾  ⠈
 ⠈⠳⣤⡴⠿⣍⠉⠉⠉⠉⠉⠉⣩⠿⣦⣤⠾⣍⡉⠉⠉⠉⠉⠉⣩⣿⣦⣤⠴
      ⠈⠳⣤⣀⣀⣤⠞⠁ ⣿⣿ ⠈⠛⢦⣀⣠⣴⡿⠟⠁
        ⠸⡁⢈⡇   ⣿⣿⣤⣤⣶⣾⣇⢀⡏           █▄
         ⠈⠁⠙⢦⣴⣾⣿⣿⠉⢉⣴⠟⠉⠉           ▄██▄
           ⢠⣿⠟⣶⠟⠻⢶⡟⠁               ██ ▄███▄ ████▄ ▄███▄ ▄██▀█
           ⣿⡏⠐⣇  ⢠⠇                ██ ██ ██ ██ ██ ██ ██ ▀███▄
          ⠰⣿⡇ ⠈⣿⣿⠋                ▄██▄▀███▀▄████▀▄▀███▀█▄▄██▀
           ⣿⣇  ⣿⣿                           ██
           ⠘⣿⡄ ⣿⣿                           ▀
            ⠘⣿⣆⣿⣿
             ⢈⡟⠉⠉⠹⡄
              ⢧⣀⣀⡴⠃
               ⠈⠁


EOF
}

run_with_spinner() {
    local label="$1" status_file pid status i frame_count
    shift

    if [ ! -t 2 ]; then
        info "${label}"
        "$@"
        status=$?
        if [ "${status}" -eq 0 ]; then
            success "${label}"
        fi
        return "${status}"
    fi

    local frames=( "⠋" "⠙" "⠹" "⠸" "⠼" "⠴" "⠦" "⠧" "⠇" "⠏" )
    frame_count=${#frames[@]}
    status_file=$(mktemp "${TMPDIR:-/tmp}/topos-spinner.XXXXXX")
    rm -f "${status_file}"

    (
        set +e
        "$@"
        printf '%s' "$?" > "${status_file}"
    ) &
    pid=$!

    i=0
    while [ ! -f "${status_file}" ]; do
        printf '\r  %s %s' "${frames[$i]}" "${label}" >&2
        i=$(((i + 1) % frame_count))
        sleep 0.08
    done

    wait "${pid}" 2>/dev/null || true
    status=$(cat "${status_file}")
    rm -f "${status_file}"

    if [ "${status}" -eq 0 ]; then
        printf '\r  \033[32m✓\033[0m %s\n' "${label}" >&2
    else
        printf '\r  \033[31mx\033[0m %s\n' "${label}" >&2
    fi

    return "${status}"
}

download_file() {
    local label="$1" url="$2" output="$3"

    run_with_spinner "${label}" curl --fail -sSL -o "${output}" "${url}"
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
    if [[ -z "${version}" || ! "${version}" =~ ^v?[0-9]+([.][0-9]+)+([-.][0-9A-Za-z]+)*$ ]]; then
        return 1
    fi

    printf '%s\n' "${version}"
}

# Download and install
install_topos() {
    local platform version download_url checksums_url asset_name tmp_binary checksums_file
    local expected_checksum actual_checksum expected_checksum_normalized actual_checksum_normalized

    platform=$(detect_platform)
    info "Platform ${platform}"

    if [ "$VERSION" = "latest" ]; then
        version=$(get_latest_version)
        if [ -z "$version" ]; then
            error "Could not determine latest version"
            exit 1
        fi
        info "Version ${version}"
    else
        version="$VERSION"
        info "Version ${version}"
    fi

    asset_name="topos-${platform}"
    download_url="https://github.com/${REPO}/releases/download/${version}/${asset_name}"

    # Create temp directory
    TMP_DIR=$(mktemp -d)
    tmp_binary="${TMP_DIR}/topos"
    checksums_file="${TMP_DIR}/checksums.txt"

    echo ""
    if ! download_file "Downloading ${asset_name}" "${download_url}" "${tmp_binary}"; then
        error "Download failed. The release asset may not exist for this platform."
        echo ""
        echo "Alternative installation methods:"
        echo ""
        echo "  # MCP server via PyPI (installs the 'topos-mcp' command)"
        echo "  uvx topos-mcp        # run without a persistent install"
        echo "  pip install topos-mcp"
        echo ""
        echo "  # From source (builds the 'topos' CLI + MCP server)"
        echo "  git clone https://github.com/${REPO}.git"
        echo "  cd topos && cargo build --release -p topos"
        exit 1
    fi

    checksums_url="https://github.com/${REPO}/releases/download/${version}/checksums.txt"
    if ! download_file "Fetching checksums" "${checksums_url}" "${checksums_file}"; then
        error "Failed to download checksums.txt for ${version}"
        exit 1
    fi

    echo ""

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
    info "Install ${INSTALL_DIR}/topos"
    mv "${tmp_binary}" "${INSTALL_DIR}/topos"
    chmod +x "${INSTALL_DIR}/topos"

    success "Installed Topos ${version}"
}

# Install optional dependencies
install_optional_dependencies() {
    echo ""
    info "Dependencies"

    if command -v gitnexus >/dev/null 2>&1; then
        success "gitnexus is already installed."
        return
    fi

    echo "gitnexus is required for coupling metrics (COMPOSABLE/IDEAL targets)."

    local reply=""
    if [ -t 0 ]; then
        printf '? Do you want to install gitnexus via pnpm/npm? [Y/n] '
        read -r reply
    elif [ -c /dev/tty ]; then
        printf '? Do you want to install gitnexus via pnpm/npm? [Y/n] '
        read -r reply < /dev/tty
    fi

    case "$reply" in
        [yY][eE][sS]|[yY]|"")
            # Keep the pnpm-first preference in sync with the other install paths:
            #   - TypeScript: extensions/vscode/src/extension.ts (resolveGitNexusInstallCommand)
            #   - Rust: topos/engine/src/adapters/gitnexus.rs (still npm-only; follow-up)
            local install_cmd=""
            if command -v pnpm >/dev/null 2>&1; then
                install_cmd="pnpm add -g gitnexus"
            elif command -v npm >/dev/null 2>&1; then
                install_cmd="npm install -g gitnexus"
            else
                warn "Neither pnpm nor npm found. Skipping gitnexus installation."
                warn "Install manually with: pnpm add -g gitnexus  # or: npm install -g gitnexus"
                warn "Coupling metrics will not be available."
                return
            fi
            info "Installing gitnexus ($install_cmd)..."
            if $install_cmd; then
                success "gitnexus installed successfully!"
            else
                error "Failed to install gitnexus."
                warn "You may need to install it manually: pnpm add -g gitnexus  # or: npm install -g gitnexus"
            fi
            ;;
        *)
            info "Skipping gitnexus installation."
            info "Note: Coupling metrics will not be available without gitnexus."
            ;;
    esac
}

# Add to PATH if needed
setup_path() {
    local shell_rc="" shell_name path_line escaped_install_dir escaped_install_dir_regex

    if [ "${TOPOS_NO_MODIFY_PATH:-0}" = "1" ]; then
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

    # Check if already in PATH
    if [[ ":${PATH:-}:" == *":$INSTALL_DIR:"* ]]; then
        return
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
    if [ -t 1 ]; then
        printf '\033[1mRecommended (agents)\033[0m\n'
        printf '  \033[1mclaude mcp add topos topos mcp\033[0m\n'
    else
        echo "Recommended (agents):"
        echo "  claude mcp add topos topos mcp"
    fi
    echo ""
    echo "Direct CLI (SIMPLE + SECURE only):"
    echo "  topos evaluate <YOUR_REPO_SRC_HERE> -r --preferences simple,secure"
    echo ""
    echo "Composability (COMPOSABLE/GOLD, via the MCP server today):"
    echo "  cd <YOUR_REPO_HERE>"
    echo "  claude mcp add --transport stdio topos -- topos mcp"
    echo "  # then, from an agent: topos_generate_depgraph, then"
    echo "  # topos_evaluate_file(..., gitnexus_dir=\".gitnexus\")"
    echo ""
    echo "Docs: https://docs.krv.ai/topos"
}

# Main
main() {
    if [ "${TOPOS_UPDATE:-0}" = "1" ]; then
        info "Updating Topos..."
    else
        print_header
    fi

    check_dependencies
    validate_install_dir
    install_topos

    if [ "${TOPOS_UPDATE:-0}" != "1" ]; then
        install_optional_dependencies
        setup_path
    fi

    write_provenance
    verify_install
}

main "$@"
