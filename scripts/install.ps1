<#
.SYNOPSIS
    OVC installer for Windows.

.DESCRIPTION
    Downloads, verifies, and installs OVC (Olib Version Control).
    Pre-built binaries are fetched from GitHub Releases with SHA256
    checksum verification.

.PARAMETER Version
    Specific version to install (e.g., "v0.1.3"). Defaults to "latest".

.PARAMETER Uninstall
    Remove OVC completely.

.PARAMETER Update
    Update the binary while preserving keys and config.

.EXAMPLE
    # Install latest version
    irm https://raw.githubusercontent.com/Olib-AI/ovc/main/scripts/install.ps1 | iex

    # Install specific version
    .\install.ps1 -Version v0.1.3

    # Update to latest
    .\install.ps1 -Update

    # Uninstall
    .\install.ps1 -Uninstall
#>

param(
    [string]$Version = "latest",
    [switch]$Uninstall,
    [switch]$Update
)

$ErrorActionPreference = "Stop"

# ── Constants ─────────────────────────────────────────────────────────────────

$Repo = "Olib-AI/ovc"
$BinaryName = "ovc.exe"
$InstallDir = Join-Path $env:LOCALAPPDATA "ovc\bin"
$BinaryPath = Join-Path $InstallDir $BinaryName
$KeyDir = Join-Path $env:USERPROFILE ".ssh\ovc"

# ── Helpers ───────────────────────────────────────────────────────────────────

function Write-Info    { param($Msg) Write-Host "[info]  $Msg" -ForegroundColor Cyan }
function Write-Success { param($Msg) Write-Host "[ok]    $Msg" -ForegroundColor Green }
function Write-Warn    { param($Msg) Write-Host "[warn]  $Msg" -ForegroundColor Yellow }
function Write-Fatal   { param($Msg) Write-Host "[error] $Msg" -ForegroundColor Red; exit 1 }

# ── Platform detection ────────────────────────────────────────────────────────

function Get-Architecture {
    if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") {
        return "arm64"
    }
    return "amd64"
}

# ── Version resolution ────────────────────────────────────────────────────────

function Resolve-Version {
    if ($Version -eq "latest") {
        Write-Info "Fetching latest release version..."
        try {
            [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
            $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
            $script:Version = $release.tag_name
        }
        catch {
            Write-Fatal "Failed to fetch latest version: $_"
        }
    }
    Write-Success "Version: $Version"
}

# ── Download and verify ──────────────────────────────────────────────────────

function Download-Binary {
    $arch = Get-Architecture
    $artifact = "ovc-windows-${arch}.exe"
    $baseUrl = "https://github.com/$Repo/releases/download/$Version"
    $tmpDir = Join-Path $env:TEMP "ovc-install"

    if (Test-Path $tmpDir) { Remove-Item -Recurse -Force $tmpDir }
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    Write-Info "Downloading $artifact..."
    try {
        Invoke-WebRequest -Uri "$baseUrl/$artifact" -OutFile (Join-Path $tmpDir $artifact)
    }
    catch {
        Write-Fatal "Failed to download binary. Check that version $Version exists."
    }

    Write-Info "Downloading checksums..."
    try {
        Invoke-WebRequest -Uri "$baseUrl/SHA256SUMS.txt" -OutFile (Join-Path $tmpDir "SHA256SUMS.txt")
    }
    catch {
        Write-Fatal "Failed to download checksums."
    }

    Write-Info "Verifying SHA256 checksum..."
    $checksumLine = Get-Content (Join-Path $tmpDir "SHA256SUMS.txt") | Where-Object { $_ -match $artifact }
    if (-not $checksumLine) {
        Write-Fatal "Binary $artifact not found in SHA256SUMS.txt"
    }
    $expected = ($checksumLine -split '\s+')[0]
    $actual = (Get-FileHash -Path (Join-Path $tmpDir $artifact) -Algorithm SHA256).Hash.ToLower()

    if ($expected -ne $actual) {
        Write-Fatal "Checksum mismatch!`n  Expected: $expected`n  Actual:   $actual"
    }
    Write-Success "Checksum verified"

    return Join-Path $tmpDir $artifact
}

# ── Install binary ────────────────────────────────────────────────────────────

function Install-Binary {
    param($SourcePath)

    Write-Info "Installing binary to $BinaryPath..."
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -Path $SourcePath -Destination $BinaryPath -Force
    Write-Success "Binary installed"

    # Verify
    try {
        $ver = & $BinaryPath --version 2>&1
        Write-Info "Installed: $ver"
    }
    catch {
        Write-Info "Binary copied (version check skipped)"
    }
}

# ── Ensure PATH includes install dir ─────────────────────────────────────────

function Ensure-Path {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$InstallDir*") {
        Write-Info "Adding $InstallDir to user PATH..."
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$InstallDir", "User")
        $env:Path = "$env:Path;$InstallDir"
        Write-Success "Added to PATH (restart your terminal to take effect)"
    }
}

