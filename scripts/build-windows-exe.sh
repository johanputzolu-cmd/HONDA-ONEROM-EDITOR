#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/minimal-build/docs/Cargo.toml"
TARGET_TRIPLE="x86_64-pc-windows-gnu"
TARGET_DIR="$REPO_ROOT/minimal-build/docs/target/$TARGET_TRIPLE/release"
SOURCE_EXE="$TARGET_DIR/onerom-honda-edition.exe"
OUTPUT_EXE="$REPO_ROOT/minimal-run/onerom-honda-edition.exe"

copy_runtime_dll() {
  local dll_name="$1"
  local dll_path

  dll_path="$(x86_64-w64-mingw32-gcc -print-file-name="$dll_name")"
  if [[ -n "$dll_path" && -f "$dll_path" ]]; then
    cp -f "$dll_path" "$REPO_ROOT/minimal-run/"
    echo "Bundled runtime DLL: $(basename "$dll_path")"
  else
    echo "Warning: could not locate runtime DLL $dll_name"
  fi
}

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: cargo is not installed. Install Rust first."
  exit 1
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "Error: rustup is required to install Windows target."
  exit 1
fi

if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
  cat <<'EOF'
Error: mingw-w64 toolchain is missing.
Install it on Ubuntu/Debian with:
  sudo apt update
  sudo apt install -y mingw-w64
EOF
  exit 1
fi

rustup target add "$TARGET_TRIPLE"

cargo build --release \
  --manifest-path "$MANIFEST_PATH" \
  --target "$TARGET_TRIPLE"

if [[ ! -f "$SOURCE_EXE" ]]; then
  echo "Error: build completed but .exe was not found at: $SOURCE_EXE"
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT_EXE")"
cp -f "$SOURCE_EXE" "$OUTPUT_EXE"

# Bundle MinGW runtime DLLs next to the executable so it opens via double-click on Windows.
copy_runtime_dll "libgcc_s_seh-1.dll"
copy_runtime_dll "libstdc++-6.dll"
copy_runtime_dll "libwinpthread-1.dll"

echo "Windows executable generated:"
echo "  $OUTPUT_EXE"
echo "Windows runtime files copied to:"
echo "  $REPO_ROOT/minimal-run/"
