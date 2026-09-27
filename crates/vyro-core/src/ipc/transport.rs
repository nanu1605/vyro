//! Local connections between the sensor service and its clients.
//!
//! On Windows a named pipe that only the service can create instances of, that SYSTEM and
//! administrators may fully use and interactive users may only read and write, and that
//! refuses remote clients. Clients check that the pipe's server process is the installed
//! service before trusting it. Elsewhere a Unix socket readable by its owner only (for
//! development and tests).

use std::io::{self, Read, Write};

/// A connected byte stream.
pub trait Conn: Read + Write + Send {}
impl<T: Read + Write + Send> Conn for T {}

/// Where the service listens by default: the named pipe on Windows, else `$VYRO_SOCKET` or
/// `vyro-engine.sock` in the runtime (or temp) folder.
pub fn default_endpoint() -> String {
    #[cfg(windows)]
    {
        win::PIPE_NAME.to_owned()
    }
    #[cfg(not(windows))]
    {
        if let Some(path) = std::env::var_os("VYRO_SOCKET") {
            return path.to_string_lossy().into_owned();
        }
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map_or_else(std::env::temp_dir, std::path::PathBuf::from);
        dir.join("vyro-engine.sock").to_string_lossy().into_owned()
    }
}

pub struct Listener {
    #[cfg(windows)]
    inner: win::PipeListener,
    #[cfg(unix)]
    inner: std::os::unix::net::UnixListener,
}

impl Listener {
    /// Claims `endpoint`. On Windows this fails if another process already serves the pipe.
    pub fn bind(endpoint: &str) -> io::Result<Self> {
        #[cfg(windows)]
        return Ok(Self {
            inner: win::PipeListener::bind(endpoint)?,
        });
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // A socket file left by a previous run would make bind fail.
            let _ = std::fs::remove_file(endpoint);
            let inner = std::os::unix::net::UnixListener::bind(endpoint)?;
            std::fs::set_permissions(endpoint, std::fs::Permissions::from_mode(0o600))?;
            Ok(Self { inner })
        }
    }

    /// Waits for the next client. Returns the connection and, where the platform reports it,
    /// the client's process id.
    pub fn accept(&self) -> io::Result<(Box<dyn Conn>, Option<u32>)> {
        #[cfg(windows)]
        return self.inner.accept();
        #[cfg(unix)]
        {
            let (stream, _) = self.inner.accept()?;
            Ok((Box::new(stream), None))
        }
    }
}

/// Connects to the service at `endpoint`. On Windows the service's well-known pipe must be
/// served by the installed service (or `VYRO_DEV_PIPE=1` must be set, for a service started
/// from a console); other pipe names (tests, `--endpoint`) are taken as given.
pub fn connect(endpoint: &str) -> io::Result<Box<dyn Conn>> {
    #[cfg(windows)]
    return win::connect(endpoint);
    #[cfg(unix)]
    {
        let stream = std::os::unix::net::UnixStream::connect(endpoint)?;
        Ok(Box::new(stream))
    }
}

/// State of the installed service, if any (Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    NotInstalled,
    Stopped,
    Starting,
    Running,
    /// Stopping, paused or another transitional state.
    Other,
}

pub fn service_state() -> ServiceState {
    #[cfg(windows)]
    return win::service_status().map_or(ServiceState::NotInstalled, |(state, _)| state);
    #[cfg(not(windows))]
    ServiceState::NotInstalled
}

