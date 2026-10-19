# Sets the CI runner's desktop to the largest size it offers, up to 2560×1440, so the screen
# checks capture a monitor as large as a real one (the runner starts at 1024×768). Prints the
# sizes on offer and the one in use. Never fails the job: the checks then run at the old size.
# -Reset puts back the size the runner started with.
#
#   pwsh scripts/ci-display.ps1 [-Reset]
param([switch]$Reset)
$ErrorActionPreference = 'Continue'

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class VyroDisplay {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct DEVMODE {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
        public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra;
        public int dmFields, dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
        public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
        public short dmLogPixels;
        public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
        public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2;
        public int dmPanningWidth, dmPanningHeight;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern bool EnumDisplaySettings(string device, int mode, ref DEVMODE dm);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int ChangeDisplaySettings(ref DEVMODE dm, int flags);
    // No mode: back to the size stored for the display.
    [DllImport("user32.dll", EntryPoint = "ChangeDisplaySettingsW")]
    public static extern int ChangeDisplaySettingsNone(IntPtr dm, int flags);
    public static DEVMODE Blank() {
        var dm = new DEVMODE();
        dm.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
        return dm;
    }
}
'@

if ($Reset) {
    $r = [VyroDisplay]::ChangeDisplaySettingsNone([IntPtr]::Zero, 0)
    Write-Host "Back to the stored size: result $r (0 is done)"
    exit 0
}

$modes = @()
for ($i = 0; ; $i++) {
    $dm = [VyroDisplay]::Blank()
    if (-not [VyroDisplay]::EnumDisplaySettings([NullString]::Value, $i, [ref]$dm)) { break }
    if ($dm.dmBitsPerPel -eq 32) { $modes += $dm }
}
$sizes = $modes | ForEach-Object { "$($_.dmPelsWidth)x$($_.dmPelsHeight)" } | Sort-Object -Unique
Write-Host "Sizes on offer: $($sizes -join ', ')"

$best = $modes |
    Where-Object { $_.dmPelsWidth -le 2560 -and $_.dmPelsHeight -le 1440 } |
    Sort-Object { $_.dmPelsWidth * $_.dmPelsHeight } -Descending |
    Select-Object -First 1
if ($best) {
    # 0: change now, for this session only.
    $r = [VyroDisplay]::ChangeDisplaySettings([ref]$best, 0)
    Write-Host "Asked for $($best.dmPelsWidth)x$($best.dmPelsHeight): result $r (0 is done)"
}

$now = [VyroDisplay]::Blank()
# -1: the size in use now.
[void][VyroDisplay]::EnumDisplaySettings([NullString]::Value, -1, [ref]$now)
Write-Host "::notice title=Desktop size::$($now.dmPelsWidth)x$($now.dmPelsHeight) (on offer: $($sizes -join ', '))"
exit 0
