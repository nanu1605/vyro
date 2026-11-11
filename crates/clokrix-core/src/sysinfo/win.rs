//! Windows system information: registry, firmware, TPM, event log, Task Scheduler, WMI.

use std::path::Path;

use windows::core::{Interface, BSTR, HSTRING};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::EventLog::{
    EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender,
    EvtRenderEventXml, EVT_HANDLE,
};
use windows::Win32::System::ProcessStatus::{K32GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{
    FirmwareTypeUefi, GetFirmwareType, GetNativeSystemInfo, GetTickCount64, FIRMWARE_TYPE,
    SYSTEM_INFO,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, IRegisteredTask, ITaskFolder, ITaskService, TaskScheduler, TASK_ACTION_EXEC,
    TASK_ACTION_TYPE, TASK_ENUM_HIDDEN, TASK_TRIGGER_BOOT, TASK_TRIGGER_LOGON, TASK_TRIGGER_TYPE2,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, IsProcessorFeaturePresent, OpenProcessToken, PF_VIRT_FIRMWARE_ENABLED,
};
use windows::Win32::System::TpmBaseServices::{Tbsi_GetDeviceInfo, TBS_SUCCESS, TPM_DEVICE_INFO};
use windows::Win32::System::Variant::VARIANT;

use super::{events, os, startup};
use crate::info::{OsInfo, PowerEvent, ProcessOverview, SecurityInfo, StartupApp, StartupSource};
use crate::winutil::{Key, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

/// Initializes COM (MTA) on the calling thread for Task Scheduler and WMI. Idempotent; a thread
/// already in another apartment keeps it.
pub fn init_com() {
    // SAFETY: plain COM initialization; the result is intentionally ignored (S_FALSE and
    // RPC_E_CHANGED_MODE are both fine here).
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

pub fn os_info() -> OsInfo {
    let mut info = OsInfo::default();
    if let Some(k) = Key::open(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
    ) {
        let build = k
            .string("CurrentBuildNumber")
            .or_else(|| k.string("CurrentBuild"));
        let build_no = build.as_deref().and_then(|b| b.parse().ok()).unwrap_or(0);
        info.name = k.string("ProductName").map(|p| os::os_name(&p, build_no));
        info.edition = k.string("EditionID");
        info.display_version = k.string("DisplayVersion").or_else(|| k.string("ReleaseId"));
        info.build = build.map(|b| os::build_string(&b, k.dword("UBR")));
        info.install_date = k.dword("InstallDate").filter(|&d| d > 0).map(i64::from);
    }
    let mut si = SYSTEM_INFO::default();
    // SAFETY: valid out-pointer.
    unsafe { GetNativeSystemInfo(&mut si) };
    // SAFETY: the union's architecture view is always initialized by GetNativeSystemInfo.
    let arch = unsafe { si.Anonymous.Anonymous.wProcessorArchitecture.0 };
    info.architecture = os::architecture_name(arch).map(str::to_owned);
    info
}

/// Boot time as Unix seconds, given the current Unix time.
pub fn boot_time(now: i64) -> Option<i64> {
    // SAFETY: no arguments.
    let ms = unsafe { GetTickCount64() };
    Some(now - i64::try_from(ms / 1000).ok()?)
}

pub fn elevated() -> Option<bool> {
    let mut token = HANDLE::default();
    // SAFETY: pseudo-handle for the current process; valid out-pointer.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.ok()?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut len = 0u32;
    // SAFETY: buffer is a TOKEN_ELEVATION of the declared size.
    let res = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some(std::ptr::from_mut(&mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
    };
    // SAFETY: token was opened above and is closed once.
    unsafe {
        let _ = CloseHandle(token);
    }
    res.ok()?;
    Some(elevation.TokenIsElevated != 0)
}

const TBS_E_TPM_NOT_FOUND: u32 = 0x8028_400F;

pub fn security(hypervisor_present: Option<bool>, elevated: bool) -> SecurityInfo {
    let mut s = SecurityInfo::default();

    let mut fw = FIRMWARE_TYPE::default();
    // SAFETY: valid out-pointer.
    if unsafe { GetFirmwareType(&mut fw) }.is_ok() {
        s.uefi = Some(fw == FirmwareTypeUefi);
    }
    s.secure_boot = Key::open(
        HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Control\SecureBoot\State",
    )
    .and_then(|k| k.dword("UEFISecureBootEnabled"))
    .map(|v| v != 0)
    .or(if s.uefi == Some(false) {
        Some(false)
    } else {
        None
    });

    let mut tpm = TPM_DEVICE_INFO {
        structVersion: 1,
        ..Default::default()
    };
    // SAFETY: `tpm` is a TPM_DEVICE_INFO of the declared size.
    let rc = unsafe {
        Tbsi_GetDeviceInfo(
            size_of::<TPM_DEVICE_INFO>() as u32,
            std::ptr::from_mut(&mut tpm).cast(),
        )
    };
    if rc == TBS_SUCCESS {
        s.tpm_present = Some(true);
        s.tpm_version = match tpm.tpmVersion {
            1 => Some("1.2".to_owned()),
            2 => Some("2.0".to_owned()),
            _ => None,
        };
    } else if rc == TBS_E_TPM_NOT_FOUND {
        s.tpm_present = Some(false);
    }

    s.hypervisor_present = hypervisor_present;
    // SAFETY: no pointers.
    let firmware_vt = unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) }.as_bool();
    // With a hypervisor running, Windows hides VT-x from itself; it is necessarily enabled.
    s.virtualization_enabled = Some(firmware_vt || hypervisor_present == Some(true));

    if elevated {
        s.bitlocker_system_drive = bitlocker_system_drive();
    }
    s
}

#[derive(serde::Deserialize)]
#[serde(rename = "Win32_EncryptableVolume", rename_all = "PascalCase")]
struct EncryptableVolume {
    drive_letter: Option<String>,
    protection_status: Option<u32>,
}

/// Requires elevation; `None` when WMI refuses or the volume is unknown.
pub fn bitlocker_system_drive() -> Option<bool> {
    let con =
        wmi::WMIConnection::with_namespace_path(r"ROOT\CIMV2\Security\MicrosoftVolumeEncryption")
            .map_err(|e| log::info!("bitlocker: WMI unavailable: {e}"))
            .ok()?;
    let vols: Vec<EncryptableVolume> = con.query().ok()?;
    let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let vol = vols.iter().find(|v| {
        v.drive_letter
            .as_deref()
            .is_some_and(|d| d.eq_ignore_ascii_case(&sys))
    })?;
    match vol.protection_status? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

pub fn processes() -> ProcessOverview {
    let mut pi = PERFORMANCE_INFORMATION::default();
    // SAFETY: `pi` is a PERFORMANCE_INFORMATION of the declared size.
    let ok = unsafe { K32GetPerformanceInfo(&mut pi, size_of::<PERFORMANCE_INFORMATION>() as u32) }
        .as_bool();
    if !ok {
        return ProcessOverview::default();
    }
    ProcessOverview {
        processes: Some(pi.ProcessCount),
        threads: Some(pi.ThreadCount),
        handles: Some(pi.HandleCount),
    }
}

// ---------------------------------------------------------------------------------------------
// Startup apps
// ---------------------------------------------------------------------------------------------

const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

pub fn startup_apps() -> Vec<StartupApp> {
    let mut out = Vec::new();
    let run_keys = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
            "Run",
            StartupSource::RegistryMachine,
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
            "Run32",
            StartupSource::RegistryMachine,
        ),
        (
            HKEY_CURRENT_USER,
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            "Run",
            StartupSource::RegistryUser,
        ),
    ];
    for (root, path, approved_sub, source) in run_keys {
        let Some(key) = Key::open(root, path) else {
            continue;
        };
        let approved = Key::open(root, &format!(r"{APPROVED}\{approved_sub}"));
        for (name, value) in key.values() {
            let Some(command) = value.as_string() else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let flag = approved
                .as_ref()
                .and_then(|a| a.value(&name))
                .map(|v| v.data);
            out.push(StartupApp {
                enabled: startup::approved_enabled(flag.as_deref()),
                name,
                command,
                source,
            });
        }
    }

    let folders = [
        (
            std::env::var("APPDATA").ok(),
            HKEY_CURRENT_USER,
            StartupSource::FolderUser,
        ),
        (
            std::env::var("ProgramData").ok(),
            HKEY_LOCAL_MACHINE,
            StartupSource::FolderMachine,
        ),
    ];
    for (base, root, source) in folders {
        let Some(base) = base else { continue };
        let dir = Path::new(&base).join(r"Microsoft\Windows\Start Menu\Programs\Startup");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let approved = Key::open(root, &format!(r"{APPROVED}\StartupFolder"));
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if file.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let flag = approved
                .as_ref()
                .and_then(|a| a.value(&file))
                .map(|v| v.data);
            out.push(StartupApp {
                name: startup::entry_name(&file),
                command: entry.path().display().to_string(),
                source,
                enabled: startup::approved_enabled(flag.as_deref()),
            });
        }
    }

    match scheduled_startup_tasks() {
        Ok(tasks) => out.extend(tasks),
        Err(e) => log::info!("startup: Task Scheduler unavailable: {e}"),
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Non-Microsoft tasks with a logon or boot trigger.
fn scheduled_startup_tasks() -> windows::core::Result<Vec<StartupApp>> {
    // SAFETY: COM calls on interfaces obtained from the Task Scheduler service; all out-values
    // are owned wrappers.
    unsafe {
        let svc: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        let empty = VARIANT::default();
        svc.Connect(&empty, &empty, &empty, &empty)?;
        let mut stack: Vec<ITaskFolder> = vec![svc.GetFolder(&BSTR::from("\\"))?];
        let mut out = Vec::new();
        while let Some(folder) = stack.pop() {
            if let Ok(subs) = folder.GetFolders(0) {
                for i in 1..=subs.Count().unwrap_or(0) {
                    let Ok(sub) = subs.get_Item(&VARIANT::from(i)) else {
                        continue;
                    };
                    // Microsoft's own maintenance tasks are not "startup apps".
                    let is_microsoft = sub
                        .Path()
                        .is_ok_and(|p| p.to_string().eq_ignore_ascii_case(r"\Microsoft"));
                    if !is_microsoft {
                        stack.push(sub);
                    }
                }
            }
            let Ok(tasks) = folder.GetTasks(TASK_ENUM_HIDDEN.0) else {
                continue;
            };
            for i in 1..=tasks.Count().unwrap_or(0) {
                if let Ok(task) = tasks.get_Item(&VARIANT::from(i)) {
                    out.extend(startup_task(&task));
                }
            }
        }
        Ok(out)
    }
}

/// # Safety
/// `task` must be a live Task Scheduler interface on a COM-initialized thread.
unsafe fn startup_task(task: &IRegisteredTask) -> Option<StartupApp> {
    // SAFETY: see function contract.
    unsafe {
        let def = task.Definition().ok()?;
        let triggers = def.Triggers().ok()?;
        let mut n = 0;
        triggers.Count(&mut n).ok()?;
        let at_startup = (1..=n).any(|i| {
            let mut kind = TASK_TRIGGER_TYPE2::default();
            triggers
                .get_Item(i)
                .and_then(|t| t.Type(&mut kind))
                .is_ok_and(|()| kind == TASK_TRIGGER_LOGON || kind == TASK_TRIGGER_BOOT)
        });
        if !at_startup {
            return None;
        }
        let actions = def.Actions().ok()?;
        let mut count = 0;
        actions.Count(&mut count).ok()?;
        let mut command = String::new();
        for i in 1..=count {
            let Ok(action) = actions.get_Item(i) else {
                continue;
            };
            let mut kind = TASK_ACTION_TYPE::default();
            if action.Type(&mut kind).is_err() || kind != TASK_ACTION_EXEC {
                continue;
            }
            let Ok(exec) = action.cast::<IExecAction>() else {
                continue;
            };
            let (mut path, mut args) = (BSTR::new(), BSTR::new());
            let _ = exec.Path(&mut path);
            let _ = exec.Arguments(&mut args);
            command = format!("{path} {args}").trim().to_owned();
            break;
        }
        Some(StartupApp {
            name: task.Name().ok()?.to_string(),
            command,
            source: StartupSource::ScheduledTask,
            enabled: task.Enabled().ok()?.as_bool(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Power events
// ---------------------------------------------------------------------------------------------

/// Newest-first power events from the System log (no elevation needed).
pub fn power_events(limit: usize) -> Vec<PowerEvent> {
    let mut out = Vec::new();
    system_log(events::QUERY, limit, |xml| {
        if let Some(event) = events::parse_event(xml) {
            out.push(event);
        }
    });
    out.truncate(limit);
    out
}

/// The active power plan's friendly name.
pub fn power_plan() -> Option<String> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::System::Power::{PowerGetActiveScheme, PowerReadFriendlyName};
    let mut guid: *mut windows::core::GUID = std::ptr::null_mut();
    // SAFETY: `guid` receives a LocalAlloc'd GUID, freed below.
    if unsafe { PowerGetActiveScheme(None, &mut guid) }.is_err() || guid.is_null() {
        return None;
    }
    let mut size = 0u32;
    // SAFETY: the first call asks for the size; the second fills a buffer of that size.
    let name = unsafe {
        let _ = PowerReadFriendlyName(None, Some(guid), None, None, None, &mut size);
        let mut buf = vec![0u8; size as usize];
        let ok = size > 0
            && PowerReadFriendlyName(
                None,
                Some(guid),
                None,
                None,
                Some(buf.as_mut_ptr()),
                &mut size,
            )
            .is_ok();
        LocalFree(Some(HLOCAL(guid.cast())));
        ok.then(|| {
            let wide: Vec<u16> = buf
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .take_while(|c| *c != 0)
                .collect();
            String::from_utf16_lossy(&wide)
        })
    };
    name.filter(|n| !n.trim().is_empty())
}

/// Unexpected shutdowns and hardware errors since `since` (the Performance Score's
/// Stability); `None` when the System log cannot be read.
pub fn stability_counts(since: i64) -> Option<crate::score::StabilityCounts> {
    let mut found = Vec::new();
    let query = events::stability_query(crate::unix_now() - since);
    let ok = system_log(&query, 10_000, |xml| {
        found.extend(events::parse_stability(xml));
    });
    ok.then(|| events::count_stability(found, since))
}

/// Unexpected shutdowns, display driver resets and GPU hangs since `since` (Unix seconds), for
/// GPU tuning's crash guard (1.6).
pub fn gpu_crashes(since: i64) -> Vec<(i64, crate::gpu_tune::CrashKind)> {
    let secs = crate::unix_now() - since;
    let mut found = Vec::new();
    system_log(&events::gpu_crash_query(secs), 200, |xml| {
        found.extend(events::parse_gpu_crash(xml));
    });
    event_log(
        "Application",
        &events::error_report_query(secs),
        200,
        |xml| {
            found.extend(events::parse_gpu_crash(xml));
        },
    );
    found.retain(|(ts, _)| *ts >= since);
    found.sort_by_key(|(ts, _)| *ts);
    found
}

/// Renders up to `limit` events of the System log matching `xpath`, newest first. False when
/// the log cannot be queried.
fn system_log(xpath: &str, limit: usize, each: impl FnMut(&str)) -> bool {
    event_log("System", xpath, limit, each)
}

/// As [`system_log`], for any log.
fn event_log(channel: &str, xpath: &str, limit: usize, mut each: impl FnMut(&str)) -> bool {
    let flags = EvtQueryChannelPath.0 | EvtQueryReverseDirection.0;
    // SAFETY: strings outlive the call; the returned handle is closed below.
    let Ok(query) =
        (unsafe { EvtQuery(None, &HSTRING::from(channel), &HSTRING::from(xpath), flags) })
    else {
        return false;
    };
    let mut seen = 0usize;
    while seen < limit {
        let mut handles = [0isize; 16];
        let mut returned = 0u32;
        // SAFETY: `handles` receives up to 16 event handles, each closed below.
        if unsafe { EvtNext(query, &mut handles, 2000, 0, &mut returned) }.is_err() {
            break;
        }
        for &h in handles.iter().take(returned as usize) {
            let h = EVT_HANDLE(h);
            if seen < limit {
                if let Some(xml) = render_xml(h) {
                    each(&xml);
                }
                seen += 1;
            }
            // SAFETY: handle from EvtNext, closed once.
            let _ = unsafe { EvtClose(h) };
        }
    }
    // SAFETY: query handle closed once.
    let _ = unsafe { EvtClose(query) };
    true
}

fn render_xml(event: EVT_HANDLE) -> Option<String> {
    let (mut used, mut props) = (0u32, 0u32);
    // SAFETY: size query with no buffer.
    let _ = unsafe {
        EvtRender(
            None,
            event,
            EvtRenderEventXml.0,
            0,
            None,
            &mut used,
            &mut props,
        )
    };
    if used == 0 {
        return None;
    }
    let mut buf = vec![0u16; (used as usize).div_ceil(2)];
    // SAFETY: `buf` holds `used` bytes.
    unsafe {
        EvtRender(
            None,
            event,
            EvtRenderEventXml.0,
            used,
            Some(buf.as_mut_ptr().cast()),
            &mut used,
            &mut props,
        )
    }
    .ok()?;
    Some(
        String::from_utf16_lossy(&buf)
            .trim_end_matches('\0')
            .to_owned(),
    )
}
