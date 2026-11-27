# Checks that OpenRGB's installer pinned in crates/clokrix-core/src/lighting/setup.rs (2.6.1)
# still downloads from its URL with its size and SHA-256: the Lighting view's Set up OpenRGB
# downloads exactly that file on the owner's PC, so a release must never point at a moved or
# changed one. Nothing is installed here; CI's Windows job also installs it (the openrgb_setup
# example).
#
#   pwsh scripts/check-openrgb.ps1
$ErrorActionPreference = 'Stop'

$Source = Get-Content -Raw (Join-Path $PSScriptRoot '..\crates\clokrix-core\src\lighting\setup.rs')
function Pinned($name, $pattern) {
    $m = [regex]::Match($Source, "pub const ${name}: [^=]+=\s*$pattern;")
    if (-not $m.Success) { throw "setup.rs has no $name" }
    $m.Groups[1].Value
}
$Url = Pinned 'URL' '"([^"]+)"'
$Sha256 = Pinned 'SHA256' '"([0-9a-f]{64})"'
$Size = [int64]((Pinned 'SIZE' '([0-9_]+)') -replace '_', '')
$Version = Pinned 'VERSION' '"([^"]+)"'

$Dest = Join-Path ([IO.Path]::GetTempPath()) 'clokrix-openrgb-check.msi'
Invoke-WebRequest -Uri $Url -OutFile $Dest
$Length = (Get-Item $Dest).Length
$Actual = (Get-FileHash $Dest -Algorithm SHA256).Hash
Remove-Item $Dest
if ($Length -ne $Size) { throw "OpenRGB's installer is $Length bytes, not the pinned ${Size}: $Url" }
if ($Actual -ne $Sha256.ToUpper()) { throw "OpenRGB's installer hash changed: expected $Sha256, got $Actual ($Url)" }
Write-Host "OpenRGB $Version as pinned: $Length bytes, SHA-256 $Sha256"
