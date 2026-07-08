# Build screen-switch-rs with the Windows (MSVC) Rust toolchain.
# Target dir is a local Windows path for speed (avoids slow UNC writes).

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA "screen-switch-rs\target"

# Stop any running instance so the exe isn't locked during the build.
Get-Process -Name "screen-switch-rs*" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

Write-Host "Building (release)..." -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "Build failed." -ForegroundColor Red
    exit 1
}

$exe = Join-Path $env:CARGO_TARGET_DIR "release\screen-switch-rs.exe"
Write-Host ""
Write-Host "Build complete: $exe" -ForegroundColor Green
if (Test-Path $exe) {
    $size = [math]::Round((Get-Item $exe).Length / 1KB, 1)
    Write-Host "Size: $size KB" -ForegroundColor Gray
}
