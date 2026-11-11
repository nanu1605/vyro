//! `\GPU Engine(*)\Utilization Percentage` decoding, aggregated the way Task Manager does it:
//! an engine's load is the sum over the processes using it, an adapter's load is its busiest
//! engine, and a process's GPU load is its busiest engine.

use std::collections::HashMap;

use crate::snapshot::GpuEngines;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineKind {
    Graphics3d,
    Decode,
    Encode,
    Compute,
    Copy,
    Other,
}

/// Maps a WDDM engine type name (`3D`, `VideoDecode`, `Compute_0`, `Cuda`, …) to a kind.
pub fn engine_kind(engtype: &str) -> EngineKind {
    let t = engtype.to_ascii_lowercase().replace(' ', "");
    if t.starts_with("3d") || t.starts_with("graphics") {
        EngineKind::Graphics3d
    } else if t.starts_with("videodecode") || t.starts_with("decode") {
        EngineKind::Decode
    } else if t.starts_with("videoencode") || t.starts_with("encode") {
        EngineKind::Encode
    } else if t.starts_with("compute") || t.starts_with("cuda") {
        EngineKind::Compute
    } else if t.starts_with("copy") {
        EngineKind::Copy
    } else {
        EngineKind::Other
    }
}

/// An adapter LUID as one number: high part in the upper 32 bits.
pub fn luid_key(high: i32, low: u32) -> u64 {
    (u64::from(high as u32) << 32) | u64::from(low)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineInstance {
    pub pid: u32,
    pub luid: u64,
    pub phys: u32,
    pub eng: u32,
    pub kind: EngineKind,
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.strip_prefix("0x").or(s.strip_prefix("0X"))?, 16).ok()
}

/// Parses `luid_0x00000000_0x0000EAF2_phys_0` (the tail of engine instances, and the whole of
/// `GPU Adapter Memory` instances) into `(luid, phys)`.
pub fn parse_adapter_instance(name: &str) -> Option<(u64, u32)> {
    let rest = &name[name.find("luid_")? + 5..];
    let mut it = rest.splitn(4, '_');
    let high = hex(it.next()?)?;
    let low = hex(it.next()?)?;
    if it.next()? != "phys" {
        return None;
    }
    let phys = it.next()?.split('_').next()?.parse().ok()?;
    Some((luid_key(high as i32, low), phys))
}

/// Parses `pid_1234_luid_0x00000000_0x0000EAF2_phys_0_eng_3_engtype_VideoDecode`.
pub fn parse_engine_instance(name: &str) -> Option<EngineInstance> {
    let pid = name.strip_prefix("pid_")?.split('_').next()?.parse().ok()?;
    let (luid, phys) = parse_adapter_instance(name)?;
    let eng_at = name.find("_eng_")? + 5;
    let eng = name[eng_at..].split('_').next()?.parse().ok()?;
    let engtype = &name[name.find("_engtype_")? + 9..];
    Some(EngineInstance {
        pid,
        luid,
        phys,
        eng,
        kind: engine_kind(engtype),
    })
}

/// Utilization of one adapter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdapterUsage {
    /// Busiest engine, %.
    pub usage: f64,
    pub engines: GpuEngines,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineSummary {
    pub adapters: HashMap<u64, AdapterUsage>,
    /// Busiest engine per process across all adapters, %.
    pub per_pid: HashMap<u32, f64>,
}

