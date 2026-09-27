//! Service Control Manager glue and the service's data folder.

use std::ffi::c_void;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};

use windows::core::{w, HSTRING, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_CALL_NOT_IMPLEMENTED, ERROR_SERVICE_SPECIFIC_ERROR,
    ERROR_SUCCESS, HLOCAL, NO_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW, SetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    GetSecurityDescriptorDacl, IsWellKnownSid, WinBuiltinAdministratorsSid, WinLocalSystemSid, ACL,
    DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
};
use windows::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
};
use windows::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SetServiceStatus, StartServiceCtrlDispatcherW,
    SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP, SERVICE_CONTROL_INTERROGATE,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_START_PENDING,
    SERVICE_STATUS, SERVICE_STATUS_CURRENT_STATE, SERVICE_STATUS_HANDLE, SERVICE_STOPPED,
    SERVICE_STOP_PENDING, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
};

use vyro_core::lock;

/// Protected DACL for the data folder and everything created in it: SYSTEM and
/// administrators full control, standard users read (logs for bug reports), nobody else.
const DATA_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FRFX;;;BU)";

/// `SERVICE_STATUS_HANDLE` as an integer, so it can live in a static.
static STATUS: AtomicUsize = AtomicUsize::new(0);
/// Signals `run` to stop.
static STOP: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);

/// Hands the process to the Service Control Manager; returns when the service has stopped.
pub fn dispatch() {
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR(w!("VyroService").as_ptr().cast_mut()),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    // SAFETY: a null-terminated table of valid entries that outlives the call.
    if let Err(e) = unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } {
        eprintln!(
            "vyro-service: not started by the Service Control Manager ({e}).\n\
             To debug, run `vyro-service --console` as administrator."
        );
        std::process::exit(1);
    }
}

fn report(state: SERVICE_STATUS_CURRENT_STATE, exit_code: u32, wait_hint_ms: u32) {
    let handle = STATUS.load(Ordering::SeqCst);
    if handle == 0 {
        return;
    }
    let accepted = if state == SERVICE_RUNNING {
        SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
    } else {
        0
    };
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accepted,
        dwWin32ExitCode: if exit_code == 0 {
            NO_ERROR.0
        } else {
            ERROR_SERVICE_SPECIFIC_ERROR.0
        },
        dwServiceSpecificExitCode: exit_code,
        dwCheckPoint: 0,
        dwWaitHint: wait_hint_ms,
    };
    // SAFETY: the handle came from RegisterServiceCtrlHandlerExW; `status` is valid.
    let _ = unsafe { SetServiceStatus(SERVICE_STATUS_HANDLE(handle as *mut c_void), &status) };
}

unsafe extern "system" fn control(
    ctrl: u32,
    _event_type: u32,
    _event_data: *mut c_void,
    _context: *mut c_void,
) -> u32 {
    match ctrl {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            report(SERVICE_STOP_PENDING, 0, 15_000);
            if let Some(tx) = lock(&STOP).take() {
                let _ = tx.send(());
            }
            NO_ERROR.0
        }
        SERVICE_CONTROL_INTERROGATE => NO_ERROR.0,
        _ => ERROR_CALL_NOT_IMPLEMENTED.0,
    }
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut PWSTR) {
    // SAFETY: registers a handler with the static name; the handler is a plain function.
    let Ok(handle) =
        (unsafe { RegisterServiceCtrlHandlerExW(w!("VyroService"), Some(control), None) })
    else {
        return;
    };
    STATUS.store(handle.0 as usize, Ordering::SeqCst);
    report(SERVICE_START_PENDING, 0, 10_000);

    let data = data_dir();
    crate::logger::init(data.as_ref().ok().map(|d| d.join(crate::LOG_FILE)), false);
    let data = data
        .inspect_err(|e| log::error!("data folder unusable, running without history: {e}"))
        .ok();

    let (tx, rx) = mpsc::channel();
    *lock(&STOP) = Some(tx);
    let endpoint = vyro_core::ipc::default_endpoint();
    let result = crate::run(data, &endpoint, &rx, || report(SERVICE_RUNNING, 0, 0));
    match result {
        Ok(()) => report(SERVICE_STOPPED, 0, 0),
        Err(e) => {
            log::error!("{e}");
            report(SERVICE_STOPPED, 1, 0);
        }
    }
    log::logger().flush();
}

