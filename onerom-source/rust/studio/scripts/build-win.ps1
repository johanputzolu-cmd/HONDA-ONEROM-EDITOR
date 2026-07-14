#!/usr/bin/env pwsh
#Requires -Version 5.0

# Builds the One ROM Studio application and NSIS installers for Windows.
#
# Pre-requisites:
# - Rust:
#
# ```powershell
#   Invoke-WebRequest -Uri https://win.rustup.rs/ -OutFile rustup-init.exe
#   .\rustup-init.exe -y
# ```
#
# Note it is strongly recommended that you run this script from a Developer
# PowerShell prompt (e.g., "x64 Native Tools Command Prompt for VS 2022") so
# the various build tools are in your PATH.
#
# Note that signing DOES NOT WORK on arm64 Windows due to Certum not providing
# an arm64 minidriver.  https://deciphertools.com/blog/yubikey-5-parallels-arm
#
# The `sign-win.ps1` instead uses https://github.com/piersfinlayson/certum-code-signer.git
# a remote signing service for Certum certificates running on Intel Linux.
#
# When running with signing for the first time, you must first install the
# signing server certificate by running:
#
#   .\scripts\install-signing-cert.ps1
#
# This only needs to be done once per machine/user.

$ErrorActionPreference = "Stop"

# Parse command line arguments
$NoSign = $args -contains "nosign"
$NoDeps = $args -contains "nodeps"
$NoClean = $args -contains "noclean"

# Extract PIN from arguments (format: pin=VALUE)
$Pin = $null
foreach ($arg in $args) {
    if ($arg -like "pin=*") {
        $Pin = $arg.Substring(4)
    }
}

# Check for unexpected arguments
$ValidArgs = @("nosign", "nodeps", "noclean")
foreach ($arg in $args) {
    if ($arg -notin $ValidArgs -and $arg -notlike "pin=*") {
        Write-Error "Unknown argument: $arg. Valid arguments are: $($ValidArgs -join ', '), pin=VALUE"
        exit 1
    }
}

# Validate PIN if signing
if (-not $NoSign -and -not $Pin) {
    Write-Error "PIN required for signing. Use: pin=SMARTCARD_PIN"
    exit 1
}

# Log args
if ($NoSign) {
    Write-Host "!!!WARNING: Code signing disabled"
}
if ($NoDeps) {
    Write-Host "!!!WARNING: Dependency installation disabled"
}
if ($NoClean) {
    Write-Host "!!!WARNING: Clean disabled"
}

$Targets = @("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")

#
# Setup
#

# Extract version from Cargo.toml
$Version = (Get-Content "Cargo.toml" | Select-String -Pattern '^version\s*=\s*"([^"]+)"').Matches.Groups[1].Value
Write-Host "Building version: $Version"

if (-not $NoDeps) {
    # Install the Rust targets
    foreach ($Target in $Targets) {
        rustup target add $Target
    }
}

# Install cargo-packager if not already installed
cargo install cargo-packager --locked

#
# Clean previous builds
#

if (-not $NoClean) {
    foreach ($Target in $Targets) {
        cargo clean --target $Target
    }
    Remove-Item -Path "dist\*.exe" -Force -ErrorAction SilentlyContinue
}

#
# Build for each target
#

foreach ($Target in $Targets) {
    Write-Host "`n=== Building for $Target ===`n"
    
    # Build One ROM Studio
    Write-Host "Building for target: $Target"
    cargo build --bin onerom-studio --release --target $Target | Out-Host
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Cargo build failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }

    # Sign the executable
    if (-not $NoSign) {
        Write-Host "Signing executable..."
        & "scripts\sign-win.ps1" "..\target\$Target\release\onerom-studio.exe" $Pin
        if ($LASTEXITCODE -ne 0) {
            Write-Error "Signing executable failed with exit code $LASTEXITCODE"
            exit $LASTEXITCODE
        }
    }

    # Create temporary versioned Packager.toml
    $PackagerContent = Get-Content "Packager.toml"
    $PackagerContent = $PackagerContent -replace "%VERSION%", $Version
    $PackagerContent = $PackagerContent -replace "%TARGET%", $Target
    $TempPackagerPath = "Packager_temp.toml"

    # Write the temporary Packager.toml
    $PackagerContent | Set-Content $TempPackagerPath

    # Package as NSIS installer
    Write-Host "Packaging NSIS installer for target: $Target"
    cargo packager -c $TempPackagerPath --release --target $Target --formats nsis | Out-Host
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Cargo packager failed with exit code $LASTEXITCODE"
        Remove-Item -Path $TempPackagerPath -Force -ErrorAction SilentlyContinue
        exit $LASTEXITCODE
    }

    # Remove temporary Packager.toml
    Remove-Item -Path $TempPackagerPath -Force

    # Sign the installer
    if (-not $NoSign) {
        # Determine installer filename suffix
        $InstallerArch = if ($Target -eq "x86_64-pc-windows-msvc") { "x64" } else { "arm64" }
        Write-Host "Signing installer..."
        & "scripts\sign-win.ps1" "dist\onerom-studio_${Version}_${InstallerArch}-setup.exe" $Pin
        if ($LASTEXITCODE -ne 0) {
            Write-Error "Signing installer failed with exit code $LASTEXITCODE"
            exit $LASTEXITCODE
        }
    }
}

Write-Host "`nWindows builds complete (x64 and ARM64)."
