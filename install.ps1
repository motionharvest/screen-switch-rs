# Install screen-switch-rs to a fixed location and run it at logon, elevated,
# with no UAC prompt (via a Scheduled Task set to highest privileges).
$ErrorActionPreference = "Stop"

$TaskName = "ScreenSwitchRs"
$InstallDir = Join-Path $env:LOCALAPPDATA "ScreenSwitchRs"
$BuiltExe = Join-Path $env:LOCALAPPDATA "screen-switch-rs\target\release\screen-switch-rs.exe"
$InstalledExe = Join-Path $InstallDir "screen-switch-rs.exe"

if (-not (Test-Path $BuiltExe)) {
    Write-Host "Built exe not found. Run build.ps1 first: $BuiltExe" -ForegroundColor Red
    exit 1
}

# Re-launch elevated if needed.
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "Elevating..." -ForegroundColor Cyan
    Start-Process -Verb RunAs -FilePath "powershell.exe" `
        -ArgumentList "-ExecutionPolicy Bypass -File `"$PSCommandPath`""
    exit
}

# Stop any running instances (incl. old copies) before overwriting.
Get-Process -Name "screen-switch-rs*" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

# Preserve existing config; copy the exe into the fixed install location.
Copy-Item -Force $BuiltExe $InstalledExe

$action = New-ScheduledTaskAction -Execute $InstalledExe -WorkingDirectory $InstallDir
$trigger = New-ScheduledTaskTrigger -AtLogOn
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit ([TimeSpan]::Zero)

Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger `
    -Principal $principal -Settings $settings -Force | Out-Null

Start-ScheduledTask -TaskName $TaskName

Write-Host "Installed and started '$TaskName'." -ForegroundColor Green
Write-Host "Exe:    $InstalledExe" -ForegroundColor Gray
Write-Host "Config: $(Join-Path $env:LOCALAPPDATA 'ScreenSwitchRs\config.json')" -ForegroundColor Gray
Write-Host "Log:    $(Join-Path $env:LOCALAPPDATA 'ScreenSwitchRs\log.txt')" -ForegroundColor Gray
Write-Host "It will launch automatically at logon (elevated, no UAC prompt)." -ForegroundColor Gray
