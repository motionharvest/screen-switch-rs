# Stop and remove the scheduled task and installed files.
$ErrorActionPreference = "Stop"

$TaskName = "ScreenSwitchRs"
$InstallDir = Join-Path $env:LOCALAPPDATA "ScreenSwitchRs"

$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)
if (-not $isAdmin) {
    Start-Process -Verb RunAs -FilePath "powershell.exe" `
        -ArgumentList "-ExecutionPolicy Bypass -File `"$PSCommandPath`""
    exit
}

Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
Get-Process -Name "screen-switch-rs*" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300
Remove-Item -Recurse -Force $InstallDir -ErrorAction SilentlyContinue

Write-Host "Uninstalled '$TaskName' and removed $InstallDir." -ForegroundColor Green
