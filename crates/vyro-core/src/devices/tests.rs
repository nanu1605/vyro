use super::*;

fn container(id: &str, name: &str) -> RawContainer {
    RawContainer {
        id: id.into(),
        name: Some(name.into()),
        connected: Some(true),
        ..RawContainer::default()
    }
}

fn node(container: &str, class: &str, enumerator: &str) -> RawNode {
    RawNode {
        container: Some(container.into()),
        class: Some(class.into()),
        enumerator: Some(enumerator.into()),
        ..RawNode::default()
    }
}

fn endpoint(id: &str, container: Option<&str>, output: bool) -> RawEndpoint {
    RawEndpoint {
        id: id.into(),
        name: format!("Endpoint {id}"),
        output,
        state: AUDIO_ACTIVE,
        container: container.map(Into::into),
        ..RawEndpoint::default()
    }
}

fn one(raw: &Raw) -> DeviceInfo {
    let d = build(raw).devices;
    assert_eq!(d.len(), 1, "{d:?}");
    d[0].clone()
}

fn kind_of(
    c: RawContainer,
    nodes: Vec<RawNode>,
    endpoints: Vec<RawEndpoint>,
) -> Option<DeviceKind> {
    build(&Raw {
        containers: vec![c],
        nodes,
        endpoints,
        ..Raw::default()
    })
    .devices
    .first()
    .map(|d| d.kind)
}

#[test]
fn classifies_by_name_category_and_function() {
    use DeviceKind as K;
    // A gaming mouse has a keyboard collection too: its name decides.
    let mouse = vec![
        node("a", "Mouse", "HID"),
        node("a", "Keyboard", "HID"),
        node("a", "USB", "USB"),
    ];
    assert_eq!(
        kind_of(
            container("a", "G502 HERO Gaming Mouse"),
            mouse.clone(),
            vec![]
        ),
        Some(K::Mouse)
    );
    assert_eq!(
        kind_of(container("a", "Viper V2"), mouse, vec![]),
        Some(K::Input)
    );
    assert_eq!(
        kind_of(
            container("a", "BlackWidow V3"),
            vec![node("a", "Keyboard", "HID"), node("a", "USB", "USB")],
            vec![]
        ),
        Some(K::Keyboard)
    );
    let mut cat = container("a", "MX Anywhere 3");
    cat.categories = vec!["Other".into(), "Input.Mouse".into()];
    assert_eq!(
        kind_of(cat, vec![node("a", "Bluetooth", "BTHLE")], vec![]),
        Some(K::Mouse)
    );

    // Audio by its endpoints.
    let usb = || vec![node("a", "USB", "USB"), node("a", "AudioEndpoint", "SWD")];
    let both = vec![
        endpoint("o", Some("a"), true),
        endpoint("i", Some("a"), false),
    ];
    assert_eq!(
        kind_of(container("a", "Arctis Nova 7"), usb(), both),
        Some(K::Headset)
    );
    let mut hp = endpoint("o", Some("a"), true);
    hp.form_factor = Some(3);
    assert_eq!(
        kind_of(container("a", "Sennheiser GSX"), usb(), vec![hp]),
        Some(K::Headphones)
    );
    assert_eq!(
        kind_of(
            container("a", "Schiit Modi"),
            usb(),
            vec![endpoint("o", Some("a"), true)]
        ),
        Some(K::Speakers)
    );
    assert_eq!(
        kind_of(
            container("a", "Yeti"),
            usb(),
            vec![endpoint("i", Some("a"), false)]
        ),
        Some(K::Microphone)
    );

    // Controllers: the XInput mark, the HID game usage, or the name; "LED Controller" is not one.
    let mut xi = node("a", "HIDClass", "HID");
    xi.hardware_ids = vec!["HID\\VID_045E&PID_0B12&IG_00".into()];
    assert_eq!(
        kind_of(container("a", "Xbox"), vec![xi], vec![]),
        Some(K::Controller)
    );
    let mut game = node("a", "HIDClass", "HID");
    game.compatible_ids = vec!["HID_DEVICE_SYSTEM_GAME".into()];
    assert_eq!(
        kind_of(container("a", "DualSense"), vec![game], vec![]),
        Some(K::Controller)
    );
    assert_eq!(
        kind_of(
            container("a", "AURA LED Controller"),
            vec![node("a", "HIDClass", "HID"), node("a", "USB", "USB")],
            vec![]
        ),
        Some(K::Other)
    );

    let receiver = vec![
        node("a", "Keyboard", "HID"),
        node("a", "Mouse", "HID"),
        node("a", "USB", "USB"),
    ];
    assert_eq!(
        kind_of(container("a", "USB Receiver"), receiver.clone(), vec![]),
        Some(K::Receiver)
    );
    assert_eq!(
        kind_of(container("a", "LIGHTSPEED Receiver"), receiver, vec![]),
        Some(K::Receiver)
    );
    assert_eq!(
        kind_of(
            container("a", "Brio"),
            vec![node("a", "Camera", "USB")],
            vec![]
        ),
        Some(K::Camera)
    );
    assert_eq!(
        kind_of(
            container("a", "Flash"),
            vec![node("a", "DiskDrive", "USBSTOR")],
            vec![]
        ),
        Some(K::Storage)
    );
    assert_eq!(
        kind_of(
            container("a", "Pixel 8"),
            vec![node("a", "WPD", "USB")],
            vec![]
        ),
        Some(K::Phone)
    );
    assert_eq!(
        kind_of(
            container("a", "LaserJet"),
            vec![node("a", "Printer", "USBPRINT")],
            vec![]
        ),
        Some(K::Printer)
    );
}

