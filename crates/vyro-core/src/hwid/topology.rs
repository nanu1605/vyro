//! Decoder for the buffer returned by `GetLogicalProcessorInformationEx(RelationAll)`: a packed
//! sequence of variable-length `SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX` records.
//!
//! Record-relative offsets used below are asserted against the `windows` crate's struct layouts
//! at compile time in `hwid::win`.

use crate::info::{CacheInfo, CacheKind, CoreInfo, CoreKind};

pub const RELATION_PROCESSOR_CORE: u32 = 0;
pub const RELATION_CACHE: u32 = 2;
pub const RELATION_PROCESSOR_PACKAGE: u32 = 3;

/// `Relationship` (u32) + `Size` (u32).
pub const HEADER: usize = 8;
// PROCESSOR_RELATIONSHIP, relative to the record start.
pub const PROC_EFFICIENCY_CLASS: usize = HEADER + 1;
pub const PROC_GROUP_COUNT: usize = HEADER + 22;
pub const PROC_GROUP_MASK: usize = HEADER + 24;
// CACHE_RELATIONSHIP, relative to the record start.
pub const CACHE_LEVEL: usize = HEADER;
pub const CACHE_ASSOCIATIVITY: usize = HEADER + 1;
pub const CACHE_LINE_SIZE: usize = HEADER + 2;
pub const CACHE_SIZE: usize = HEADER + 4;
pub const CACHE_TYPE: usize = HEADER + 8;
pub const CACHE_GROUP_MASK: usize = HEADER + 32;
/// `GROUP_AFFINITY`: `Mask` (usize) + `Group` (u16) + 3 reserved u16.
pub const GROUP_AFFINITY_SIZE: usize = 16;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Topology {
    pub packages: u32,
    pub cores: Vec<CoreInfo>,
    pub caches: Vec<CacheInfo>,
    pub threads: u32,
    pub performance_cores: Option<u32>,
    pub efficiency_cores: Option<u32>,
}

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(off..off + 8)?.try_into().ok()?))
}

/// Logical processor numbers named by a `GROUP_AFFINITY` at `off`.
fn logical_cpus(rec: &[u8], off: usize) -> Vec<u32> {
    let (Some(mask), Some(group)) = (u64_at(rec, off), u16_at(rec, off + 8)) else {
        return Vec::new();
    };
    (0..64)
        .filter(|bit| mask >> bit & 1 == 1)
        .map(|bit| u32::from(group) * 64 + bit)
        .collect()
}

