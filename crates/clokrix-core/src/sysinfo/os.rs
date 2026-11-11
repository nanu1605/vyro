//! OS naming quirks.

/// Windows 11 still reports "Windows 10 …" in `ProductName`; builds ≥ 22000 are Windows 11.
pub fn os_name(product_name: &str, build: u32) -> String {
    let name = product_name.trim();
    match name.strip_prefix("Windows 10") {
        Some(rest) if build >= 22_000 => format!("Windows 11{rest}"),
        _ => name.to_owned(),
    }
}

/// "26100" + UBR 2033 → "26100.2033".
pub fn build_string(build: &str, ubr: Option<u32>) -> String {
    match ubr {
        Some(ubr) => format!("{}.{ubr}", build.trim()),
        None => build.trim().to_owned(),
    }
}

/// `SYSTEM_INFO.wProcessorArchitecture` → display name.
pub fn architecture_name(code: u16) -> Option<&'static str> {
    Some(match code {
        9 => "x64",
        12 => "ARM64",
        0 => "x86",
        5 => "ARM",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_11_is_named_from_the_build() {
        assert_eq!(os_name("Windows 10 Pro", 26100), "Windows 11 Pro");
        assert_eq!(os_name("Windows 10 Pro", 19045), "Windows 10 Pro");
        assert_eq!(
            os_name("Windows Server 2022 Datacenter", 20348),
            "Windows Server 2022 Datacenter"
        );
    }

    #[test]
    fn build_and_arch() {
        assert_eq!(build_string("26100", Some(2033)), "26100.2033");
        assert_eq!(build_string("19045", None), "19045");
        assert_eq!(architecture_name(9), Some("x64"));
    }
}
