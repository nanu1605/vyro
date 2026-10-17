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
fn indirect_names_give_their_fallback_text() {
    assert_eq!(
        plain_text("@input.inf,%hid.mousedevice%;HID-compliant mouse").as_deref(),
        Some("HID-compliant mouse")
    );
    assert_eq!(plain_text("@bthpan.sys,#1"), None);
    assert_eq!(
        plain_text("  MX Master 3S ").as_deref(),
        Some("MX Master 3S")
    );
    assert_eq!(plain_text("   "), None);
}

/// 1.3: Settings named every device while Vyro said "Unknown device". The name comes from the
/// device itself first, then its paired entry, its bus, its own node, and only then its kind.
#[test]
fn names_come_from_the_device_then_its_parts_then_its_kind() {
    let unnamed = || RawContainer {
        id: "a".into(),
        connected: Some(true),
        ..RawContainer::default()
    };
    let named = |n: &str, mut node: RawNode| {
        node.name = Some(n.into());
        node
    };
    let name = |c: RawContainer, nodes: Vec<RawNode>, aeps: Vec<RawAep>| {
        one(&Raw {
            containers: vec![c],
            nodes,
            aeps,
            ..Raw::default()
        })
        .name
    };
    let aep = |n: &str| RawAep {
        container: Some("{A}".into()),
        name: Some(n.into()),
        battery: None,
    };

    // The container's own name wins.
    assert_eq!(
        name(
            container("a", "WH-1000XM4"),
            vec![named("Avrcp Transport", node("a", "Bluetooth", "BTHENUM"))],
            vec![aep("Other")]
        ),
        "WH-1000XM4"
    );
    // A paired device's entry, as Settings lists it, even with its parts named generically.
    assert_eq!(
        name(
            unnamed(),
            vec![named(
                "Generic Attribute Profile",
                node("a", "Bluetooth", "BTHLEDEVICE")
            )],
            vec![aep("MX Master 3S")]
        ),
        "MX Master 3S"
    );
    // The product string on the USB bus before Windows' function names.
    let mut usb = node("a", "USB", "USB");
    usb.bus_name = Some("USB Audio Condenser Mic".into());
    usb.name = Some("USB Composite Device".into());
    assert_eq!(
        name(
            unnamed(),
            vec![usb, node("a", "AudioEndpoint", "SWD")],
            vec![]
        ),
        "USB Audio Condenser Mic"
    );
    // A Bluetooth LE device's own node, not its GATT services.
    assert_eq!(
        name(
            unnamed(),
            vec![
                named(
                    "Device Information Service",
                    node("a", "Bluetooth", "BTHLEDEVICE")
                ),
                named("Xbox Wireless Controller", node("a", "Bluetooth", "BTHLE")),
                named(
                    "HID-compliant game controller",
                    node("a", "HIDClass", "HID")
                ),
            ],
            vec![]
        ),
        "Xbox Wireless Controller"
    );
    // Nothing but Windows' function names: the kind, never "Unknown device".
    assert_eq!(
        name(
            unnamed(),
            vec![named("HID-compliant mouse", node("a", "Mouse", "HID"))],
            vec![]
        ),
        "Mouse"
    );
    assert_eq!(
        name(unnamed(), vec![node("a", "Keyboard", "HID")], vec![]),
        "Keyboard"
    );
}

#[test]
fn the_chosen_name_decides_the_kind() {
    let unnamed = RawContainer {
        id: "a".into(),
        connected: Some(true),
        ..RawContainer::default()
    };
    let mut usb = node("a", "USB", "USB");
    usb.bus_name = Some("USB Receiver".into());
    let d = one(&Raw {
        containers: vec![unnamed],
        nodes: vec![usb, node("a", "Mouse", "HID"), node("a", "Keyboard", "HID")],
        ..Raw::default()
    });
    assert_eq!(
        (d.name.as_str(), d.kind),
        ("USB Receiver", DeviceKind::Receiver)
    );
    assert!(d.wireless);
}

