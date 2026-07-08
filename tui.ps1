# Build then open the config TUI. The background watcher keeps running; the TUI
# edits the shared config.json, which the watcher hot-reloads.

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

& "$PSScriptRoot\build.ps1"
if ($LASTEXITCODE -ne 0) { exit 1 }

$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "screen-switch-rs\target"
$tui = Join-Path $env:CARGO_TARGET_DIR "release\screen-switch-tui.exe"

Write-Host ""
Write-Host "Opening config TUI..." -ForegroundColor Cyan
& $tui
