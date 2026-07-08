# Build then run screen-switch-rs in this console so you can watch the logs.
# Runs elevated so simulated keys can reach elevated target apps.

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

& "$PSScriptRoot\build.ps1"
if ($LASTEXITCODE -ne 0) { exit 1 }

$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "screen-switch-rs\target"
$exe = Join-Path $env:CARGO_TARGET_DIR "release\screen-switch-rs.exe"

Write-Host ""
Write-Host "Launching (elevated, new window)..." -ForegroundColor Cyan
Start-Process -Verb RunAs $exe
