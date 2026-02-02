#!/bin/bash
# Treeline CLI Installer
# Usage: curl -fsSL https://treeline.money/install.sh | sh
#
# Installs the Treeline CLI to /usr/local/bin/tl

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

REPO="treeline-money/treeline"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="tl"

# Detect OS and architecture
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux*)
            case "$ARCH" in
                x86_64)
                    PLATFORM="linux"
                    ARTIFACT="tl-linux-x64"
                    ;;
                *)
                    echo -e "${RED}Error: Unsupported architecture: $ARCH${NC}"
                    echo "Supported: x86_64"
                    exit 1
                    ;;
            esac
            ;;
        Darwin*)
            case "$ARCH" in
                arm64)
                    PLATFORM="macos"
                    ARTIFACT="tl-macos-arm64"
                    ;;
                x86_64)
                    # Intel Mac - check if we have a binary for it
                    echo -e "${YELLOW}Note: Intel Mac support is limited. Trying arm64 binary with Rosetta...${NC}"
                    PLATFORM="macos"
                    ARTIFACT="tl-macos-arm64"
                    ;;
                *)
                    echo -e "${RED}Error: Unsupported architecture: $ARCH${NC}"
                    exit 1
                    ;;
            esac
            ;;
        MINGW*|MSYS*|CYGWIN*)
            echo -e "${YELLOW}For Windows, please use PowerShell:${NC}"
            echo "  irm https://treeline.money/install.ps1 | iex"
            exit 1
            ;;
        *)
            echo -e "${RED}Error: Unsupported operating system: $OS${NC}"
            echo "Supported: Linux, macOS"
            echo "For Windows, use: irm https://treeline.money/install.ps1 | iex"
            exit 1
            ;;
    esac
}

# Get latest release version
get_latest_version() {
    if command -v curl &> /dev/null; then
        VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    elif command -v wget &> /dev/null; then
        VERSION=$(wget -qO- "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    else
        echo -e "${RED}Error: curl or wget is required${NC}"
        exit 1
    fi

    if [ -z "$VERSION" ]; then
        echo -e "${RED}Error: Could not determine latest version${NC}"
        exit 1
    fi
}

# Download and install
install() {
    echo -e "${GREEN}Installing Treeline CLI...${NC}"
    echo ""

    detect_platform
    get_latest_version

    echo "  Platform: $PLATFORM ($ARCH)"
    echo "  Version:  $VERSION"
    echo "  Install:  $INSTALL_DIR/$BINARY_NAME"
    echo ""

    # Create install directory
    mkdir -p "$INSTALL_DIR"

    # Download URL
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$ARTIFACT"

    echo -e "${YELLOW}Downloading...${NC}"

    # Download to temp location first
    TMP_FILE=$(mktemp)
    if command -v curl &> /dev/null; then
        curl -fsSL "$DOWNLOAD_URL" -o "$TMP_FILE"
    else
        wget -q "$DOWNLOAD_URL" -O "$TMP_FILE"
    fi

    chmod +x "$TMP_FILE"

    # Install to /usr/local/bin (may need sudo)
    if [ -w "$INSTALL_DIR" ]; then
        mv "$TMP_FILE" "$INSTALL_DIR/$BINARY_NAME"
    else
        echo -e "${YELLOW}Installing to $INSTALL_DIR requires sudo...${NC}"
        sudo mv "$TMP_FILE" "$INSTALL_DIR/$BINARY_NAME"
        sudo chmod +x "$INSTALL_DIR/$BINARY_NAME"
    fi

    echo ""
    echo -e "${GREEN}Installed successfully to $INSTALL_DIR/$BINARY_NAME${NC}"
    echo ""
    echo "Run 'tl --help' to get started."
}

install
