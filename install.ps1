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
$Arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'amd64' }
$InnerBinary = "$BinaryName-windows-$Arch.exe"
$Artifact = "$BinaryName-windows-$Arch.zip"

$InstallDir = if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { Join-Path $HOME '.local\bin' }

# --- resolve version --------------------------------------------------------

if ($env:VERSION) {
    $Version = $env:VERSION
} else {
    Write-Host 'Fetching latest release...'
    $Headers = @{}
    if ($env:GITHUB_TOKEN) {
        $Headers['Authorization'] = "token $($env:GITHUB_TOKEN)"
    }
    $Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -Headers $Headers
    $Version = $Release.tag_name -replace '^v', ''
}

$Tag = "v$Version"
$Url = "https://github.com/$Repo/releases/download/$Tag/$Artifact"
$ChecksumsUrl = "https://github.com/$Repo/releases/download/$Tag/SHA256SUMS"

# --- download and install ---------------------------------------------------

Write-Host "Installing $BinaryName $Tag (windows/$Arch)..."

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$Dest = Join-Path $InstallDir "$BinaryName.exe"
$Temp = [System.IO.Path]::GetTempFileName()
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("pipelines-extract-" + [System.IO.Path]::GetRandomFileName())
$RawContent = (Invoke-WebRequest -Uri $ChecksumsUrl -UseBasicParsing).Content
$ChecksumBody = if ($RawContent -is [byte[]]) {
    [System.Text.Encoding]::UTF8.GetString($RawContent)
} else {
    $RawContent
}
$ExpectedHash = $null
foreach ($line in ($ChecksumBody -split "`r?`n")) {
    $parts = $line -split '\s+', 2
    if ($parts.Count -eq 2 -and $parts[1] -eq $Artifact) {
        $ExpectedHash = $parts[0].ToLowerInvariant()
        break
    }
}
if (-not $ExpectedHash) {
    throw "Could not find checksum for $Artifact"
}

try {
    Invoke-WebRequest -Uri $Url -OutFile $Temp -UseBasicParsing
    $ActualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Temp).Hash.ToLowerInvariant()
    if ($ActualHash -ne $ExpectedHash) {
        throw "Checksum mismatch for $Artifact"
    }

    New-Item -ItemType Directory -Path $TempDir -Force | Out-Null
    Expand-Archive -Path $Temp -DestinationPath $TempDir -Force
    Move-Item -Force (Join-Path $TempDir $InnerBinary) $Dest
}
finally {
    if (Test-Path $Temp) {
        Remove-Item -Force $Temp -ErrorAction SilentlyContinue
    }
    if (Test-Path $TempDir) {
        Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
    }
}

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