#[test]
fn leaves_out_the_pc_monitors_hubs_and_virtual_devices() {
    let mut pc = container("pc", "DESKTOP-1");
    pc.local_machine = true;
    let mut hidden = container("h", "Hidden");
    hidden.hidden = true;
    let raw = Raw {
        containers: vec![
            pc,
            hidden,
            container("m", "LG ULTRAGEAR"),
            container("d", "Generic Monitor"),
            container("pdf", "Microsoft Print to PDF"),
            container("hub", "Generic USB Hub"),
            container("gone", "Unplugged stick"),
            container("ok", "Keyboard"),
        ],
        nodes: vec![
            node("pc", "Keyboard", "HID"),
            node("h", "Keyboard", "HID"),
            node("m", "Monitor", "DISPLAY"),
            node("d", "Other", "DISPLAY"),
            node("pdf", "PrintQueue", "SWD"),
            node("pdf", "Printer", "ROOT"),
            RawNode {
                name: Some("Generic USB Hub".into()),
                ..node("hub", "USB", "USB")
            },
            node("ok", "Keyboard", "HID"),
        ],
        ..Raw::default()
    };
    let d = build(&raw).devices;
    assert_eq!(
        d.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
        ["Keyboard"]
    );
}

#[test]
fn connection_state_and_problems() {
    let mut off = container("a", "WH-1000XM5");
    off.connected = Some(false);
    let mut n = node("a", "Bluetooth", "BTHENUM");
    n.battery = Some(40);
    n.problem = Some(43);
    let d = one(&Raw {
        containers: vec![off],
        nodes: vec![n.clone()],
        ..Raw::default()
    });
    assert_eq!(d.connection, Connection::Bluetooth);
    assert!(d.wireless && !d.connected);
    // Nothing stale for a device that is off: no battery, no problem.
    assert_eq!((d.battery, d.problem), (None, None));

    let d = one(&Raw {
        containers: vec![container("a", "WH-1000XM5")],
        nodes: vec![n],
        ..Raw::default()
    });
    assert!(d.connected);
    assert_eq!(d.problem, Some(43));

    // Only Bluetooth uses the container's connected flag: a listed wired device is plugged in.
    let mut usb = container("a", "Keyboard");
    usb.connected = Some(false);
    let d = one(&Raw {
        containers: vec![usb],
        nodes: vec![node("a", "Keyboard", "HID"), node("a", "USB", "USB")],
        ..Raw::default()
    });
    assert_eq!(d.connection, Connection::Usb);
    assert!(d.connected && !d.wireless);

    let mut net = container("a", "LaserJet");
    net.network = true;
    let d = one(&Raw {
        containers: vec![net],
        nodes: vec![node("a", "Printer", "WSDPRINT")],
        ..Raw::default()
    });
    assert_eq!(d.connection, Connection::Network);
    let d = one(&Raw {
        containers: vec![container("a", "MX Master 3S")],
        nodes: vec![node("a", "Mouse", "HID"), node("a", "Bluetooth", "BTHLE")],
        ..Raw::default()
    });
    assert_eq!(d.connection, Connection::BluetoothLe);
    // Placeholder makers are dropped.
    let mut generic = container("a", "Keyboard");
    generic.manufacturer = Some("(Standard keyboards)".into());
    let d = one(&Raw {
        containers: vec![generic],
        nodes: vec![node("a", "Keyboard", "HID")],
        ..Raw::default()
    });
    assert_eq!(d.manufacturer, None);
}

