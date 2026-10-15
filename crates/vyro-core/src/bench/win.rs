//! Windows pieces of the benchmark: Windows' power throttling for this process, free space on a
//! drive, and the Direct3D 11 GPU workloads (`gpu`).

pub mod gpu;

use std::path::Path;

use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Threading::{
    GetCurrentProcess, ProcessPowerThrottling, SetProcessInformation,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE,
};

pub fn full_speed_process(on: bool) {
    // ControlMask set with StateMask clear opts out of EcoQoS; both clear hand it back.
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: if on {
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED
        } else {
            0
        },
        StateMask: 0,
    };
    // SAFETY: the pointer and size describe `state`, which outlives the call; the pseudo
    // handle needs no closing.
    let r = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            std::ptr::from_ref(&state).cast(),
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if let Err(e) = r {
        log::debug!("benchmark: power throttling: {e}");
    }
}

pub fn free_bytes(path: &Path) -> Option<u64> {
    let mut free = 0u64;
    // SAFETY: the string outlives the call and `free` is a valid out pointer.
    unsafe {
        GetDiskFreeSpaceExW(
            &HSTRING::from(path.as_os_str()),
            Some(&mut free),
            None,
            None,
        )
    }
    .ok()?;
    Some(free)
}
