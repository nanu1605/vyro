//! What Windows Defender Firewall does with the app's incoming connections on the networks in
//! use, so Settings can say "Windows will ask", "allowed" or "blocked" instead of the companion
//! silently not answering. Read only: Vyro never adds or changes a rule.

use super::FirewallState;

/// An inbound rule naming the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub enabled: bool,
    pub allow: bool,
    /// `NET_FW_PROFILE2_*` bits.
    pub profiles: i32,
    /// TCP or any protocol.
    pub tcp: bool,
}

/// `on`: the firewall is on for a network in use; `current`: those networks' profile bits.
/// Block rules win, as in the firewall itself; with no rule Windows asks on first use.
pub fn decide(on: bool, current: i32, rules: &[Rule]) -> FirewallState {
    if !on {
        return FirewallState::Off;
    }
    let applies = |r: &&Rule| r.enabled && r.tcp && r.profiles & current != 0;
    if rules.iter().filter(applies).any(|r| !r.allow) {
        FirewallState::Blocked
    } else if rules.iter().filter(applies).any(|r| r.allow) {
        FirewallState::Allowed
    } else {
        FirewallState::Ask
    }
}

/// The firewall's state for `exe` (this app); `Unknown` when it cannot be read.
pub fn state(exe: &std::path::Path) -> FirewallState {
    #[cfg(windows)]
    return win::read(exe).unwrap_or_else(|e| {
        log::debug!("remote: firewall state: {e}");
        FirewallState::Unknown
    });
    #[cfg(not(windows))]
    {
        let _ = exe;
        FirewallState::Unknown
    }
}

#[cfg(windows)]
mod win {
    use windows::core::Interface;
    use windows::Win32::NetworkManagement::WindowsFirewall::{
        INetFwPolicy2, INetFwRule, NetFwPolicy2, NET_FW_ACTION_ALLOW, NET_FW_PROFILE_TYPE2,
        NET_FW_RULE_DIR_IN,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IDispatch, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Ole::IEnumVARIANT;
    use windows::Win32::System::Variant::VARIANT;

    use super::{decide, Rule};
    use crate::remote::FirewallState;

    /// `NET_FW_IP_PROTOCOL_TCP` and `..._ANY`.
    const TCP: i32 = 6;
    const ANY: i32 = 256;

    pub fn read(exe: &std::path::Path) -> windows::core::Result<FirewallState> {
        // SAFETY: COM on this thread; an earlier initialisation (any model) is fine.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // SAFETY: plain COM calls on interfaces this function owns.
        unsafe {
            let policy: INetFwPolicy2 =
                CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER)?;
            let current = policy.CurrentProfileTypes()?;
            let mut on = false;
            for bit in [1, 2, 4] {
                if current & bit != 0
                    && policy
                        .get_FirewallEnabled(NET_FW_PROFILE_TYPE2(bit))?
                        .as_bool()
                {
                    on = true;
                }
            }
            let me = exe.to_string_lossy().to_lowercase();
            let items: IEnumVARIANT = policy.Rules()?._NewEnum()?.cast()?;
            let mut rules = Vec::new();
            loop {
                let mut v = [VARIANT::default()];
                let mut got = 0u32;
                if items.Next(&mut v, &mut got).is_err() || got == 0 {
                    break;
                }
                let Ok(rule) = IDispatch::try_from(&v[0]).and_then(|d| d.cast::<INetFwRule>())
                else {
                    continue;
                };
                let app = rule
                    .ApplicationName()
                    .map(|b| b.to_string().to_lowercase())
                    .unwrap_or_default();
                if app != me || rule.Direction()? != NET_FW_RULE_DIR_IN {
                    continue;
                }
                let proto = rule.Protocol()?;
                rules.push(Rule {
                    enabled: rule.Enabled()?.as_bool(),
                    allow: rule.Action()? == NET_FW_ACTION_ALLOW,
                    profiles: rule.Profiles()?,
                    tcp: proto == TCP || proto == ANY,
                });
            }
            Ok(decide(on, current, &rules))
        }
    }
}