#[test]
fn battery_prefers_windows_then_the_battery_service() {
    let ble = |battery: Option<u8>| {
        let mut n = node("A", "Bluetooth", "BTHLE");
        n.battery = battery;
        vec![node("A", "Mouse", "HID"), n]
    };
    let raw = |nodes, gatt: Vec<(String, u8)>| Raw {
        containers: vec![container("{A}", "MX Master 3S")],
        nodes,
        gatt,
        ..Raw::default()
    };
    let pct = |d: DeviceInfo| d.battery.map(|b| (b.percent, b.source));
    // Container ids match whatever their case or braces.
    assert_eq!(
        pct(one(&raw(ble(Some(64)), vec![("a".into(), 81)]))),
        Some((Some(64), BatterySource::Windows))
    );
    assert_eq!(
        pct(one(&raw(ble(None), vec![("{a}".into(), 81)]))),
        Some((Some(81), BatterySource::BluetoothLe))
    );
    assert_eq!(
        pct(one(&raw(ble(Some(255)), vec![("a".into(), 101)]))),
        None
    );
    assert_eq!(pct(one(&raw(ble(None), vec![]))), None);

    // The Battery Service is read only where Windows has nothing and the device is on.
    assert_eq!(gatt_wanted(&raw(ble(None), vec![])), ["a"]);
    assert!(gatt_wanted(&raw(ble(Some(64)), vec![])).is_empty());
    let mut off = raw(ble(None), vec![]);
    off.containers[0].connected = Some(false);
    assert!(gatt_wanted(&off).is_empty());
    assert!(gatt_wanted(&raw(
        vec![node("A", "Mouse", "HID"), node("A", "Bluetooth", "BTHENUM")],
        vec![]
    ))
    .is_empty());
}

#[test]
fn xinput_battery_matches_one_controller_and_lists_the_rest() {
    let mut xi = node("x", "HIDClass", "HID");
    xi.hardware_ids = vec!["HID\\VID_045E&PID_0B13&IG_00".into()];
    let pad = |slot, battery_type, battery_level| RawPad {
        slot,
        wireless: true,
        battery_type,
        battery_level,
    };
    let raw = |pads: Vec<RawPad>| Raw {
        containers: vec![container("x", "Xbox Wireless Controller")],
        nodes: vec![xi.clone(), node("x", "Bluetooth", "BTHLE")],
        pads,
        ..Raw::default()
    };

    let d = build(&raw(vec![pad(0, 3, 1)])).devices;
    assert_eq!(d.len(), 1);
    assert_eq!(
        d[0].battery,
        Some(Battery {
            percent: None,
            level: Some(BatteryLevel::Low),
            source: BatterySource::Xinput
        })
    );
    // A wired pad has no battery; a disconnected slot is not there.
    assert_eq!(
        build(&raw(vec![pad(0, PAD_WIRED, 0)])).devices[0].battery,
        None
    );
    assert_eq!(
        build(&raw(vec![pad(1, PAD_DISCONNECTED, 0), pad(0, 2, 3)])).devices[0]
            .battery
            .as_ref()
            .and_then(|b| b.level),
        Some(BatteryLevel::Full)
    );

    // Two slots and one controller: no guessing, each slot gets its own row.
    let d = build(&raw(vec![pad(0, 3, 2), pad(2, 2, 0)])).devices;
    let names: Vec<&str> = d.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Controller · player 1",
            "Controller · player 3",
            "Xbox Wireless Controller"
        ]
    );
    assert_eq!(
        d[0].battery.as_ref().and_then(|b| b.level),
        Some(BatteryLevel::Medium)
    );
    assert_eq!(d[2].battery, None);

    // A percentage from Windows beats XInput's levels.
    let mut with_pct = raw(vec![pad(0, 3, 1)]);
    with_pct.nodes[1].battery = Some(72);
    let b = build(&with_pct).devices[0].battery.clone().unwrap();
    assert_eq!((b.percent, b.source), (Some(72), BatterySource::Windows));
}

