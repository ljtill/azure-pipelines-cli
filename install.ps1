# install.ps1 — Install azure-devops-cli from GitHub Releases.
#
# Usage:
#   irm https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.ps1 | iex
#
# Environment variables:
#   VERSION      — Pin to a specific version (e.g., "0.2.0"). Defaults to latest.
#   INSTALL_DIR  — Override install directory. Defaults to $HOME\.local\bin.

$ErrorActionPreference = 'Stop'

$Repo = 'ljtill/azure-devops-cli'
$BinaryName = 'devops'
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
$CosignBundleUrl = "https://github.com/$Repo/releases/download/$Tag/SHA256SUMS.cosign.bundle"
$CosignCertIdentityRegex = '^https://github\.com/ljtill/azure-devops-cli/\.github/workflows/ci\.release\.yml@refs/tags/v.+$'
$CosignOidcIssuer = 'https://token.actions.githubusercontent.com'
$IssuesUrl = "https://github.com/$Repo/issues"

# --- validate platform is published

$ApiHeaders = @{}
if ($env:GITHUB_TOKEN) {
    $ApiHeaders['Authorization'] = "Bearer $($env:GITHUB_TOKEN)"
}

$Release = $null
try {
    $Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/tags/$Tag" -Headers $ApiHeaders
} catch {
    Write-Warning "Could not reach GitHub API to validate platform ($($_.Exception.Message)); continuing."
}

if ($null -ne $Release -and $Release.assets) {
    $AssetNames = @($Release.assets | ForEach-Object { $_.name })
    if ($AssetNames -notcontains $Artifact) {
        Write-Host "ERROR: Platform windows/$Arch is not published for $Tag." -ForegroundColor Red
        Write-Host 'Available artifacts:'
        $ArchiveList = @($AssetNames | Where-Object { $_ -match '\.(tar\.gz|zip)$' })
        if ($ArchiveList.Count -eq 0) { $ArchiveList = $AssetNames }
        foreach ($name in $ArchiveList) {
            Write-Host "  - $name"
        }
        Write-Host "If you need this platform, please file an issue at $IssuesUrl"
        exit 1
    }
}

# --- download and install ---------------------------------------------------

Write-Host "Installing $BinaryName $Tag (windows/$Arch)..."

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$Dest = Join-Path $InstallDir "$BinaryName.exe"
$Temp = [System.IO.Path]::GetTempFileName()
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("devops-extract-" + [System.IO.Path]::GetRandomFileName())
$TempChecksums = [System.IO.Path]::GetTempFileName()
$TempBundle = [System.IO.Path]::GetTempFileName()

# --- verify cosign / Sigstore signature over SHA256SUMS

if (-not (Get-Command cosign -ErrorAction SilentlyContinue)) {
    throw "'cosign' is required to verify release signatures. Install it from https://docs.sigstore.dev/cosign/installation/ and retry."
}

Invoke-WebRequest -Uri $ChecksumsUrl -OutFile $TempChecksums -UseBasicParsing
Invoke-WebRequest -Uri $CosignBundleUrl -OutFile $TempBundle -UseBasicParsing

Write-Host 'Verifying cosign signature for SHA256SUMS...'
& cosign verify-blob `
    --bundle $TempBundle `
    --certificate-identity-regexp $CosignCertIdentityRegex `
    --certificate-oidc-issuer $CosignOidcIssuer `
    $TempChecksums | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "cosign signature verification failed for SHA256SUMS ($Tag)"
}
Write-Host 'Signature verified.'

$ChecksumBody = Get-Content -Raw -LiteralPath $TempChecksums
$ExpectedHash = $null
foreach ($line in ($ChecksumBody -split "`r?`n")) {
    $parts = $line -split '\s+', 2
    if ($parts.Count -eq 2 -and $parts[1] -eq $Artifact) {
        $ExpectedHash = $parts[0].ToLowerInvariant()
        break
    }
}
if (-not $ExpectedHash) {
    throw "Could not find checksum for $Artifact. If you need this platform, please file an issue at $IssuesUrl"
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
    if (Test-Path $TempChecksums) {
        Remove-Item -Force $TempChecksums -ErrorAction SilentlyContinue
    }
    if (Test-Path $TempBundle) {
        Remove-Item -Force $TempBundle -ErrorAction SilentlyContinue
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
