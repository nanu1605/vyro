use std::net::Ipv4Addr;

use super::power::{PowerAction, PowerRequest, MAX_WAIT, WAIT};
use super::process::{file_name, refused};
use super::wake::{broadcast, mac_text, magic_packet, parse_mac};

#[test]
fn windows_and_vyro_processes_are_never_ended() {
    for name in [
        "System",
        "csrss.exe",
        "LSASS.EXE",
        "winlogon.exe",
        "svchost.exe",
        "explorer.exe",
        "vyro.exe",
        "vyro-service.exe",
        " ",
    ] {
        assert!(refused(1234, name).is_some(), "{name}");
    }
    assert!(
        refused(4, "game.exe").is_some(),
        "the System process number"
    );
    assert!(
        refused(std::process::id(), "game.exe").is_some(),
        "this process"
    );
    assert_eq!(refused(1234, "chrome.exe"), None);
    assert_eq!(refused(1234, "Game.exe"), None);
    assert_eq!(file_name(r"C:\Program Files\Game\Game.exe"), "Game.exe");
    assert_eq!(file_name("game.exe"), "game.exe");
}

#[test]
fn shutting_down_waits_within_limits_and_others_do_not() {
    let req: PowerRequest = serde_json::from_str(r#"{"action":"shutDown"}"#).unwrap();
    assert_eq!((req.wait(), req.force), (WAIT, false));
    let now: PowerRequest =
        serde_json::from_str(r#"{"action":"restart","wait":0,"force":true}"#).unwrap();
    assert_eq!((now.wait(), now.force), (0, true));
    let long = PowerRequest {
        action: PowerAction::ShutDown,
        wait: 100_000,
        force: false,
    };
    assert_eq!(long.wait(), MAX_WAIT);
    let lock = PowerRequest {
        action: PowerAction::Lock,
        wait: 30,
        force: false,
    };
    assert_eq!(lock.wait(), 0);
    assert!(serde_json::from_str::<PowerRequest>(r#"{"action":"format"}"#).is_err());
    assert!(super::power::message(PowerAction::Restart, "Pixel 9")
        .contains("Restart asked from Pixel 9"));
}

#[test]
fn wake_packets_and_addresses() {
    let mac = parse_mac("A8-A1-59-12-34-56").unwrap();
    assert_eq!(parse_mac("a8:a1:59:12:34:56"), Some(mac));
    assert_eq!(parse_mac("a8a159123456"), Some(mac));
    assert_eq!(mac_text(&mac), "A8-A1-59-12-34-56");
    for bad in [
        "",
        "A8-A1-59-12-34",
        "ZZ-A1-59-12-34-56",
        "00-00-00-00-00-00",
        "FF-FF-FF-FF-FF-FF",
        "01-00-5E-00-00-01",
    ] {
        assert_eq!(parse_mac(bad), None, "{bad}");
    }
    let p = magic_packet(&mac);
    assert_eq!(&p[..6], &[0xff; 6]);
    for i in 0..16 {
        assert_eq!(&p[6 + i * 6..12 + i * 6], &mac);
    }
    assert_eq!(
        broadcast(Ipv4Addr::new(192, 168, 1, 23), 24),
        Ipv4Addr::new(192, 168, 1, 255)
    );
    assert_eq!(
        broadcast(Ipv4Addr::new(10, 1, 2, 3), 8),
        Ipv4Addr::new(10, 255, 255, 255)
    );
    assert_eq!(
        broadcast(Ipv4Addr::new(10, 1, 2, 3), 32),
        Ipv4Addr::new(10, 1, 2, 3)
    );
}
