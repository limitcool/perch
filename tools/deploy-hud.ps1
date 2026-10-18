# Build perch-core + perch and (re)deploy them to the fixed install directory.
#   <InstallDir>\perch.exe
#   <InstallDir>\@Resources\bin\perch-core.exe   (+ data\, weather\, wallpapers\, ... runtime tree)
# A running exe cannot be overwritten, so: build into target-dev, stop the HUD, copy, start it again.
param(
    [string]$InstallDir = $(if ($env:PERCH_HOME) { $env:PERCH_HOME } else { "$env:LOCALAPPDATA\perch" }),
    [switch]$NoStart,
    # Optional folder of extra theme packs (*.toml), copied to <InstallDir>\themes
    [string]$ThemePacks = ''
)
$ErrorActionPreference = 'Stop'
$root = Join-Path $PSScriptRoot '..'

# perch-core
Push-Location (Join-Path $root 'perch-core')
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'core build failed' }
} finally { Pop-Location }

# perch
Push-Location (Join-Path $root 'perch')
try {
    $env:CARGO_TARGET_DIR = 'target-dev'
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'hud build failed' }
} finally { Pop-Location; Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue }

Get-Process perch -ErrorAction SilentlyContinue | Stop-Process -Force
$deadline = (Get-Date).AddSeconds(4)
while ((Get-Process perch -ErrorAction SilentlyContinue) -and ((Get-Date) -lt $deadline)) {
    Start-Sleep -Milliseconds 200
}
New-Item -ItemType Directory -Force (Join-Path $InstallDir '@Resources\bin') | Out-Null
Copy-Item (Join-Path $root 'perch\target-dev\release\perch.exe') (Join-Path $InstallDir 'perch.exe') -Force
Copy-Item (Join-Path $root 'perch-core\target\release\perch-core.exe') (Join-Path $InstallDir '@Resources\bin\perch-core.exe') -Force
if ($ThemePacks) {
    $dst = Join-Path $InstallDir 'themes'
    New-Item -ItemType Directory -Force $dst | Out-Null
    Copy-Item (Join-Path $ThemePacks '*.toml') $dst -Force
    Write-Host "theme packs copied to $dst"
}
if (-not $NoStart) { Start-Process (Join-Path $InstallDir 'perch.exe') }
Write-Host "deployed to $InstallDir"