/// Aggregates `(instance name, utilization %)` samples. Unparseable names are skipped.
pub fn summarize<'a>(samples: impl IntoIterator<Item = (&'a str, f64)>) -> EngineSummary {
    let mut engines: HashMap<(u64, u32, u32), (EngineKind, f64)> = HashMap::new();
    let mut per_pid: HashMap<u32, f64> = HashMap::new();
    for (name, value) in samples {
        let Some(inst) = parse_engine_instance(name) else {
            continue;
        };
        let value = if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        };
        engines
            .entry((inst.luid, inst.phys, inst.eng))
            .or_insert((inst.kind, 0.0))
            .1 += value;
        if inst.pid != 0 {
            let p = per_pid.entry(inst.pid).or_insert(0.0);
            *p = p.max(value.min(100.0));
        }
    }

    let mut adapters: HashMap<u64, AdapterUsage> = HashMap::new();
    for ((luid, _, _), (kind, sum)) in engines {
        let v = sum.min(100.0);
        let a = adapters.entry(luid).or_insert_with(|| AdapterUsage {
            usage: 0.0,
            engines: GpuEngines {
                graphics3d: Some(0.0),
                decode: Some(0.0),
                encode: Some(0.0),
                compute: Some(0.0),
                copy: Some(0.0),
                tensor: None,
                raytrace: None,
            },
        });
        a.usage = a.usage.max(v);
        let slot = match kind {
            EngineKind::Graphics3d => &mut a.engines.graphics3d,
            EngineKind::Decode => &mut a.engines.decode,
            EngineKind::Encode => &mut a.engines.encode,
            EngineKind::Compute => &mut a.engines.compute,
            EngineKind::Copy => &mut a.engines.copy,
            EngineKind::Other => continue,
        };
        *slot = Some(slot.unwrap_or(0.0).max(v));
    }
    EngineSummary { adapters, per_pid }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NV: u64 = 0x0000_EAF2;

    #[test]
    fn parses_instance_names() {
        let i = parse_engine_instance(
            "pid_1234_luid_0x00000000_0x0000EAF2_phys_0_eng_3_engtype_VideoDecode",
        )
        .unwrap();
        assert_eq!(
            (i.pid, i.luid, i.phys, i.eng, i.kind),
            (1234, NV, 0, 3, EngineKind::Decode)
        );
        let i = parse_engine_instance(
            "pid_8_luid_0x00000001_0x00012345_phys_0_eng_5_engtype_Compute_1",
        )
        .unwrap();
        assert_eq!(i.luid, 0x1_0001_2345);
        assert_eq!(i.kind, EngineKind::Compute);
        assert_eq!(
            parse_adapter_instance("luid_0x00000000_0x0000EAF2_phys_0"),
            Some((NV, 0))
        );
        assert!(parse_engine_instance("_Total").is_none());
        assert!(parse_engine_instance("pid_x_luid_0x0_0x1_phys_0_eng_0_engtype_3D").is_none());
    }

    #[test]
    fn engine_kinds() {
        assert_eq!(engine_kind("3D"), EngineKind::Graphics3d);
        assert_eq!(engine_kind("Graphics_1"), EngineKind::Graphics3d);
        assert_eq!(engine_kind("Video Encode"), EngineKind::Encode);
        assert_eq!(engine_kind("Cuda"), EngineKind::Compute);
        assert_eq!(engine_kind("Copy"), EngineKind::Copy);
        assert_eq!(engine_kind("Security"), EngineKind::Other);
    }

    #[test]
    fn aggregates_like_task_manager() {
        let s = summarize([
            // two processes share the 3D engine → summed
            (
                "pid_10_luid_0x00000000_0x0000EAF2_phys_0_eng_0_engtype_3D",
                30.0,
            ),
            (
                "pid_20_luid_0x00000000_0x0000EAF2_phys_0_eng_0_engtype_3D",
                25.0,
            ),
            // pid 20 also decodes
            (
                "pid_20_luid_0x00000000_0x0000EAF2_phys_0_eng_3_engtype_VideoDecode",
                40.0,
            ),
            // over-reporting clamps to 100
            (
                "pid_30_luid_0x00000000_0x0000EAF2_phys_0_eng_5_engtype_Copy",
                70.0,
            ),
            (
                "pid_31_luid_0x00000000_0x0000EAF2_phys_0_eng_5_engtype_Copy",
                60.0,
            ),
            // second adapter, idle
            (
                "pid_10_luid_0x00000000_0x00001111_phys_0_eng_0_engtype_3D",
                0.0,
            ),
            ("garbage", 99.0),
        ]);
        let nv = &s.adapters[&NV];
        assert_eq!(nv.engines.graphics3d, Some(55.0));
        assert_eq!(nv.engines.decode, Some(40.0));
        assert_eq!(nv.engines.copy, Some(100.0));
        assert_eq!(nv.engines.encode, Some(0.0));
        assert_eq!(nv.engines.tensor, None);
        assert_eq!(nv.usage, 100.0);
        assert_eq!(s.adapters[&0x1111].usage, 0.0);
        assert_eq!(s.per_pid[&10], 30.0);
        assert_eq!(s.per_pid[&20], 40.0);
        assert_eq!(s.per_pid[&30], 70.0);
    }
}
