# Downloads the official, signed PawnIO installer that the MSI bundles and runs with
# `-install -silent` (see src-tauri/wix/pawnio.wxs). The file is not committed; the pinned
# SHA-256 below guarantees every build ships exactly this release.
#
#   pwsh scripts/fetch-pawnio.ps1
$ErrorActionPreference = 'Stop'

$Version = '2.2.0'
$Url = "https://github.com/namazso/PawnIO.Setup/releases/download/$Version/PawnIO_setup.exe"
$Sha256 = '1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032'

$Dest = Join-Path $PSScriptRoot '..\src-tauri\pawnio\PawnIO_setup.exe'
New-Item -ItemType Directory -Force (Split-Path $Dest) | Out-Null

if (-not (Test-Path $Dest) -or (Get-FileHash $Dest -Algorithm SHA256).Hash -ne $Sha256.ToUpper()) {
    Write-Host "Downloading PawnIO $Version"
    Invoke-WebRequest -Uri $Url -OutFile $Dest
}
$Actual = (Get-FileHash $Dest -Algorithm SHA256).Hash
if ($Actual -ne $Sha256.ToUpper()) {
    Remove-Item $Dest
    throw "PawnIO_setup.exe hash mismatch: expected $Sha256, got $Actual"
}
Write-Host "PawnIO $Version ready: $Dest"
