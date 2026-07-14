#!/bin/sh
set -e

# Builds the One ROM Studio application and dmg packages for Linux.
#
# Pre-requisites:
# - Rust:
#
# ```sh
#   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# ```

# Check we're running on Linux
if [ "$(uname -s)" != "Linux" ]; then
    echo "Error: This script must be run on Linux" >&2
    exit 1
fi

# Parse arguments
CLEAN=true
DEPS=true

for arg in "$@"; do
    case "$arg" in
        noclean)
            echo "!!! WARNING: Not cleaning cargo artifacts" >&2
            CLEAN=false
            ;;
        nodeps)
            echo "!!! WARNING: Skipping dependencies installation step" >&2
            DEPS=false
            ;;
        *)
            echo "Error: Unknown argument '$arg'" >&2
            echo "Usage: $0 [nosign] [noclean] [nodeps]" >&2
            exit 1
            ;;
    esac
done

#
# Setup
#

# Install required packages
if [ "$DEPS" = true ]; then
    echo "Installing dependencies..."

    # Clean up any Ubuntu repo files from previous runs
    sudo rm -f /etc/apt/sources.list.d/ubuntu-ports.list
    sudo rm -f /etc/apt/sources.list.d/ubuntu-amd64.sources
    sudo rm -f /etc/apt/sources.list.d/ubuntu-archive.list

    sudo apt update && sudo apt install -y libudev-dev libusb-1.0-0-dev gcc-aarch64-linux-gnu

    # Detect OS and host architecture
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        OS_ID="$ID"
    else
        echo "ERROR: Cannot detect OS" >&2
        exit 1
    fi
    HOST_ARCH=$(dpkg --print-architecture)
    echo "Detected OS: $OS_ID, host architecture: $HOST_ARCH"

    if [ "$OS_ID" = "debian" ]; then
        # On Debian, native repos support both architectures
        sudo dpkg --add-architecture arm64
        sudo dpkg --add-architecture amd64
        sudo apt update && sudo apt install -y libudev-dev:arm64 libusb-1.0-0-dev:arm64
        sudo apt install -y libudev-dev:amd64 libusb-1.0-0-dev:amd64

    elif [ "$OS_ID" = "ubuntu" ]; then
        # On Ubuntu, architectures need different repos
        CODENAME=$(lsb_release -sc)
        
        # Restrict existing repos to native architecture before adding foreign arch
        if [ -f /etc/apt/sources.list.d/ubuntu.sources ]; then
            sudo cp /etc/apt/sources.list.d/ubuntu.sources /tmp/ubuntu.sources.backup
            sudo sed -i '/^Architectures:/d' /etc/apt/sources.list.d/ubuntu.sources
            sudo sed -i "/^Types:/a Architectures: ${HOST_ARCH}" /etc/apt/sources.list.d/ubuntu.sources
        fi
        
        if [ "$HOST_ARCH" = "amd64" ]; then
            # On x86_64 host: add arm64 packages from ports.ubuntu.com
            sudo dpkg --add-architecture arm64
            echo "Configuring ports.ubuntu.com for arm64 packages"
            echo "deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports ${CODENAME} main universe" | sudo tee /etc/apt/sources.list.d/ubuntu-ports.list
            echo "deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports ${CODENAME}-updates main universe" | sudo tee -a /etc/apt/sources.list.d/ubuntu-ports.list
            echo "deb [arch=arm64] http://ports.ubuntu.com/ubuntu-ports ${CODENAME}-security main universe" | sudo tee -a /etc/apt/sources.list.d/ubuntu-ports.list
            sudo apt update && sudo apt install -y libudev-dev:arm64 libusb-1.0-0-dev:arm64
            
            # amd64 packages already available from native repos
            sudo apt install -y libudev-dev:amd64 libusb-1.0-0-dev:amd64
            
        elif [ "$HOST_ARCH" = "arm64" ]; then
            # On arm64 host: arm64 packages already available from native repos
            sudo apt install -y libudev-dev:arm64 libusb-1.0-0-dev:arm64
            
            # Add amd64 packages from archive.ubuntu.com
            sudo dpkg --add-architecture amd64
            echo "Configuring archive.ubuntu.com for amd64 packages"
            echo "deb [arch=amd64] http://archive.ubuntu.com/ubuntu ${CODENAME} main universe" | sudo tee /etc/apt/sources.list.d/ubuntu-archive.list
            echo "deb [arch=amd64] http://archive.ubuntu.com/ubuntu ${CODENAME}-updates main universe" | sudo tee -a /etc/apt/sources.list.d/ubuntu-archive.list
            echo "deb [arch=amd64] http://security.ubuntu.com/ubuntu ${CODENAME}-security main universe" | sudo tee -a /etc/apt/sources.list.d/ubuntu-archive.list
            sudo apt update && sudo apt install -y libudev-dev:amd64 libusb-1.0-0-dev:amd64
        fi

    else
        echo "ERROR: Unsupported OS: $OS_ID" >&2
        exit 1
    fi

    # Verify packages installed
    if ! dpkg -l | grep -q "libudev-dev.*arm64"; then
        echo "ERROR: libudev-dev:arm64 not installed" >&2
        exit 1
    fi
    echo "libudev-dev:arm64 is installed."

    if ! dpkg -l | grep -q "libudev-dev.*amd64"; then
        echo "ERROR: libudev-dev:amd64 not installed" >&2
        exit 1
    fi
    echo "libudev-dev:amd64 is installed."

    # Install the Rust targets
    rustup target add x86_64-unknown-linux-gnu
    rustup target add aarch64-unknown-linux-gnu

    # Install cargo-deb if not already installed
    # Forced to ensure it builds against glibc version on this system
    cargo install cargo-deb --locked --force
else
    echo "Skipping dependencies installation step."
fi

#
# Clean previous builds
#

if [ "$CLEAN" = true ]; then
    echo "Cleaning previous build artifacts..."
    cargo clean --target x86_64-unknown-linux-gnu
    cargo clean --target aarch64-unknown-linux-gnu
    rm -fr dist/*.deb
else
    echo "Skipping cleaning of previous build artifacts."
fi

#
# Intel (x86_64)
#

# Build One ROM Studio
PACKAGER_TARGET="x86_64-unknown-linux-gnu"
export PKG_CONFIG_SYSROOT_DIR=/
export PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc
echo "Building for target: $PACKAGER_TARGET"
cargo build --bin onerom-studio --release --target $PACKAGER_TARGET

# Package as a deb
echo "Packaging dmg for target: $PACKAGER_TARGET"
cargo deb -v --target $PACKAGER_TARGET

echo "Linux x86_64 build complete."

#
# ARM (aarch64)
#

# Build One ROM Studio
# Note: Requires setting PKG_CONFIG_SYSROOT_DIR and PKG_CONFIG_PATH to find
# the arm64 libudev-dev files
PACKAGER_TARGET="aarch64-unknown-linux-gnu"
export PKG_CONFIG_SYSROOT_DIR=/
export PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
echo "Building for target: $PACKAGER_TARGET"
cargo build --bin onerom-studio --release --target $PACKAGER_TARGET

# Package as a deb
echo "Packaging deb for target: $PACKAGER_TARGET"
cargo deb -v --target $PACKAGER_TARGET
echo "Linux ARM64 build complete."

# Copy .deb files to dist/
mkdir -p dist
cp ../target/debian/*.deb dist/

echo "Build complete."