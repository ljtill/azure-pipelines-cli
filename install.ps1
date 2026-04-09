# install.ps1 — Install azure-pipelines-cli from GitHub Releases.
#
# Usage:
#   irm https://raw.githubusercontent.com/ljtill/azure-pipelines-cli/main/install.ps1 | iex
#
# Environment variables:
#   VERSION      — Pin to a specific version (e.g., "0.2.0"). Defaults to latest.
#   INSTALL_DIR  — Override install directory. Defaults to $HOME\.local\bin.

$ErrorActionPreference = 'Stop'

$Repo = 'ljtill/azure-pipelines-cli'
$BinaryName = 'pipelines'
$Artifact = "$BinaryName-windows-amd64.exe"

$InstallDir = if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { Join-Path $HOME '.local\bin' }

# --- resolve version --------------------------------------------------------

if ($env:VERSION) {
    $Version = $env:VERSION
} else {
    Write-Host 'Fetching latest release...'
    $Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $Release.tag_name -replace '^v', ''
}

$Tag = "v$Version"
$Url = "https://github.com/$Repo/releases/download/$Tag/$Artifact"

# --- download and install ---------------------------------------------------

Write-Host "Installing $BinaryName $Tag (windows/amd64)..."

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$Dest = Join-Path $InstallDir "$BinaryName.exe"
Invoke-WebRequest -Uri $Url -OutFile $Dest -UseBasicParsing

Write-Host "Installed to $Dest"

# --- PATH check -------------------------------------------------------------

$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath -notlike "*$InstallDir*") {
    Write-Host ''
    Write-Host "Add $InstallDir to your PATH:"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', `"$InstallDir;`$env:Path`", 'User')"
    Write-Host ''
    Write-Host 'Then restart your terminal for the change to take effect.'
}
