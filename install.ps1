# arch installer for Windows (PowerShell)
# Usage: irm https://raw.githubusercontent.com/micsh/arch/main/install.ps1 | iex
#   or:  .\install.ps1 [-InstallDir C:\tools]

param(
    [string]$InstallDir = "$env:USERPROFILE\.arch\bin"
)

$ErrorActionPreference = "Stop"
$repo = "micsh/arch"
$asset = "arch-windows-x64.exe"

Write-Host "🏗️  Installing arch..." -ForegroundColor Cyan

# Get latest release URL
$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
$url = ($release.assets | Where-Object { $_.name -eq $asset }).browser_download_url

if (-not $url) {
    Write-Host "❌ Could not find $asset in latest release" -ForegroundColor Red
    exit 1
}

Write-Host "   Downloading $($release.tag_name) from GitHub..."

# Create install directory
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

$dest = Join-Path $InstallDir "arch.exe"
Invoke-WebRequest -Uri $url -OutFile $dest -UseBasicParsing

# Add to PATH if not already there
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$InstallDir", "User")
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "   Added $InstallDir to user PATH" -ForegroundColor Green
} else {
    Write-Host "   $InstallDir already in PATH" -ForegroundColor DarkGray
}

# Verify
$version = & $dest --version 2>&1
Write-Host "✅ Installed $version to $dest" -ForegroundColor Green
Write-Host ""
Write-Host "   Restart your terminal, then run: arch --help" -ForegroundColor Yellow