#[test]
fn windows_battery_also_from_the_container_or_paired_entry() {
    let raw = |c_battery: Option<u8>, a_battery: Option<u8>| {
        let mut c = container("{A}", "Buds");
        c.battery = c_battery;
        Raw {
            containers: vec![c],
            nodes: vec![node("A", "Bluetooth", "BTHENUM")],
            aeps: vec![RawAep {
                container: Some("a".into()),
                name: None,
                battery: a_battery,
            }],
            ..Raw::default()
        }
    };
    let pct = |r: &Raw| one(r).battery.map(|b| (b.percent, b.source));
    assert_eq!(
        pct(&raw(Some(40), None)),
        Some((Some(40), BatterySource::Windows))
    );
    assert_eq!(
        pct(&raw(None, Some(55))),
        Some((Some(55), BatterySource::Windows))
    );
    assert_eq!(pct(&raw(None, None)), None);
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
        ids: None,
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
            charging: None,
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
fn xinput_slots_match_controllers_by_vendor_and_product() {
    assert_eq!(
        vid_pid(&["HID\\VID_045E&PID_0B13&IG_00".into()]),
        Some((0x045E, 0x0B13))
    );
    assert_eq!(
        vid_pid(&["HID\\{00001812-0000-1000-8000-00805f9b34fb}_VID&02045e_PID&0b13&IG_00".into()]),
        Some((0x045E, 0x0B13))
    );
    assert_eq!(vid_pid(&["ROOT\\THING".into()]), None);

    let pad_node = |c: &str, id: &str| {
        let mut n = node(c, "HIDClass", "HID");
        n.hardware_ids = vec![id.into()];
        n
    };
    let pad = |slot, ids, level| RawPad {
        slot,
        ids,
        wireless: true,
        battery_type: 2,
        battery_level: level,
    };
    // Two different controllers and two slots: each slot finds its own.
    let d = build(&Raw {
        containers: vec![
            container("x", "Xbox Wireless Controller"),
            container("b", "8BitDo Ultimate"),
        ],
        nodes: vec![
            pad_node("x", "HID\\VID_045E&PID_0B13&IG_00"),
            pad_node("b", "HID\\VID_2DC8&PID_3106&IG_00"),
        ],
        pads: vec![
            pad(0, Some((0x2DC8, 0x3106)), 1),
            pad(1, Some((0x045E, 0x0B13)), 3),
        ],
        ..Raw::default()
    })
    .devices;
    let level = |name: &str| {
        d.iter()
            .find(|x| x.name == name)
            .and_then(|x| x.battery.as_ref())
            .and_then(|b| b.level)
    };
    assert_eq!(d.len(), 2, "{d:?}");
    assert_eq!(level("8BitDo Ultimate"), Some(BatteryLevel::Low));
    assert_eq!(level("Xbox Wireless Controller"), Some(BatteryLevel::Full));
}

#[test]
fn devices_behind_a_receiver_get_their_own_rows() {
    let hid = |slot, name: &str, kind, awake, pct| RawHid {
        container: "{R}".into(),
        slot: Some(slot),
        name: Some(name.into()),
        kind: Some(kind),
        awake,
        charge: Some(hid::Charge {
            percent: Some(pct),
            level: None,
            charging: Some(slot == 2),
        }),
        source: BatterySource::Logitech,
    };
    let mut receiver = container("r", "USB Receiver");
    receiver.manufacturer = Some("Logitech".into());
    let d = build(&Raw {
        containers: vec![receiver],
        nodes: vec![node("r", "USB", "USB"), node("r", "Mouse", "HID")],
        hid: vec![
            hid(1, "G Pro X Superlight 2", DeviceKind::Mouse, true, 82),
            hid(2, "G915 TKL", DeviceKind::Keyboard, true, 46),
            hid(3, "MX Anywhere 3", DeviceKind::Mouse, false, 70),
        ],
        ..Raw::default()
    })
    .devices;
    let r = d.iter().find(|x| x.name == "USB Receiver").unwrap();
    assert_eq!((r.kind, r.parent.as_deref()), (DeviceKind::Receiver, None));
    let mouse = d.iter().find(|x| x.name == "G Pro X Superlight 2").unwrap();
    assert_eq!(mouse.parent.as_deref(), Some(r.id.as_str()));
    assert_eq!(
        (
            mouse.kind,
            mouse.connection,
            mouse.wireless,
            mouse.connected
        ),
        (DeviceKind::Mouse, Connection::Receiver, true, true)
    );
    assert_eq!(mouse.manufacturer.as_deref(), Some("Logitech"));
    let b = mouse.battery.clone().unwrap();
    assert_eq!(
        (b.percent, b.charging, b.source),
        (Some(82), Some(false), BatterySource::Logitech)
    );
    let kb = d.iter().find(|x| x.name == "G915 TKL").unwrap();
    assert_eq!(kb.battery.as_ref().and_then(|b| b.charging), Some(true));
    // Seen earlier, asleep now: listed, not connected, no battery.
    let asleep = d.iter().find(|x| x.name == "MX Anywhere 3").unwrap();
    assert_eq!((asleep.connected, asleep.battery.clone()), (false, None));
    // Ids are hashes too, and differ per slot.
    assert!(d.iter().all(|x| x.id.len() == 12));
    assert_ne!(mouse.id, kb.id);
}

#[test]
fn a_maker_protocol_battery_comes_after_windows_and_the_battery_service() {
    let raw = |windows: Option<u8>| {
        let mut usb = node("p", "USB", "USB");
        usb.battery = windows;
        Raw {
            containers: vec![container("p", "DualSense Wireless Controller")],
            nodes: vec![usb, node("p", "HIDClass", "HID")],
            hid: vec![RawHid {
                container: "P".into(),
                slot: None,
                name: None,
                kind: None,
                awake: true,
                charge: Some(hid::Charge {
                    percent: Some(75),
                    level: None,
                    charging: Some(true),
                }),
                source: BatterySource::PlayStation,
            }],
            ..Raw::default()
        }
    };
    let b = one(&raw(None)).battery.unwrap();
    assert_eq!(
        (b.percent, b.charging, b.source),
        (Some(75), Some(true), BatterySource::PlayStation)
    );
    assert_eq!(
        one(&raw(Some(40))).battery.unwrap().source,
        BatterySource::Windows
    );
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
    assert_eq!(
        by("G Pro X Superlight 2").battery.map(|b| b.source),
        Some(BatterySource::Logitech)
    );
    assert_eq!(by("G915 TKL").battery.and_then(|b| b.charging), Some(true));
    assert_eq!(
        by("DualSense Wireless Controller")
            .battery
            .map(|b| b.source),
        Some(BatterySource::PlayStation)
    );
    assert_eq!(by("USB Audio Condenser Mic").kind, DeviceKind::Microphone);
    assert_eq!(s.audio.len(), 6);
}

#[test]
fn bluetooth_addresses_come_from_the_device_node_only() {
    assert_eq!(
        bt_address("BTHENUM\\DEV_B0B44894CE4D\\7&1B2C3D&0&BLUETOOTHDEVICE_B0B44894CE4D"),
        Some((0xB0B4_4894_CE4D, false))
    );
    assert_eq!(
        bt_address("bthle\\dev_d02a3b1f4c77\\7&2a&0&d02a3b1f4c77"),
        Some((0xD02A_3B1F_4C77, true))
    );
    // A service's node, the radio, a USB device and nonsense have none.
    assert_eq!(
        bt_address("BTHENUM\\{0000110B-0000-1000-8000-00805F9B34FB}_LOCALMFG&0002\\7&1&0&B0B44894CE4D_C00000000"),
        None
    );
    assert_eq!(bt_address("BTH\\MS_BTHBRB\\7&1"), None);
    assert_eq!(bt_address("USB\\VID_046D&PID_C539\\1"), None);
    assert_eq!(bt_address("BTHLE\\DEV_ZZZZ\\1"), None);
    assert_eq!(bt_address("BTHLE\\DEV_000000000000\\1"), None);
}

#[test]
fn bluetooth_devices_get_connect_and_disconnect() {
    let d = |nodes: Vec<RawNode>, endpoints: Vec<RawEndpoint>, name: &str| {
        one(&Raw {
            containers: vec![container("a", name)],
            nodes,
            endpoints,
            ..Raw::default()
        })
        .control
    };
    // Audio goes through its audio connection, also while off and without endpoints.
    assert_eq!(
        d(
            vec![node("a", "Bluetooth", "BTHENUM")],
            vec![endpoint("e", Some("a"), true)],
            "Speaker"
        ),
        Some(Control::Audio)
    );
    assert_eq!(
        d(
            vec![node("a", "Bluetooth", "BTHENUM")],
            vec![],
            "WH-1000XM4 Headphones"
        ),
        Some(Control::Audio)
    );
    // A Bluetooth mouse or controller: its link.
    assert_eq!(
        d(
            vec![node("a", "Mouse", "HID"), node("a", "Bluetooth", "BTHLE")],
            vec![],
            "MX Master 3S"
        ),
        Some(Control::Link)
    );
    // Wired and receiver devices: nothing.
    assert_eq!(
        d(
            vec![node("a", "Mouse", "HID"), node("a", "USB", "USB")],
            vec![],
            "G502"
        ),
        None
    );
}

#[test]
fn one_action_per_device_in_a_while() {
    let t = Throttle::default();
    let now = Instant::now();
    assert!(t.allow("a", now));
    assert!(!t.allow("a", now + Duration::from_secs(1)));
    assert!(t.allow("b", now + Duration::from_secs(1)));
    assert!(t.allow("a", now + Throttle::GAP));
}
