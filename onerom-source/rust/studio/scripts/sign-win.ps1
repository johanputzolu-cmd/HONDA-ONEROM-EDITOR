#!/usr/bin/env pwsh
#Requires -Version 5.0

# Signs One ROM Studio for Windows using remote signing service
#
# Used for both the application and installer executables

$ErrorActionPreference = "Stop"

# Get and check arguments
$FileName = $args[0]
$Pin = $args[1]

if (-not $FileName) {   
    Write-Error "No filename specified."
    exit 1
}

if (-not $Pin) {
    Write-Error "PIN not specified."
    exit 1
}

# Resolve to absolute path
$FileName = Resolve-Path $FileName

if (-not (Test-Path $FileName)) {
    Write-Error "File not found: $FileName"
    exit 1
}

# Signing service URL
$SigningUrl = "https://sb1:8443/sign"

# Create temporary file for signed output
$TempFile = [System.IO.Path]::GetTempFileName()

try {
    # Create multipart form data
    $FileContent = [System.IO.File]::ReadAllBytes($FileName)
    $Boundary = [System.Guid]::NewGuid().ToString()
    $LF = "`r`n"
    
    $BodyLines = @(
        "--$Boundary",
        "Content-Disposition: form-data; name=`"file`"; filename=`"$(Split-Path $FileName -Leaf)`"",
        "Content-Type: application/octet-stream$LF",
        [System.Text.Encoding]::GetEncoding("iso-8859-1").GetString($FileContent),
        "--$Boundary",
        "Content-Disposition: form-data; name=`"pin`"$LF",
        $Pin,
        "--$Boundary--$LF"
    ) -join $LF
    
    # Upload file to signing service
    $Response = Invoke-WebRequest -Uri $SigningUrl -Method Post -ContentType "multipart/form-data; boundary=$Boundary" -Body $BodyLines -UseBasicParsing
    
    # Save signed file
    [System.IO.File]::WriteAllBytes($TempFile, $Response.Content)
    
    # Replace original with signed version
    Move-Item -Path $TempFile -Destination $FileName -Force
    
    Write-Host "Successfully signed: $FileName"
} catch {
    # Check if this is a certificate trust error
    if ($_.Exception.Message -match "SSL|certificate|trust|TLS") {
        $ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
        Write-Error @"
Signing failed: Certificate not trusted.

Run this once to install the signing server certificate:
    .\scripts\install-signing-cert.ps1

Then re-run the build.
"@
    } else {
        Write-Error "Signing failed: $_"
    }
    
    if (Test-Path $TempFile) { Remove-Item $TempFile }
    exit 1
}