/// `%ProgramData%\Vyro`, created with [`DATA_SDDL`]. A folder that already exists must be owned
/// by SYSTEM or the administrators and must not be a link; anything else (planted by a
/// standard user to redirect the service's writes) is moved aside and replaced.
pub fn data_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("ProgramData").ok_or("%ProgramData% is not set")?;
    let dir = PathBuf::from(base).join("Vyro");
    let mut notes = Vec::new();
    match std::fs::symlink_metadata(&dir) {
        Ok(meta) => {
            let link = meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
            if link || !meta.is_dir() || !trusted_owner(&dir) {
                let aside = dir.with_file_name(format!("Vyro.untrusted-{}", vyro_core::unix_now()));
                std::fs::rename(&dir, &aside).map_err(|e| {
                    format!(
                        "{} is untrusted and cannot be moved aside: {e}",
                        dir.display()
                    )
                })?;
                notes.push(format!(
                    "moved an untrusted {} to {}",
                    dir.display(),
                    aside.display()
                ));
                create(&dir)?;
            } else {
                protect(&dir)?;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => create(&dir)?,
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    }
    // The logger starts after this; keep what happened for the log.
    for n in notes {
        eprintln!("vyro-service: {n}");
    }
    Ok(dir)
}

struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn from_sddl(sddl: &str) -> Result<Self, String> {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: valid SDDL; `sd` receives a LocalAlloc'ed descriptor freed on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &mut sd,
                None,
            )
        }
        .map_err(|e| format!("security descriptor: {e}"))?;
        Ok(Self(sd))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: LocalAlloc'ed by the conversion or GetNamedSecurityInfoW; freed once.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0 .0)));
        }
    }
}

fn create(dir: &Path) -> Result<(), String> {
    let sd = Descriptor::from_sddl(DATA_SDDL)?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0 .0,
        bInheritHandle: false.into(),
    };
    // SAFETY: valid path and security attributes for the duration of the call.
    unsafe { CreateDirectoryW(&HSTRING::from(dir.as_os_str()), Some(&sa)) }
        .map_err(|e| format!("cannot create {}: {e}", dir.display()))
}

fn trusted_owner(dir: &Path) -> bool {
    let mut owner = PSID::default();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: valid path; `owner` points into `sd`, which is freed below.
    let err = unsafe {
        GetNamedSecurityInfoW(
            &HSTRING::from(dir.as_os_str()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            &mut sd,
        )
    };
    if err != ERROR_SUCCESS {
        return false;
    }
    let sd = Descriptor(sd);
    // SAFETY: `owner` is a valid SID inside the live descriptor.
    let trusted = unsafe {
        IsWellKnownSid(owner, WinLocalSystemSid).as_bool()
            || IsWellKnownSid(owner, WinBuiltinAdministratorsSid).as_bool()
    };
    drop(sd);
    trusted
}

/// Re-applies [`DATA_SDDL`] to the folder itself only (no propagation to the files in it).
fn protect(dir: &Path) -> Result<(), String> {
    let sd = Descriptor::from_sddl(DATA_SDDL)?;
    let mut present = windows::core::BOOL::default();
    let mut defaulted = windows::core::BOOL::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `dacl` points into `sd`, which outlives its use.
    unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) }
        .map_err(|e| format!("security descriptor: {e}"))?;
    // SAFETY: opens the folder itself (not a link target) for changing its DACL.
    let handle = unsafe {
        CreateFileW(
            &HSTRING::from(dir.as_os_str()),
            (WRITE_DAC | READ_CONTROL).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|e| format!("cannot open {}: {e}", dir.display()))?;
    // SAFETY: live handle and DACL.
    let err = unsafe {
        SetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl),
            None,
        )
    };
    // SAFETY: owned handle, closed once.
    unsafe {
        let _ = CloseHandle(handle);
    }
    if err == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("cannot secure {}: error {}", dir.display(), err.0))
    }
}
