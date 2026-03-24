#!/usr/bin/env bash
# =============================================================================
# OVC — Installer for Linux & macOS
# =============================================================================
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Olib-AI/ovc/main/scripts/install.sh | bash
#
# Or download and inspect first:
#   curl -fsSL https://raw.githubusercontent.com/Olib-AI/ovc/main/scripts/install.sh -o install.sh
#   chmod +x install.sh
#   ./install.sh
#
# Options:
#   --version VERSION    Install a specific version (default: latest)
#   --uninstall          Remove OVC completely
#   --update             Update binary only, preserve keys and config
#   --help               Show this help message

set -euo pipefail

# ── Constants ─────────────────────────────────────────────────────────────────

REPO="Olib-AI/ovc"
BINARY_NAME="ovc"

# ── Colors ────────────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m' # No Color

info()    { echo -e "${BLUE}[info]${NC}  $*"; }
success() { echo -e "${GREEN}[ok]${NC}    $*"; }
warn()    { echo -e "${YELLOW}[warn]${NC}  $*"; }
error()   { echo -e "${RED}[error]${NC} $*" >&2; }
fatal()   { error "$@"; exit 1; }

# ── Argument parsing ──────────────────────────────────────────────────────────

VERSION="latest"
UNINSTALL=false
UPDATE=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)   VERSION="$2"; shift 2 ;;
        --uninstall) UNINSTALL=true; shift ;;
        --update)    UPDATE=true; shift ;;
        --help|-h)
            echo "OVC Installer"
            echo ""
            echo "Usage: install.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --version VERSION    Install specific version (default: latest)"
            echo "  --uninstall          Remove OVC completely"
            echo "  --update             Update binary, preserve keys and config"
            echo "  --help               Show this help message"
            exit 0
            ;;
        *) fatal "Unknown option: $1. Use --help for usage." ;;
    esac
done

# ── Platform detection ────────────────────────────────────────────────────────

detect_platform() {
    local os arch

    case "$(uname -s)" in
        Linux)  os="linux" ;;
        Darwin) os="darwin" ;;
        *)      fatal "Unsupported OS: $(uname -s). Only Linux and macOS are supported." ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64)       arch="amd64" ;;
        aarch64|arm64)      arch="arm64" ;;
        *)                  fatal "Unsupported architecture: $(uname -m). Only amd64 and arm64 are supported." ;;
    esac

    OS="$os"
    ARCH="$arch"
}

# ── Path configuration ────────────────────────────────────────────────────────

configure_paths() {
    if [ "$(id -u)" -eq 0 ]; then
        SUDO_CMD=""
        INSTALL_DIR="/usr/local/bin"
        USE_SUDO=false
    elif command -v sudo >/dev/null 2>&1; then
        SUDO_CMD="sudo"
        INSTALL_DIR="/usr/local/bin"
        USE_SUDO=true
    else
        SUDO_CMD=""
        INSTALL_DIR="${HOME}/.local/bin"
        USE_SUDO=false
    fi

    BINARY_PATH="${INSTALL_DIR}/${BINARY_NAME}"
    KEY_DIR="${HOME}/.ssh/ovc"
}

# ── Prerequisites ─────────────────────────────────────────────────────────────

check_prereqs() {
    if ! command -v curl >/dev/null 2>&1; then
        fatal "curl is required but not installed. Install it with your package manager."
    fi
}

# ── Version resolution ────────────────────────────────────────────────────────

resolve_version() {
    if [ "$VERSION" = "latest" ]; then
        info "Fetching latest release version..."
        VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep '"tag_name"' | head -1 | cut -d'"' -f4) \
            || fatal "Failed to fetch latest version. Check your internet connection."
        [ -n "$VERSION" ] || fatal "Could not determine latest version."
    fi
    success "Version: $VERSION"
}

# ── Download and verify ──────────────────────────────────────────────────────

download_binary() {
    local artifact="ovc-${OS}-${ARCH}"
    local base_url="https://github.com/${REPO}/releases/download/${VERSION}"
    local tmp_dir
    tmp_dir=$(mktemp -d)

    info "Downloading ${artifact}..."
    curl -fSL "${base_url}/${artifact}" -o "${tmp_dir}/${artifact}" \
        || fatal "Failed to download binary. Check that version ${VERSION} exists."

    info "Downloading checksums..."
    curl -fSL "${base_url}/SHA256SUMS.txt" -o "${tmp_dir}/SHA256SUMS.txt" \
        || fatal "Failed to download checksums."

    info "Verifying SHA256 checksum..."
    local expected
    expected=$(grep "${artifact}" "${tmp_dir}/SHA256SUMS.txt" | awk '{print $1}')
    [ -n "$expected" ] || fatal "Binary ${artifact} not found in SHA256SUMS.txt"

    local actual
    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "${tmp_dir}/${artifact}" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "${tmp_dir}/${artifact}" | awk '{print $1}')
    else
        fatal "Neither sha256sum nor shasum found. Cannot verify checksum."
    fi

    if [ "$expected" != "$actual" ]; then
        fatal "Checksum mismatch!\n  Expected: ${expected}\n  Actual:   ${actual}"
    fi
    success "Checksum verified"

    DOWNLOAD_PATH="${tmp_dir}/${artifact}"
}