# ── Uninstall ─────────────────────────────────────────────────────────────────

function Do-Uninstall {
    Write-Host "OVC - Uninstall" -ForegroundColor White
    Write-Host ""

    # Remove binary
    if (Test-Path $BinaryPath) {
        Write-Info "Removing binary..."
        Remove-Item -Force $BinaryPath
        Write-Success "Binary removed"
    }
    else {
        Write-Info "Binary not found at $BinaryPath"
    }

    # Remove install dir if empty
    if ((Test-Path $InstallDir) -and -not (Get-ChildItem $InstallDir)) {
        Remove-Item -Force $InstallDir
    }

    # Remove from PATH
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -like "*$InstallDir*") {
        $newPath = ($userPath -split ";" | Where-Object { $_ -ne $InstallDir }) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Success "Removed from PATH"
    }

    # Ask about keys
    if (Test-Path $KeyDir) {
        Write-Host ""
        Write-Warn "Key directory found at $KeyDir"
        Write-Warn "This contains your OVC key pairs."
        Write-Warn "If you delete it, you will lose your keys and cannot decrypt your repos."
        Write-Host ""
        $confirm = Read-Host "Delete key directory? [y/N]"
        if ($confirm -match "^[Yy]$") {
            Remove-Item -Recurse -Force $KeyDir
            Write-Success "Key directory removed"
        }
        else {
            Write-Info "Key directory preserved at $KeyDir"
        }
    }

    Write-Host ""
    Write-Success "OVC has been uninstalled."
}

# ── Update ────────────────────────────────────────────────────────────────────

function Do-Update {
    Write-Host "OVC - Update" -ForegroundColor White
    Write-Host ""

    if (-not (Test-Path $BinaryPath)) {
        Write-Fatal "OVC is not installed at $BinaryPath. Run without -Update to install."
    }

    Resolve-Version
    $downloadPath = Download-Binary
    Install-Binary -SourcePath $downloadPath

    Write-Host ""
    Write-Success "OVC updated to $Version"
}

# ── Main install ──────────────────────────────────────────────────────────────

function Do-Install {
    Write-Host ""
    Write-Host "OVC - Installer" -ForegroundColor White
    Write-Host "Secure, self-hosted version control" -ForegroundColor Gray
    Write-Host ""

    if (Test-Path $BinaryPath) {
        Write-Warn "OVC is already installed at $BinaryPath"
        Write-Warn "Use -Update to update or -Uninstall to remove first."
        exit 1
    }

    Resolve-Version
    $downloadPath = Download-Binary
    Install-Binary -SourcePath $downloadPath
    Ensure-Path

    Write-Host ""
    Write-Host "================================================================" -ForegroundColor White
    Write-Host "  OVC is installed!" -ForegroundColor White
    Write-Host ""
    Write-Host "  Get started:" -ForegroundColor White
    Write-Host ""
    Write-Host "  ovc key generate --name mykey --identity `"Your Name <you@email.com>`"" -ForegroundColor Green
    Write-Host "  ovc init --name my-project.ovc --key mykey" -ForegroundColor Green
    Write-Host "  ovc add . && ovc commit -m `"initial commit`"" -ForegroundColor Green
    Write-Host ""
    Write-Host "  Run 'ovc onboard' for an interactive setup wizard." -ForegroundColor Cyan
    Write-Host "  Full docs: https://github.com/$Repo" -ForegroundColor Cyan
    Write-Host "================================================================" -ForegroundColor White
    Write-Host ""
}

# ── Entry point ───────────────────────────────────────────────────────────────

if ($Uninstall) {
    Do-Uninstall
}
elseif ($Update) {
    Do-Update
}
else {
    Do-Install
}