pub fn parse(buf: &[u8]) -> Topology {
    let mut t = Topology::default();
    // (efficiency class, logical cpus) per core, in OS order.
    let mut cores: Vec<(u8, Vec<u32>)> = Vec::new();
    let mut caches: Vec<CacheInfo> = Vec::new();

    let mut off = 0;
    while off + HEADER <= buf.len() {
        let (Some(rel), Some(size)) = (u32_at(buf, off), u32_at(buf, off + 4)) else {
            break;
        };
        let size = size as usize;
        if size < HEADER || off + size > buf.len() {
            break;
        }
        let rec = &buf[off..off + size];
        match rel {
            RELATION_PROCESSOR_CORE => {
                let class = rec.get(PROC_EFFICIENCY_CLASS).copied().unwrap_or(0);
                let groups = u16_at(rec, PROC_GROUP_COUNT).unwrap_or(0) as usize;
                let cpus = (0..groups)
                    .flat_map(|g| logical_cpus(rec, PROC_GROUP_MASK + g * GROUP_AFFINITY_SIZE))
                    .collect();
                cores.push((class, cpus));
            }
            RELATION_PROCESSOR_PACKAGE => t.packages += 1,
            RELATION_CACHE => {
                let kind = match u32_at(rec, CACHE_TYPE) {
                    Some(0) => CacheKind::Unified,
                    Some(1) => CacheKind::Instruction,
                    Some(2) => CacheKind::Data,
                    Some(3) => CacheKind::Trace,
                    _ => {
                        off += size;
                        continue;
                    }
                };
                let level = rec.get(CACHE_LEVEL).copied().unwrap_or(0);
                let size_kb = u32_at(rec, CACHE_SIZE).unwrap_or(0) / 1024;
                let ways = rec.get(CACHE_ASSOCIATIVITY).copied().map(u32::from);
                // 0xFF means fully associative; report as unknown rather than 255 ways.
                let ways = ways.filter(|&w| w != 0 && w != 0xFF);
                let line_bytes = u32::from(u16_at(rec, CACHE_LINE_SIZE).unwrap_or(0));
                match caches.iter_mut().find(|c| {
                    c.level == level && c.kind == kind && c.size_kb == size_kb && c.ways == ways
                }) {
                    Some(c) => c.count += 1,
                    None => caches.push(CacheInfo {
                        level,
                        kind,
                        size_kb,
                        ways,
                        line_bytes,
                        count: 1,
                    }),
                }
            }
            _ => {}
        }
        off += size;
    }

    // Hybrid CPUs report a higher efficiency class for performance cores.
    let max_class = cores.iter().map(|(c, _)| *c).max().unwrap_or(0);
    let hybrid = cores.iter().any(|(c, _)| *c != max_class);
    t.cores = cores
        .into_iter()
        .enumerate()
        .map(|(i, (class, logical))| CoreInfo {
            index: i as u32,
            kind: match (hybrid, class == max_class) {
                (false, _) => CoreKind::Uniform,
                (true, true) => CoreKind::Performance,
                (true, false) => CoreKind::Efficiency,
            },
            logical,
        })
        .collect();
    t.threads = t.cores.iter().map(|c| c.logical.len() as u32).sum();
    if hybrid {
        let p = t
            .cores
            .iter()
            .filter(|c| c.kind == CoreKind::Performance)
            .count() as u32;
        t.performance_cores = Some(p);
        t.efficiency_cores = Some(t.cores.len() as u32 - p);
    }
    caches.sort_by_key(|c| (c.level, c.kind as u8));
    t.caches = caches;
    t
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::*;

    fn record(rel: u32, body_len: usize) -> Vec<u8> {
        let size = HEADER + body_len;
        let mut r = vec![0u8; size];
        r[0..4].copy_from_slice(&rel.to_le_bytes());
        r[4..8].copy_from_slice(&u32::try_from(size).unwrap().to_le_bytes());
        r
    }

    fn put_affinity(r: &mut [u8], off: usize, mask: u64, group: u16) {
        r[off..off + 8].copy_from_slice(&mask.to_le_bytes());
        r[off + 8..off + 10].copy_from_slice(&group.to_le_bytes());
    }

    pub fn core(class: u8, mask: u64) -> Vec<u8> {
        let mut r = record(RELATION_PROCESSOR_CORE, 24 + GROUP_AFFINITY_SIZE);
        r[PROC_EFFICIENCY_CLASS] = class;
        r[PROC_GROUP_COUNT..PROC_GROUP_COUNT + 2].copy_from_slice(&1u16.to_le_bytes());
        put_affinity(&mut r, PROC_GROUP_MASK, mask, 0);
        r
    }

    pub fn package(mask: u64) -> Vec<u8> {
        let mut r = core(0, mask);
        r[0..4].copy_from_slice(&RELATION_PROCESSOR_PACKAGE.to_le_bytes());
        r
    }

    pub fn cache(level: u8, kind: u32, size: u32, ways: u8, mask: u64) -> Vec<u8> {
        let mut r = record(RELATION_CACHE, 32 + GROUP_AFFINITY_SIZE);
        r[CACHE_LEVEL] = level;
        r[CACHE_ASSOCIATIVITY] = ways;
        r[CACHE_LINE_SIZE..CACHE_LINE_SIZE + 2].copy_from_slice(&64u16.to_le_bytes());
        r[CACHE_SIZE..CACHE_SIZE + 4].copy_from_slice(&size.to_le_bytes());
        r[CACHE_TYPE..CACHE_TYPE + 4].copy_from_slice(&kind.to_le_bytes());
        put_affinity(&mut r, CACHE_GROUP_MASK, mask, 0);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;

    /// 2 P-cores with SMT + 4 E-cores, like a scaled-down Alder Lake.
    fn hybrid_buffer() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(core(1, 0b11));
        b.extend(cache(1, 2, 48 * 1024, 12, 0b11));
        b.extend(cache(1, 1, 32 * 1024, 8, 0b11));
        b.extend(core(1, 0b1100));
        b.extend(cache(1, 2, 48 * 1024, 12, 0b1100));
        b.extend(cache(1, 1, 32 * 1024, 8, 0b1100));
        for i in 0..4 {
            b.extend(core(0, 1 << (4 + i)));
        }
        b.extend(cache(2, 0, 2048 * 1024, 16, 0b1111_1111));
        b.extend(cache(3, 0, 24 * 1024 * 1024, 12, 0b1111_1111));
        b.extend(package(0b1111_1111));
        b
    }

    #[test]
    fn hybrid_cores_threads_and_p_e_split() {
        let t = parse(&hybrid_buffer());
        assert_eq!(t.packages, 1);
        assert_eq!(t.cores.len(), 6);
        assert_eq!(t.threads, 8);
        assert_eq!(
            (t.performance_cores, t.efficiency_cores),
            (Some(2), Some(4))
        );
        assert_eq!(t.cores[0].kind, CoreKind::Performance);
        assert_eq!(t.cores[0].logical, [0, 1]);
        assert_eq!(t.cores[5].kind, CoreKind::Efficiency);
        assert_eq!(t.cores[5].logical, [7]);
    }

    #[test]
    fn caches_are_aggregated_and_sorted() {
        let t = parse(&hybrid_buffer());
        let summary: Vec<_> = t
            .caches
            .iter()
            .map(|c| (c.level, c.kind, c.size_kb, c.count))
            .collect();
        assert_eq!(
            summary,
            [
                (1, CacheKind::Data, 48, 2),
                (1, CacheKind::Instruction, 32, 2),
                (2, CacheKind::Unified, 2048, 1),
                (3, CacheKind::Unified, 24 * 1024, 1),
            ]
        );
        assert_eq!(t.caches[0].line_bytes, 64);
        assert_eq!(t.caches[0].ways, Some(12));
    }

    #[test]
    fn uniform_cpu_is_not_hybrid() {
        let mut b = Vec::new();
        for i in 0..4 {
            b.extend(core(0, 0b11 << (2 * i)));
        }
        let t = parse(&b);
        assert!(t.cores.iter().all(|c| c.kind == CoreKind::Uniform));
        assert_eq!(t.threads, 8);
        assert_eq!(t.performance_cores, None);
    }

    #[test]
    fn second_processor_group_numbers_continue_past_64() {
        let mut r = core(0, 0b1);
        r[PROC_GROUP_MASK + 8..PROC_GROUP_MASK + 10].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(parse(&r).cores[0].logical, [64]);
    }

    #[test]
    fn truncated_or_corrupt_buffers_do_not_panic() {
        let b = hybrid_buffer();
        for cut in 0..b.len() {
            let _ = parse(&b[..cut]);
        }
        let mut bad = core(0, 1);
        bad[4..8].copy_from_slice(&3u32.to_le_bytes()); // size smaller than header
        assert!(parse(&bad).cores.is_empty());
    }
}
