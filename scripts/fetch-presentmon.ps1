# Downloads the official, Intel-signed PresentMon console app that the MSI installs next to
# vyro-service.exe (see src-tauri/wix/vyro.wxs); the service runs it to measure game frame
# rates. The file is not committed; the pinned SHA-256 below guarantees every build ships
# exactly this release.
#
#   pwsh scripts/fetch-presentmon.ps1
$ErrorActionPreference = 'Stop'

$Version = '2.6.0'
$Url = "https://github.com/GameTechDev/PresentMon/releases/download/v$Version/PresentMon-$Version-x64.exe"
$Sha256 = 'b2a706bc6ad475749e3b7e3409263aa1e6906d45bdcf993f6dbc0f660188f1af'

$Dest = Join-Path $PSScriptRoot '..\src-tauri\presentmon\PresentMon.exe'
New-Item -ItemType Directory -Force (Split-Path $Dest) | Out-Null

if (-not (Test-Path $Dest) -or (Get-FileHash $Dest -Algorithm SHA256).Hash -ne $Sha256.ToUpper()) {
    Write-Host "Downloading PresentMon $Version"
    Invoke-WebRequest -Uri $Url -OutFile $Dest
}
$Actual = (Get-FileHash $Dest -Algorithm SHA256).Hash
if ($Actual -ne $Sha256.ToUpper()) {
    Remove-Item $Dest
    throw "PresentMon.exe hash mismatch: expected $Sha256, got $Actual"
}
Write-Host "PresentMon $Version ready: $Dest"
