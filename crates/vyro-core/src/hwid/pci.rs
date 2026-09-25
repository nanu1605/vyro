//! PCI identifiers and driver-version formatting helpers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciId {
    pub vendor: u16,
    pub device: u16,
    pub subsys: Option<u32>,
    pub revision: Option<u8>,
}

fn field<'a>(upper: &'a str, key: &str) -> Option<&'a str> {
    let start = upper.find(key)? + key.len();
    let rest = &upper[start..];
    Some(rest.split(['&', '\\']).next().unwrap_or(rest))
}

/// Parses a Windows PCI hardware id such as `PCI\VEN_10DE&DEV_2786&SUBSYS_51411458&REV_A1`
/// (case-insensitive).
pub fn parse_hardware_id(id: &str) -> Option<PciId> {
    let up = id.to_ascii_uppercase();
    Some(PciId {
        vendor: u16::from_str_radix(field(&up, "VEN_")?, 16).ok()?,
        device: u16::from_str_radix(field(&up, "DEV_")?, 16).ok()?,
        subsys: field(&up, "SUBSYS_").and_then(|s| u32::from_str_radix(s, 16).ok()),
        revision: field(&up, "REV_").and_then(|s| u8::from_str_radix(s, 16).ok()),
    })
}

pub fn vendor_name(vendor_id: u32) -> Option<&'static str> {
    Some(match vendor_id {
        0x10DE => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x8086 => "Intel",
        0x1414 => "Microsoft",
        0x5143 => "Qualcomm",
        0x1AF4 => "Red Hat (virtio)",
        0x15AD => "VMware",
        0x80EE => "VirtualBox",
        _ => return None,
    })
}

/// Text part of a registry `DeviceDesc`, e.g. `@oem12.inf,%dev%;Intel(R) … Controller` →
/// `Intel(R) … Controller`.
pub fn device_desc_text(desc: &str) -> &str {
    desc.rsplit(';').next().unwrap_or(desc).trim()
}

/// Chipset name from an LPC / eSPI bridge description, e.g.
/// "Intel(R) Z790 Chipset LPC/eSPI Controller - 7A04" → "Intel Z790".
pub fn chipset_from_description(desc: &str) -> Option<String> {
    let text = device_desc_text(desc)
        .replace("(R)", "")
        .replace("(TM)", "");
    let words: Vec<&str> = text.split_whitespace().collect();
    let pos = words
        .iter()
        .position(|w| w.eq_ignore_ascii_case("chipset"))?;
    if pos == 0 {
        return None;
    }
    let vendor = words[0];
    let model = words[1..pos].join(" ");
    if model.is_empty() {
        return None;
    }
    Some(format!("{vendor} {model}"))
}

/// Formats the user-mode driver version DXGI reports as a packed `u64`.
pub fn format_umd_version(v: u64) -> String {
    format!(
        "{}.{}.{}.{}",
        v >> 48,
        v >> 32 & 0xFFFF,
        v >> 16 & 0xFFFF,
        v & 0xFFFF
    )
}

/// NVIDIA's public version from the Windows driver version: the last five digits of the last
/// two fields, e.g. "32.0.15.6094" → "560.94".
pub fn nvidia_driver_version(windows_version: &str) -> Option<String> {
    let parts: Vec<&str> = windows_version.trim().split('.').collect();
    let [.., third, fourth] = parts.as_slice() else {
        return None;
    };
    let fourth: u32 = fourth.parse().ok()?;
    let digits = format!("{}{:04}", third.parse::<u32>().ok()?, fourth);
    let last5 = digits.get(digits.len().checked_sub(5)?..)?;
    Some(format!("{}.{}", &last5[..3], &last5[3..]))
}

/// Decodes a registry string that may be stored as `REG_BINARY` UTF-16LE or ASCII.
pub fn decode_reg_text(bytes: &[u8]) -> Option<String> {
    let looks_utf16 = bytes.len() >= 2
        && bytes.len().is_multiple_of(2)
        && bytes.iter().skip(1).step_by(2).all(|&b| b == 0);
    let s = if looks_utf16 {
        let wide: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&wide)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    let s = s.trim_matches(char::from(0)).trim().to_owned();
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hardware_ids() {
        let id = parse_hardware_id(r"PCI\VEN_10DE&DEV_2786&SUBSYS_51411458&REV_A1").unwrap();
        assert_eq!(
            (id.vendor, id.device, id.subsys, id.revision),
            (0x10DE, 0x2786, Some(0x5141_1458), Some(0xA1))
        );
        let id = parse_hardware_id(r"pci\ven_1002&dev_744c").unwrap();
        assert_eq!((id.vendor, id.device, id.subsys), (0x1002, 0x744C, None));
        assert!(parse_hardware_id("USB\\VID_046D&PID_C52B").is_none());
    }

    #[test]
    fn chipset_names() {
        assert_eq!(
            chipset_from_description("@oem34.inf,%pci\\ven_8086&dev_7a04%;Intel(R) Z790 Chipset LPC/eSPI Controller - 7A04")
                .as_deref(),
            Some("Intel Z790")
        );
        assert_eq!(
            chipset_from_description("Intel(R) 600 Series Chipset Family LPC Controller")
                .as_deref(),
            Some("Intel 600 Series")
        );
        assert_eq!(chipset_from_description("PCI standard ISA bridge"), None);
    }

    #[test]
    fn driver_versions() {
        assert_eq!(format_umd_version(0x0020_0000_000F_17D0), "32.0.15.6096");
        assert_eq!(
            nvidia_driver_version("32.0.15.6094").as_deref(),
            Some("560.94")
        );
        assert_eq!(
            nvidia_driver_version("31.0.15.3623").as_deref(),
            Some("536.23")
        );
        assert_eq!(nvidia_driver_version("garbage"), None);
    }

    #[test]
    fn registry_text_in_both_encodings() {
        let wide: Vec<u8> = "94.04.1F.00.2D\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(decode_reg_text(&wide).as_deref(), Some("94.04.1F.00.2D"));
        assert_eq!(decode_reg_text(b"GDDR6X\0").as_deref(), Some("GDDR6X"));
        assert_eq!(decode_reg_text(b"\0\0"), None);
    }
}
