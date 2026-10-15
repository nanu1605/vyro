//! System information (Phase 1): OS, uptime, security posture, startup apps, power history.
//! Decoders are platform-neutral and fixture-tested; Windows collection lives in `win`.

pub mod events;
pub mod os;
pub mod startup;
#[cfg(windows)]
mod win;

use crate::info::SystemInfo;

/// How many power events to keep.
pub const POWER_EVENT_LIMIT: usize = 30;

/// Unexpected shutdowns and hardware errors since `since` from the System event log (the
/// Performance Score's Stability); `None` where it cannot be read.
pub fn stability_counts(since: i64) -> Option<crate::score::StabilityCounts> {
    #[cfg(windows)]
    return win::stability_counts(since);
    #[cfg(not(windows))]
    {
        let _ = since;
        None
    }
}

/// Collects system information. Slow-ish (event log, Task Scheduler, WMI): call once, off the
/// UI thread. `hypervisor_present` comes from CPUID (see `hwid`).
pub fn collect(hypervisor_present: Option<bool>) -> SystemInfo {
    let mut info = SystemInfo::default();
    info.os.architecture = Some(std::env::consts::ARCH.to_owned());

    #[cfg(windows)]
    {
        win::init_com();
        let now = crate::unix_now();
        info.os = win::os_info();
        info.boot_time = win::boot_time(now);
        info.elevated = win::elevated();
        info.security = win::security(hypervisor_present, info.elevated == Some(true));
        info.processes = win::processes();
        info.startup_apps = win::startup_apps();
        info.power_events = win::power_events(POWER_EVENT_LIMIT);
    }
    #[cfg(not(windows))]
    {
        info.os.name = Some(std::env::consts::OS.to_owned());
        info.security.hypervisor_present = hypervisor_present;
    }
    info
}

/// BitLocker protection of the system drive. Needs administrator rights, so the sensor service
/// answers it for the unelevated UI; `None` when unknown.
pub fn bitlocker_system_drive() -> Option<bool> {
    #[cfg(windows)]
    {
        win::init_com();
        win::bitlocker_system_drive()
    }
    #[cfg(not(windows))]
    None
}

/// Fills the time-dependent fields for a response produced at `now`.
pub fn with_uptime(mut info: SystemInfo, now: i64) -> SystemInfo {
    info.uptime_secs = info.boot_time.map(|b| (now - b).max(0));
    #[cfg(windows)]
    {
        // Process counts are cheap and change constantly; refresh them per request.
        info.processes = win::processes();
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_is_derived_from_boot_time() {
        let info = SystemInfo {
            boot_time: Some(1_000),
            ..Default::default()
        };
        assert_eq!(with_uptime(info, 4_600).uptime_secs, Some(3_600));
        assert_eq!(with_uptime(SystemInfo::default(), 10).uptime_secs, None);
    }

    #[test]
    fn collect_runs_on_any_host() {
        let info = collect(Some(false));
        assert!(info.os.architecture.is_some());
    }

    /// On a real Windows machine (CI), the collectors must produce the basics.
    #[cfg(windows)]
    #[test]
    fn windows_system_info_is_populated() {
        let info = with_uptime(collect(None), crate::unix_now());
        assert!(info
            .os
            .name
            .as_deref()
            .is_some_and(|n| n.starts_with("Windows")));
        assert!(info.os.build.is_some());
        assert_eq!(info.os.architecture.as_deref(), Some("x64"));
        assert!(info.uptime_secs.unwrap() > 0);
        assert!(info.elevated.is_some());
        assert!(info.processes.processes.unwrap() > 0);
        assert!(info.security.uefi.is_some());
    }
}