#[cfg(windows)]
mod win {
    use std::fs::File;
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
    use std::sync::Mutex;

    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL,
        INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT,
    };
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId,
        GetNamedPipeServerProcessId, WaitNamedPipeW, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, SC_MANAGER_CONNECT,
        SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START_PENDING,
        SERVICE_STATUS_PROCESS, SERVICE_STOPPED,
    };

    use super::{Conn, ServiceState};
    use crate::lock;

    pub const PIPE_NAME: &str = r"\\.\pipe\vyro-engine";

    /// FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE:
    /// enough to talk, not FILE_CREATE_PIPE_INSTANCE (which GENERIC_WRITE would include), so
    /// no one else can add server instances to the pipe.
    const CLIENT_ACCESS: u32 = 0x0012_0083;

    /// Protected DACL: SYSTEM, administrators and the pipe's owner (the service; a developer's
    /// console run) full control; interactive users talk only.
    fn pipe_sddl() -> String {
        format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;{CLIENT_ACCESS:#x};;;IU)")
    }

    struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

    // SAFETY: an immutable, self-contained LocalAlloc'ed buffer.
    unsafe impl Send for SecurityDescriptor {}
    // SAFETY: as above; only read after creation.
    unsafe impl Sync for SecurityDescriptor {}

    impl Drop for SecurityDescriptor {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0 .0)));
            }
        }
    }

    struct PipeHandle(HANDLE);
    // SAFETY: a kernel handle, usable from any thread.
    unsafe impl Send for PipeHandle {}

    pub struct PipeListener {
        name: HSTRING,
        sd: SecurityDescriptor,
        /// The instance waiting for the next client, created ahead so a client never finds the
        /// pipe missing between two accepts.
        next: Mutex<Option<PipeHandle>>,
    }

    impl PipeListener {
        pub fn bind(name: &str) -> io::Result<Self> {
            let mut sd = PSECURITY_DESCRIPTOR::default();
            // SAFETY: valid SDDL string; `sd` receives a LocalAlloc'ed descriptor.
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    &HSTRING::from(pipe_sddl()),
                    SDDL_REVISION_1,
                    &mut sd,
                    None,
                )
            }
            .map_err(io::Error::other)?;
            let listener = Self {
                name: HSTRING::from(name),
                sd: SecurityDescriptor(sd),
                next: Mutex::new(None),
            };
            // The first instance must be ours: fails if another process already serves it.
            let first = listener.instance(true)?;
            *lock(&listener.next) = Some(first);
            Ok(listener)
        }

        fn instance(&self, first: bool) -> io::Result<PipeHandle> {
            let sa = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.sd.0 .0,
                bInheritHandle: false.into(),
            };
            let mut mode = PIPE_ACCESS_DUPLEX;
            if first {
                mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
            }
            // SAFETY: valid name and security attributes for the duration of the call.
            let h = unsafe {
                CreateNamedPipeW(
                    &self.name,
                    mode,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_UNLIMITED_INSTANCES,
                    64 * 1024,
                    64 * 1024,
                    0,
                    Some(&sa),
                )
            };
            if h == INVALID_HANDLE_VALUE || h.is_invalid() {
                return Err(io::Error::last_os_error());
            }
            Ok(PipeHandle(h))
        }

        pub fn accept(&self) -> io::Result<(Box<dyn Conn>, Option<u32>)> {
            let pending = lock(&self.next).take();
            let h = match pending {
                Some(h) => h,
                None => self.instance(false)?,
            };
            // SAFETY: `h` is a live pipe instance; blocking connect.
            if let Err(e) = unsafe { ConnectNamedPipe(h.0, None) } {
                if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                    // SAFETY: owned handle, closed once.
                    unsafe {
                        let _ = CloseHandle(h.0);
                    }
                    return Err(io::Error::other(e));
                }
            }
            let mut pid = 0u32;
            // SAFETY: connected pipe handle; `pid` is a valid out pointer.
            let pid = unsafe { GetNamedPipeClientProcessId(h.0, &mut pid) }
                .ok()
                .map(|()| pid);
            // SAFETY: the handle is owned; File takes over closing it.
            let file = unsafe { File::from_raw_handle(h.0 .0 as RawHandle) };
            match self.instance(false) {
                Ok(next) => *lock(&self.next) = Some(next),
                Err(e) => log::warn!("ipc: cannot pre-create the next pipe instance: {e}"),
            }
            Ok((Box::new(file), pid))
        }
    }

    impl Drop for PipeListener {
        fn drop(&mut self) {
            if let Some(h) = lock(&self.next).take() {
                // SAFETY: owned, unconnected instance.
                unsafe {
                    let _ = CloseHandle(h.0);
                }
            }
        }
    }

    pub fn connect(name: &str) -> io::Result<Box<dyn Conn>> {
        let mut tries = 0;
        let file = loop {
            let opened = std::fs::OpenOptions::new()
                .access_mode(CLIENT_ACCESS)
                // The service may identify the caller but never act as it.
                .security_qos_flags(SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0)
                .open(name);
            match opened {
                Ok(f) => break f,
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) && tries < 5 => {
                    tries += 1;
                    // SAFETY: valid name.
                    let _ = unsafe { WaitNamedPipeW(&HSTRING::from(name), 1000) };
                }
                Err(e) => return Err(e),
            }
        };
        if name.eq_ignore_ascii_case(PIPE_NAME) {
            verify_server(&file)?;
        }
        Ok(Box::new(file))
    }

    /// The pipe must be served by the running Vyro service, so a process that grabbed the name
    /// first cannot pose as it.
    fn verify_server(file: &File) -> io::Result<()> {
        if std::env::var_os("VYRO_DEV_PIPE").is_some_and(|v| v == "1") {
            return Ok(());
        }
        let mut server = 0u32;
        // SAFETY: open pipe handle; valid out pointer.
        unsafe { GetNamedPipeServerProcessId(HANDLE(file.as_raw_handle()), &mut server) }
            .map_err(io::Error::other)?;
        match service_status() {
            Some((ServiceState::Running, pid)) if pid == server && pid != 0 => Ok(()),
            _ => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the engine pipe is not served by the Vyro service",
            )),
        }
    }

    /// The service's state and process id; `None` when it is not installed or not visible.
    pub fn service_status() -> Option<(ServiceState, u32)> {
        // SAFETY: plain SCM calls; handles are closed below and the buffer outlives the call.
        unsafe {
            let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT).ok()?;
            let svc = OpenServiceW(
                scm,
                &HSTRING::from(crate::ipc::SERVICE_NAME),
                SERVICE_QUERY_STATUS,
            );
            let result = svc.ok().and_then(|svc| {
                let mut status = SERVICE_STATUS_PROCESS::default();
                let buf = std::slice::from_raw_parts_mut(
                    std::ptr::from_mut(&mut status).cast::<u8>(),
                    size_of::<SERVICE_STATUS_PROCESS>(),
                );
                let mut needed = 0u32;
                let ok = QueryServiceStatusEx(svc, SC_STATUS_PROCESS_INFO, Some(buf), &mut needed);
                let _ = CloseServiceHandle(svc);
                ok.ok()?;
                let state = match status.dwCurrentState {
                    SERVICE_RUNNING => ServiceState::Running,
                    SERVICE_START_PENDING => ServiceState::Starting,
                    SERVICE_STOPPED => ServiceState::Stopped,
                    _ => ServiceState::Other,
                };
                Some((state, status.dwProcessId))
            });
            let _ = CloseServiceHandle(scm);
            result
        }
    }
}
