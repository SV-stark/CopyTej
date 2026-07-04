# CopyTej Context Menu Registry Setup
# Run this script as Administrator to register CopyTej in the Windows Right-Click Context Menu.

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Warning "This script must be run as Administrator to modify system registry."
    Write-Host "Relaunching with administrative privileges..."
    Start-Process powershell -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`"" -Verb RunAs
    exit
}

# Determine executable path
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$releaseExe = Join-Path $scriptDir "src-tauri\target\release\copytej-app.exe"
$debugExe = Join-Path $scriptDir "src-tauri\target\debug\copytej-app.exe"
$exePath = ""

if (Test-Path $releaseExe) {
    $exePath = Resolve-Path $releaseExe
} elseif (Test-Path $debugExe) {
    $exePath = Resolve-Path $debugExe
} else {
    Write-Error "Could not find copytej-app.exe in target release or debug folders. Please compile the Tauri application first (npm run tauri build)."
    Read-Host "Press Enter to exit..."
    exit
}

Write-Host "Found CopyTej executable at: $exePath"
Write-Host "Registering context menu entries..."

# 1. Register for Files (*)
$registryPaths = @(
    "Registry::HKEY_CLASSES_ROOT\*\shell",
    "Registry::HKEY_CLASSES_ROOT\Directory\shell"
)

foreach ($rootPath in $registryPaths) {
    # Copy with CopyTej
    $copyKey = Join-Path $rootPath "CopyTejCopy"
    if (-not (Test-Path $copyKey)) { New-Item -Path $copyKey -Force | Out-Null }
    Set-ItemProperty -Path $copyKey -Name "(Default)" -Value "Copy with CopyTej" -Force
    Set-ItemProperty -Path $copyKey -Name "Icon" -Value "$exePath,0" -Force
    
    $copyCmd = Join-Path $copyKey "command"
    if (-not (Test-Path $copyCmd)) { New-Item -Path $copyCmd -Force | Out-Null }
    Set-ItemProperty -Path $copyCmd -Name "(Default)" -Value "`"$exePath`" `"%1`"" -Force

    # Move with CopyTej
    $moveKey = Join-Path $rootPath "CopyTejMove"
    if (-not (Test-Path $moveKey)) { New-Item -Path $moveKey -Force | Out-Null }
    Set-ItemProperty -Path $moveKey -Name "(Default)" -Value "Move with CopyTej" -Force
    Set-ItemProperty -Path $moveKey -Name "Icon" -Value "$exePath,0" -Force
    
    $moveCmd = Join-Path $moveKey "command"
    if (-not (Test-Path $moveCmd)) { New-Item -Path $moveCmd -Force | Out-Null }
    Set-ItemProperty -Path $moveCmd -Name "(Default)" -Value "`"$exePath`" -m `"%1`"" -Force
}

Write-Host "Successfully registered context menu bindings!"
Write-Host "You can now right-click any file or directory and copy/move it via CopyTej."
Write-Host "To unregister, run this script with '-Unregister' argument."

# Support Unregistration
if ($args -contains "-Unregister" -or $args -contains "/u") {
    Write-Host "Unregistering CopyTej..."
    foreach ($rootPath in $registryPaths) {
        $copyKey = Join-Path $rootPath "CopyTejCopy"
        if (Test-Path $copyKey) { Remove-Item -Path $copyKey -Recurse -Force }
        
        $moveKey = Join-Path $rootPath "CopyTejMove"
        if (Test-Path $moveKey) { Remove-Item -Path $moveKey -Recurse -Force }
    }
    Write-Host "Successfully unregistered context menu bindings!"
}

Read-Host "Press Enter to exit..."
