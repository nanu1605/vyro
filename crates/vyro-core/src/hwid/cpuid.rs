//! CPUID decoding. Reading CPUID needs no privileges and works on any x86-64 OS; decoding is
//! separated from reading so it can be tested against fixture leaves.

/// Registers returned by one CPUID query: `[eax, ebx, ecx, edx]`.
pub type Regs = [u32; 4];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CpuidInfo {
    pub vendor: String,
    pub brand: Option<String>,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub isa: Vec<String>,
    pub hybrid: bool,
    pub hypervisor_present: bool,
    /// VT-x / AMD-V supported by the CPU (not necessarily enabled in firmware).
    pub virtualization_capable: bool,
    /// Intel leaf 0x16: base / max frequency.
    pub base_mhz: Option<u32>,
    pub max_mhz: Option<u32>,
}

const fn bit(reg: u32, n: u32) -> bool {
    reg >> n & 1 == 1
}

fn regs_to_ascii(regs: &[u32]) -> String {
    let bytes: Vec<u8> = regs.iter().flat_map(|r| r.to_le_bytes()).collect();
    String::from_utf8_lossy(&bytes)
        .trim_matches(char::from(0))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Decodes the CPU identity from a CPUID reader `(leaf, subleaf) -> regs`.
pub fn decode(read: impl Fn(u32, u32) -> Regs) -> CpuidInfo {
    let [max_leaf, b, c, d] = read(0, 0);
    let vendor = regs_to_ascii(&[b, d, c]);

    let mut info = CpuidInfo {
        vendor,
        ..Default::default()
    };
    if max_leaf < 1 {
        return info;
    }

    let [sig, _, ecx1, edx1] = read(1, 0);
    let base_family = sig >> 8 & 0xF;
    let base_model = sig >> 4 & 0xF;
    info.stepping = sig & 0xF;
    info.family = if base_family == 0xF {
        base_family + (sig >> 20 & 0xFF)
    } else {
        base_family
    };
    info.model = if base_family == 0x6 || base_family == 0xF {
        base_model + ((sig >> 16 & 0xF) << 4)
    } else {
        base_model
    };
    info.hypervisor_present = bit(ecx1, 31);

    let (ebx7, ecx7, edx7, eax7_1) = if max_leaf >= 7 {
        let [_, b7, c7, d7] = read(7, 0);
        let [a71, ..] = read(7, 1);
        (b7, c7, d7, a71)
    } else {
        (0, 0, 0, 0)
    };
    info.hybrid = bit(edx7, 15);

    if max_leaf >= 0x16 {
        let [base, max, ..] = read(0x16, 0);
        info.base_mhz = (base & 0xFFFF != 0).then_some(base & 0xFFFF);
        info.max_mhz = (max & 0xFFFF != 0).then_some(max & 0xFFFF);
    }

    let [max_ext, ..] = read(0x8000_0000, 0);
    let (ecx_e1, edx_e1) = if max_ext >= 0x8000_0001 {
        let [_, _, c, d] = read(0x8000_0001, 0);
        (c, d)
    } else {
        (0, 0)
    };
    if max_ext >= 0x8000_0004 {
        let regs: Vec<u32> = (0x8000_0002..=0x8000_0004)
            .flat_map(|l| read(l, 0))
            .collect();
        let brand = regs_to_ascii(&regs);
        info.brand = (!brand.is_empty()).then_some(brand);
    }

    let vmx = bit(ecx1, 5);
    let svm = bit(ecx_e1, 2);
    info.virtualization_capable = vmx || svm;

    let features: &[(&str, bool)] = &[
        ("x86-64", bit(edx_e1, 29)),
        ("SSE2", bit(edx1, 26)),
        ("SSE3", bit(ecx1, 0)),
        ("SSSE3", bit(ecx1, 9)),
        ("SSE4.1", bit(ecx1, 19)),
        ("SSE4.2", bit(ecx1, 20)),
        ("SSE4a", bit(ecx_e1, 6)),
        ("AVX", bit(ecx1, 28)),
        ("AVX2", bit(ebx7, 5)),
        ("AVX-VNNI", bit(eax7_1, 4)),
        ("AVX-512F", bit(ebx7, 16)),
        ("AVX-512DQ", bit(ebx7, 17)),
        ("AVX-512CD", bit(ebx7, 28)),
        ("AVX-512BW", bit(ebx7, 30)),
        ("AVX-512VL", bit(ebx7, 31)),
        ("AVX-512VNNI", bit(ecx7, 11)),
        ("AVX-512FP16", bit(edx7, 23)),
        ("AMX", bit(edx7, 24)),
        ("FMA3", bit(ecx1, 12)),
        ("F16C", bit(ecx1, 29)),
        ("BMI1", bit(ebx7, 3)),
        ("BMI2", bit(ebx7, 8)),
        ("AES", bit(ecx1, 25)),
        ("VAES", bit(ecx7, 9)),
        ("SHA", bit(ebx7, 29)),
        ("RDRAND", bit(ecx1, 30)),
        ("RDSEED", bit(ebx7, 18)),
        ("VT-x", vmx),
        ("AMD-V", svm),
    ];
    info.isa = features
        .iter()
        .filter(|(_, present)| *present)
        .map(|(name, _)| (*name).to_owned())
        .collect();
    info
}

/// Reads CPUID on the current CPU.
#[cfg(target_arch = "x86_64")]
pub fn read_live() -> CpuidInfo {
    decode(|leaf, sub| {
        let r = core::arch::x86_64::__cpuid_count(leaf, sub);
        [r.eax, r.ebx, r.ecx, r.edx]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn ascii_regs(s: &str, n: usize) -> Vec<u32> {
        let mut bytes = s.as_bytes().to_vec();
        bytes.resize(n * 4, 0);
        bytes
            .chunks(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    /// Builds a leaf table for a CPU with the given vendor, brand and signature.
    fn fixture(vendor: &str, brand: &str, sig: u32, max_leaf: u32) -> HashMap<(u32, u32), Regs> {
        let v = ascii_regs(vendor, 3); // ebx, edx, ecx order
        let b = ascii_regs(brand, 12);
        let mut m = HashMap::new();
        m.insert((0, 0), [max_leaf, v[0], v[2], v[1]]);
        m.insert((0x8000_0000, 0), [0x8000_0008, 0, 0, 0]);
        m.insert((0x8000_0001, 0), [0, 0, 0, 1 << 29]);
        for i in 0..3u32 {
            let k = (i * 4) as usize;
            m.insert((0x8000_0002 + i, 0), [b[k], b[k + 1], b[k + 2], b[k + 3]]);
        }
        m.insert((1, 0), [sig, 0, 0, 0]);
        m
    }

    fn reader(m: HashMap<(u32, u32), Regs>) -> impl Fn(u32, u32) -> Regs {
        move |l, s| m.get(&(l, s)).copied().unwrap_or_default()
    }

    #[test]
    fn decodes_intel_hybrid_signature_brand_and_features() {
        let mut m = fixture(
            "GenuineIntel",
            "13th Gen Intel(R) Core(TM) i7-13700K",
            0x000B_0671,
            0x20,
        );
        // leaf 1: SSE3, SSSE3, FMA, SSE4.1, SSE4.2, AES, AVX, VMX, hypervisor; SSE2 in edx
        let ecx1 = 1 | 1 << 5 | 1 << 9 | 1 << 12 | 1 << 19 | 1 << 20 | 1 << 25 | 1 << 28 | 1 << 31;
        m.insert((1, 0), [0x000B_0671, 0, ecx1, 1 << 26]);
        m.insert((7, 0), [0, 1 << 3 | 1 << 5 | 1 << 8 | 1 << 29, 0, 1 << 15]);
        m.insert((7, 1), [1 << 4, 0, 0, 0]);
        m.insert((0x16, 0), [3400, 5400, 100, 0]);
        let info = decode(reader(m));

        assert_eq!(info.vendor, "GenuineIntel");
        assert_eq!(
            info.brand.as_deref(),
            Some("13th Gen Intel(R) Core(TM) i7-13700K")
        );
        assert_eq!((info.family, info.model, info.stepping), (6, 0xB7, 1));
        assert!(info.hybrid);
        assert!(info.hypervisor_present);
        assert!(info.virtualization_capable);
        assert_eq!((info.base_mhz, info.max_mhz), (Some(3400), Some(5400)));
        for f in [
            "x86-64", "SSE4.2", "AVX", "AVX2", "AVX-VNNI", "FMA3", "BMI2", "AES", "SHA", "VT-x",
        ] {
            assert!(
                info.isa.iter().any(|i| i == f),
                "missing {f}: {:?}",
                info.isa
            );
        }
        assert!(!info.isa.iter().any(|i| i.starts_with("AVX-512")));
    }

    #[test]
    fn decodes_amd_extended_family_and_svm() {
        // Zen 4 (Raphael): family 0x19, model 0x61, stepping 2.
        let mut m = fixture(
            "AuthenticAMD",
            "AMD Ryzen 7 7800X3D 8-Core Processor",
            0x00A6_0F12,
            0x10,
        );
        m.insert((0x8000_0001, 0), [0, 0, 1 << 2 | 1 << 6, 1 << 29]);
        let info = decode(reader(m));
        assert_eq!(info.vendor, "AuthenticAMD");
        assert_eq!((info.family, info.model, info.stepping), (0x19, 0x61, 2));
        assert!(info.isa.iter().any(|i| i == "AMD-V"));
        assert!(info.isa.iter().any(|i| i == "SSE4a"));
        assert!(!info.hybrid);
        assert_eq!(info.base_mhz, None, "leaf 0x16 is beyond max leaf");
    }

    #[test]
    fn brand_whitespace_is_normalized() {
        let m = fixture(
            "GenuineIntel",
            "   Intel(R)  Xeon(R)   CPU  ",
            0x0005_0654,
            0x16,
        );
        assert_eq!(
            decode(reader(m)).brand.as_deref(),
            Some("Intel(R) Xeon(R) CPU")
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn live_read_reports_this_cpu() {
        let info = read_live();
        assert!(!info.vendor.is_empty());
        assert!(info.isa.iter().any(|i| i == "x86-64"));
        assert!(info.isa.iter().any(|i| i == "SSE2"));
    }
}
