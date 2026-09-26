//! "Start with Windows": a Task Scheduler task that starts Vyro elevated at logon, straight to
//! the tray. A plain Run-key entry cannot start an app that requires administrator rights
//! without a UAC prompt; a task with the highest run level can. The MSI removes the task on
//! uninstall (`src-tauri/wix/pawnio.wxs`).

/// Task name, also used by the MSI's uninstall action.
pub const TASK_NAME: &str = "Vyro";
/// Command-line flag: start hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Task definition: at `user`'s logon, run `exe --minimized` with the highest privileges,
/// on battery too, with no time limit.
pub fn task_xml(exe: &str, user: &str) -> String {
    let (exe, user) = (escape(exe), escape(user));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts Vyro in the tray at sign-in (low-level sensors need administrator rights).</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT10S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{exe}"</Command>
      <Arguments>{MINIMIZED_ARG}</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

#[cfg(windows)]
mod win {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Output};

    use super::{task_xml, TASK_NAME};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn schtasks(args: &[&str]) -> std::io::Result<Output> {
        let dir = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        Command::new(
            std::path::Path::new(&dir)
                .join("System32")
                .join("schtasks.exe"),
        )
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    }

    fn check(out: Output) -> Result<(), String> {
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
        }
    }

    pub fn is_enabled() -> bool {
        schtasks(&["/Query", "/TN", TASK_NAME]).is_ok_and(|o| o.status.success())
    }

    /// Creates (or replaces) the logon task for the current user. Needs administrator rights,
    /// which the release build always has.
    pub fn enable() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
            (Ok(d), Ok(u)) => format!("{d}\\{u}"),
            (_, Ok(u)) => u,
            _ => return Err("cannot tell the current user".into()),
        };
        let xml = task_xml(&exe.to_string_lossy(), &user);
        // schtasks reads task XML reliably as UTF-16 LE with a byte-order mark.
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        let path = std::env::temp_dir().join(format!("vyro-task-{}.xml", std::process::id()));
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        let result = schtasks(&[
            "/Create",
            "/TN",
            TASK_NAME,
            "/XML",
            &path.to_string_lossy(),
            "/F",
        ])
        .map_err(|e| e.to_string())
        .and_then(check);
        let _ = std::fs::remove_file(&path);
        result
    }

    pub fn disable() -> Result<(), String> {
        if !is_enabled() {
            return Ok(());
        }
        schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])
            .map_err(|e| e.to_string())
            .and_then(check)
    }
}

#[cfg(windows)]
pub use win::{disable, enable, is_enabled};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_runs_elevated_at_logon_on_battery_too() {
        let xml = task_xml(r"C:\Program Files\Vyro\vyro.exe", r"PC\Tanu & Co");
        assert!(xml.contains(r#"<Command>"C:\Program Files\Vyro\vyro.exe"</Command>"#));
        assert!(xml.contains("<Arguments>--minimized</Arguments>"));
        assert!(xml.contains("<UserId>PC\\Tanu &amp; Co</UserId>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
    }
}