#[test]
fn audio_endpoints_link_to_listed_devices() {
    let mut unplugged = endpoint("front", None, true);
    unplugged.state = 8;
    unplugged.default = true;
    let mut speakers = endpoint("spk", None, true);
    speakers.default = true;
    let raw = Raw {
        containers: vec![container("h", "Arctis Nova 7")],
        nodes: vec![node("h", "USB", "USB"), node("h", "AudioEndpoint", "SWD")],
        endpoints: vec![
            endpoint("mic", Some("H"), false),
            unplugged,
            endpoint("hp", Some("{h}"), true),
            speakers,
            endpoint("pc-mic", Some("not-listed"), false),
        ],
        ..Raw::default()
    };
    let out = build(&raw);
    let names: Vec<&str> = out.audio.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Endpoint spk",
            "Endpoint hp",
            "Endpoint front",
            "Endpoint mic",
            "Endpoint pc-mic"
        ]
    );
    let a = &out.audio;
    assert!(a[0].default && a[0].plugged && a[0].device.is_none());
    assert_eq!(a[1].device.as_deref(), Some(out.devices[0].id.as_str()));
    // An empty jack is never the default in use.
    assert!(!a[2].plugged && !a[2].default);
    assert_eq!(a[3].device, a[1].device);
    assert_eq!(a[4].device, None);
}

#[test]
fn ids_are_hashes_and_nothing_identifying_leaves() {
    assert_eq!(
        key("{ABCDEF01-0000-1111-2222-333344445555}"),
        key("abcdef01-0000-1111-2222-333344445555")
    );
    assert_eq!(key("x").len(), 12);
    assert_ne!(key("a"), key("b"));

    let mut n = node(
        "{5A3E1C2B-1111-2222-3333-444455556666}",
        "Bluetooth",
        "BTHENUM",
    );
    n.hardware_ids =
        vec!["BTHENUM\\{0000110b-0000-1000-8000-00805f9b34fb}_VID&0001000a_PID&0001".into()];
    n.name = Some("BTHENUM\\7&2A1B&0&001122AABBCC_C00000000".into());
    let raw = Raw {
        containers: vec![container(
            "{5A3E1C2B-1111-2222-3333-444455556666}",
            "Galaxy Buds2 Pro",
        )],
        nodes: vec![n],
        endpoints: vec![RawEndpoint {
            name: "Headphones".into(),
            ..endpoint(
                "{0.0.0.00000000}.{9d2c-secret}",
                Some("5a3e1c2b-1111-2222-3333-444455556666"),
                true,
            )
        }],
        ..Raw::default()
    };
    let json = serde_json::to_string(&build(&raw))
        .unwrap()
        .to_ascii_lowercase();
    for secret in ["001122aabbcc", "5a3e1c2b", "9d2c", "bthenum", "0000110b"] {
        assert!(!json.contains(secret), "{secret} in {json}");
    }
}

#[test]
fn connected_first_then_kind_then_name() {
    let mut off = container("k2", "MX Keys");
    off.connected = Some(false);
    let raw = Raw {
        containers: vec![
            off,
            container("s", "zeta speakers"),
            container("m", "Alpha Mouse"),
            container("k", "Beta Keyboard"),
        ],
        nodes: vec![
            node("k2", "Keyboard", "BTHLE"),
            node("s", "USB", "USB"),
            node("m", "Mouse", "USB"),
            node("k", "Keyboard", "USB"),
        ],
        endpoints: vec![endpoint("o", Some("s"), true)],
        ..Raw::default()
    };
    let names: Vec<String> = build(&raw).devices.into_iter().map(|d| d.name).collect();
    assert_eq!(
        names,
        ["Beta Keyboard", "Alpha Mouse", "zeta speakers", "MX Keys"]
    );
}

#[test]
fn sample_shows_each_source() {
    let s = sample();
    let by = |name: &str| s.devices.iter().find(|d| d.name == name).unwrap().clone();
    assert_eq!(
        by("Xbox Wireless Controller").battery.and_then(|b| b.level),
        Some(BatteryLevel::Medium)
    );
    assert_eq!(
        by("Galaxy Buds2 Pro").battery.map(|b| b.source),
        Some(BatterySource::Windows)
    );
    assert_eq!(
        by("MX Master 3S").battery.map(|b| b.source),
        Some(BatterySource::BluetoothLe)
    );
    assert!(!by("MX Keys").connected);
    assert_eq!(by("MX Keys").kind, DeviceKind::Keyboard);
    assert_eq!(by("Arctis Nova 7").kind, DeviceKind::Headset);
    assert_eq!(by("AURA LED Controller").kind, DeviceKind::Other);
    assert_eq!(by("AURA LED Controller").manufacturer, None);
    assert_eq!(by("USB Receiver").kind, DeviceKind::Receiver);
    assert!(by("USB Receiver").wireless);
    assert_eq!(s.audio.len(), 5);
}