# ── Install binary ────────────────────────────────────────────────────────────

install_binary() {
    info "Installing binary to ${BINARY_PATH}..."
    $SUDO_CMD mkdir -p "$INSTALL_DIR"
    $SUDO_CMD install -m 755 "$DOWNLOAD_PATH" "$BINARY_PATH"
    $SUDO_CMD chmod +x "$BINARY_PATH"

    # macOS: remove quarantine and ad-hoc sign
    if [ "$OS" = "darwin" ]; then
        $SUDO_CMD xattr -cr "$BINARY_PATH" 2>/dev/null || true
        $SUDO_CMD codesign --force --sign - "$BINARY_PATH" 2>/dev/null || true
    fi

    success "Binary installed"

    local version_output
    version_output=$("$BINARY_PATH" --version 2>&1 || true)
    info "Installed: ${version_output}"
}

# ── Ensure PATH includes install dir ─────────────────────────────────────────

ensure_path() {
    if [[ ":$PATH:" != *":${INSTALL_DIR}:"* ]]; then
        warn "${INSTALL_DIR} is not in your PATH."
        echo ""
        echo "  Add this to your shell profile (~/.zshrc or ~/.bashrc):"
        echo ""
        echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
        echo ""
    fi
}

# ── Uninstall ─────────────────────────────────────────────────────────────────

do_uninstall() {
    echo -e "${BOLD}OVC — Uninstall${NC}"
    echo ""

    # Remove binary
    if [ -f "$BINARY_PATH" ]; then
        info "Removing binary..."
        $SUDO_CMD rm -f "$BINARY_PATH"
        success "Binary removed"
    else
        info "Binary not found at ${BINARY_PATH}"
    fi

    # Ask about keys
    if [ -d "$KEY_DIR" ]; then
        echo ""
        warn "Key directory found at ${KEY_DIR}"
        warn "This contains your OVC key pairs."
        warn "If you delete it, you will lose your keys and cannot decrypt your repos."
        echo ""
        read -rp "Delete key directory? [y/N] " confirm
        if [[ "$confirm" =~ ^[Yy]$ ]]; then
            rm -rf "$KEY_DIR"
            success "Key directory removed"
        else
            info "Key directory preserved at ${KEY_DIR}"
        fi
    fi

    echo ""
    success "OVC has been uninstalled."
}

# ── Update ────────────────────────────────────────────────────────────────────

do_update() {
    echo -e "${BOLD}OVC — Update${NC}"
    echo ""

    if [ ! -f "$BINARY_PATH" ]; then
        fatal "OVC is not installed at ${BINARY_PATH}. Run without --update to install."
    fi

    resolve_version
    download_binary
    install_binary

    echo ""
    success "OVC updated to ${VERSION}"
}

# ── Main install flow ─────────────────────────────────────────────────────────

do_install() {
    echo ""
    echo -e "${BOLD}OVC — Installer${NC}"
    echo -e "Secure, self-hosted version control"
    echo ""

    if [ -f "$BINARY_PATH" ]; then
        warn "OVC is already installed at ${BINARY_PATH}"
        warn "Use --update to update or --uninstall to remove first."
        exit 1
    fi

    resolve_version
    download_binary
    install_binary
    ensure_path

    echo ""
    echo -e "${BOLD}════════════════════════════════════════════════════════════${NC}"
    echo -e "${BOLD}  OVC is installed!${NC}"
    echo ""
    echo -e "  Get started:"
    echo ""
    echo -e "  ${GREEN}ovc key generate --name mykey --identity \"Your Name <you@email.com>\"${NC}"
    echo -e "  ${GREEN}ovc init --name my-project.ovc --key mykey${NC}"
    echo -e "  ${GREEN}ovc add . && ovc commit -m \"initial commit\"${NC}"
    echo ""
    echo -e "  Run ${BLUE}ovc onboard${NC} for an interactive setup wizard."
    echo -e "  Full docs: ${BLUE}https://github.com/${REPO}${NC}"
    echo -e "${BOLD}════════════════════════════════════════════════════════════${NC}"
    echo ""
}

# ── Entry point ───────────────────────────────────────────────────────────────

detect_platform
check_prereqs
configure_paths

if [ "$UNINSTALL" = true ]; then
    do_uninstall
elif [ "$UPDATE" = true ]; then
    do_update
else
    do_install
fi
