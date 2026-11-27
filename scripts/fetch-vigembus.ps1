# Downloads the official, signed ViGEmBus installer (Nefarius Software Solutions e.U., BSD-3)
# that the MSI carries for the phone wheel (2.6). It is not run at install: the app runs it, with
# one administrator prompt, the first time the owner sets the wheel up (src-tauri/src/wheel.rs).
# The file is not committed; the pinned SHA-256 below guarantees every build ships exactly this
# release.
#
#   pwsh scripts/fetch-vigembus.ps1
$ErrorActionPreference = 'Stop'

$Version = '1.22.0'
$Url = "https://github.com/nefarius/ViGEmBus/releases/download/v$Version/ViGEmBus_${Version}_x64_x86_arm64.exe"
$Sha256 = '89220a7865076b342892f98865f3499fb7c4cfd673159e89d352c360fd014c6a'

$Dest = Join-Path $PSScriptRoot '..\src-tauri\vigembus\ViGEmBus_setup.exe'
New-Item -ItemType Directory -Force (Split-Path $Dest) | Out-Null

if (-not (Test-Path $Dest) -or (Get-FileHash $Dest -Algorithm SHA256).Hash -ne $Sha256.ToUpper()) {
    Write-Host "Downloading ViGEmBus $Version"
    Invoke-WebRequest -Uri $Url -OutFile $Dest
}
$Actual = (Get-FileHash $Dest -Algorithm SHA256).Hash
if ($Actual -ne $Sha256.ToUpper()) {
    Remove-Item $Dest
    throw "ViGEmBus_setup.exe hash mismatch: expected $Sha256, got $Actual"
}
Write-Host "ViGEmBus $Version ready: $Dest"
