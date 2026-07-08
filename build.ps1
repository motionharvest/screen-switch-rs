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
$tui = Join-Path $env:CARGO_TARGET_DIR "release\screen-switch-tui.exe"
Write-Host ""
Write-Host "Build complete:" -ForegroundColor Green
foreach ($f in @($exe, $tui)) {
    if (Test-Path $f) {
        $size = [math]::Round((Get-Item $f).Length / 1KB, 1)
        Write-Host ("  {0}  ({1} KB)" -f $f, $size) -ForegroundColor Gray
    }
}
