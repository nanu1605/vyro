# Bundled PawnIO installer

`PawnIO_setup.exe` (not committed) is the unmodified, signed installer from
[PawnIO.Setup 2.2.0](https://github.com/namazso/PawnIO.Setup/releases/tag/2.2.0), fetched and
hash-checked by [`scripts/fetch-pawnio.ps1`](../../scripts/fetch-pawnio.ps1). The MSI installs it
into `pawnio\` next to `vyro.exe` and runs it with `-install -silent` when PawnIO is missing or
older than 2.0 ([`../wix/vyro.wxs`](../wix/vyro.wxs)). Uninstalling Vyro leaves PawnIO in
place, since other tools (LibreHardwareMonitor, FanControl) may use it; it has its own entry in
Apps & features.

PawnIO driver: GPL-2.0 with an exception for programs that use it only through its IOCTLs.
Source: <https://github.com/namazso/PawnIO>.
