#!/usr/bin/env pwsh
#Requires -Version 5.0

# Installs the signing server certificate into the CurrentUser trusted root store
# This only needs to be run once per machine/user

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$CertPath = Join-Path $ScriptDir "certs\https_cert.pem"

if (-not (Test-Path $CertPath)) {
    Write-Error "Certificate not found: $CertPath"
    exit 1
}

Write-Host "Installing signing server certificate..."

$Cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($CertPath)
$Store = New-Object System.Security.Cryptography.X509Certificates.X509Store("Root", "CurrentUser")
$Store.Open("ReadWrite")
$Store.Add($Cert)
$Store.Close()

Write-Host "Certificate installed successfully."
Write-Host "You may now run builds that require signing."