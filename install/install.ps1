#!/usr/bin/env pwsh
# gvm (Go Version Manager) - Windows installer
#
# Usage (one-liner):
#   irm https://raw.githubusercontent.com/jhonsferg/gvm/main/install/install.ps1 | iex
#
# Customise with environment variables before piping:
#   $env:GVM_INSTALL_DIR = "$env:USERPROFILE\.local\bin"
#   $env:GVM_VERSION     = "v1.0.0"
#
# Override base URLs for local/offline testing:
#   $env:GVM_TEST_API_BASE = "http://localhost:8765"   # replaces https://api.github.com
#   $env:GVM_TEST_DL_BASE  = "http://localhost:8765"   # replaces https://github.com

$ErrorActionPreference = "Stop"

$REPO        = "jhonsferg/gvm"
$InstallDir  = if ($env:GVM_INSTALL_DIR)    { $env:GVM_INSTALL_DIR }    else { "$env:USERPROFILE\.local\bin" }
$Version     = if ($env:GVM_VERSION)        { $env:GVM_VERSION }        else { "latest" }
$ApiBase     = if ($env:GVM_TEST_API_BASE)  { $env:GVM_TEST_API_BASE }  else { "https://api.github.com" }
$DlBase      = if ($env:GVM_TEST_DL_BASE)   { $env:GVM_TEST_DL_BASE }   else { "https://github.com" }

# ── Terminal helpers ──────────────────────────────────────────────────────────
function Write-Step([string]$msg) { Write-Host "  -> $msg" -ForegroundColor Cyan }
function Write-Ok([string]$msg)   { Write-Host "  v  $msg" -ForegroundColor Green }
function Write-Warn([string]$msg) { Write-Host "  !  $msg" -ForegroundColor Yellow }
function Abort([string]$msg) {
    Write-Host "`n  x  $msg" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "  gvm" -ForegroundColor Cyan -NoNewline
Write-Host " -- Go Version Manager installer" -ForegroundColor White
Write-Host ""

# ── 1. Detect architecture ────────────────────────────────────────────────────
$isArm = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture `
         -eq [System.Runtime.InteropServices.Architecture]::Arm64

$Arch = if ($isArm) { "aarch64" } else { "x86_64" }

if (-not [System.Environment]::Is64BitOperatingSystem) {
    Abort "32-bit Windows is not supported."
}

Write-Step "Detected platform: windows-$Arch"

# ── 2. Resolve version ────────────────────────────────────────────────────────
if ($Version -eq "latest") {
    Write-Step "Fetching latest release from $ApiBase..."
    try {
        $rel     = Invoke-RestMethod "$ApiBase/repos/$REPO/releases/latest"
        $Version = $rel.tag_name
    }
    catch {
        Abort "Could not fetch latest version: $_"
    }
}

Write-Step "Installing gvm $Version"

# ── 3. Download binary ────────────────────────────────────────────────────────
$BinaryName  = "gvm-windows-$Arch.exe"
$DownloadUrl = "$DlBase/$REPO/releases/download/$Version/$BinaryName"
$TmpFile     = [System.IO.Path]::Combine(
                   [System.IO.Path]::GetTempPath(),
                   "gvm-install-$([System.Guid]::NewGuid()).exe"
               )

Write-Step "Downloading $BinaryName from $DownloadUrl..."

try {
    $ProgressPreference = "SilentlyContinue"
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $TmpFile -UseBasicParsing
}
catch {
    if (Test-Path $TmpFile) { Remove-Item -Force $TmpFile }
    Abort "Download failed.`n  URL: $DownloadUrl`n  Error: $_"
}

# ── 4. Install ────────────────────────────────────────────────────────────────
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

$Dest = Join-Path $InstallDir "gvm.exe"

# Overwrite gracefully even if the binary is currently running
if (Test-Path $Dest) {
    Remove-Item -Force $Dest -ErrorAction SilentlyContinue
}

Move-Item -Force $TmpFile $Dest
Write-Ok "Installed to $Dest"

# Verify the binary is functional before touching anything else
try {
    $installedVersion = & $Dest --version 2>&1
    Write-Ok "Binary check: $installedVersion"
}
catch {
    Abort "Installed binary failed to run: $_"
}

# ── 5. Persist install dir in the user PATH (registry) ───────────────────────
$UserPath = [Environment]::GetEnvironmentVariable("PATH", [EnvironmentVariableTarget]::User)
if ($null -eq $UserPath) { $UserPath = "" }

if ($UserPath -notlike "*$InstallDir*") {
    $NewPath = "$InstallDir;$UserPath".TrimEnd(";")
    [Environment]::SetEnvironmentVariable("PATH", $NewPath, [EnvironmentVariableTarget]::User)
    Write-Ok "Added $InstallDir to your user PATH"
} else {
    Write-Ok "$InstallDir is already in your user PATH"
}

# Inject into the current process PATH so gvm setup works immediately
if ($env:PATH -notlike "*$InstallDir*") {
    $env:PATH = "$InstallDir;$env:PATH"
}

# ── 6. Configure shell profile ────────────────────────────────────────────────
Write-Step "Configuring PowerShell profile..."

try {
    & $Dest setup --shell powershell
}
catch {
    Write-Warn "Could not configure profile automatically: $_"
    Write-Warn "Run 'gvm setup --shell powershell' manually after restarting your terminal."
}

# ── 7. Summary ────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "  gvm $Version installed successfully!" -ForegroundColor Green
Write-Host ""
Write-Host "  Restart your terminal or reload your profile:" -ForegroundColor White
Write-Host "    . `$PROFILE" -ForegroundColor DarkGray
Write-Host ""
Write-Host "  Then install and activate a Go version:" -ForegroundColor White
Write-Host "    gvm install latest" -ForegroundColor Cyan
Write-Host "    gvm use latest" -ForegroundColor Cyan
Write-Host ""
Write-Host "  Run 'gvm doctor' to verify the setup." -ForegroundColor DarkGray
Write-Host ""
