//! Live usage (Phase 2): CPU, GPU engines and per-process readings. The decoders here are
//! platform-neutral and fixture-tested; the Windows providers that feed them live in `live`.

pub mod engines;
pub mod gpu;
pub mod process;
pub mod vendor;

/// Clamps a utilization reading to 0–100 %. `% Processor Utility` legitimately exceeds 100
/// while boosting; Task Manager caps it too.
pub fn clamp_pct(v: f64) -> f64 {
    if v.is_finite() {
        v.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

/// Parses a `Processor Information` instance name such as `"0,3"` (group, number). `_Total`
/// and per-group totals (`"0,_Total"`) return `None`.
pub fn parse_processor_instance(name: &str) -> Option<(u32, u32)> {
    let (group, number) = name.split_once(',')?;
    Some((group.trim().parse().ok()?, number.trim().parse().ok()?))
}

/// Per-logical-processor values ordered by (group, number), from `(instance, value)` pairs.
pub fn per_processor<'a>(items: impl IntoIterator<Item = (&'a str, f64)>) -> Vec<f64> {
    let mut v: Vec<((u32, u32), f64)> = items
        .into_iter()
        .filter_map(|(name, value)| Some((parse_processor_instance(name)?, value)))
        .collect();
    v.sort_by_key(|(k, _)| *k);
    v.into_iter().map(|(_, value)| value).collect()
}

/// Effective clock from `% Processor Performance` and the nominal `Processor Frequency`, the
/// same way Task Manager computes "Speed".
pub fn effective_mhz(performance_pct: f64, nominal_mhz: f64) -> Option<f64> {
    let mhz = nominal_mhz * performance_pct / 100.0;
    (mhz.is_finite() && mhz > 0.0).then_some(mhz)
}

/// Sums `SYSTEM_PAGEFILE_INFORMATION` records (x64 layout, 32-byte fixed part) into
/// (total pages, pages in use). An empty buffer means no page file.
pub fn pagefile_pages(buf: &[u8]) -> (u64, u64) {
    let u32_at =
        |o: usize| -> Option<u32> { Some(u32::from_le_bytes(buf.get(o..o + 4)?.try_into().ok()?)) };
    let (mut total, mut used, mut off) = (0u64, 0u64, 0usize);
    while let (Some(next), Some(size), Some(in_use)) =
        (u32_at(off), u32_at(off + 4), u32_at(off + 8))
    {
        total += u64::from(size);
        used += u64::from(in_use);
        if next == 0 {
            break;
        }
        off += next as usize;
    }
    (total, used)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processor_instances() {
        assert_eq!(parse_processor_instance("0,3"), Some((0, 3)));
        assert_eq!(parse_processor_instance("1,12"), Some((1, 12)));
        assert_eq!(parse_processor_instance("0,_Total"), None);
        assert_eq!(parse_processor_instance("_Total"), None);
        let v = per_processor([
            ("0,10", 10.0),
            ("_Total", 50.0),
            ("0,2", 2.0),
            ("1,0", 100.0),
            ("0,_Total", 40.0),
            ("0,1", 1.0),
        ]);
        assert_eq!(v, vec![1.0, 2.0, 10.0, 100.0]);
    }

    #[test]
    fn clamps_and_derives_clock() {
        assert_eq!(clamp_pct(123.4), 100.0);
        assert_eq!(clamp_pct(-1.0), 0.0);
        assert_eq!(clamp_pct(f64::NAN), 0.0);
        assert_eq!(effective_mhz(120.0, 3800.0), Some(4560.0));
        assert_eq!(effective_mhz(0.0, 3800.0), None);
    }

    #[test]
    fn sums_page_files() {
        let mut b = vec![0u8; 80];
        b[0..4].copy_from_slice(&48u32.to_le_bytes());
        b[4..8].copy_from_slice(&1000u32.to_le_bytes());
        b[8..12].copy_from_slice(&200u32.to_le_bytes());
        b[52..56].copy_from_slice(&500u32.to_le_bytes());
        b[56..60].copy_from_slice(&50u32.to_le_bytes());
        assert_eq!(pagefile_pages(&b), (1500, 250));
        assert_eq!(pagefile_pages(&[]), (0, 0));
    }
}
