//! CPU utilization and clock from PDH `Processor Information` counters, the same source as
//! Task Manager. Temperatures, power and voltages need the driver (Phase 6).

use windows::Win32::System::Power::{
    CallNtPowerInformation, ProcessorInformation, PROCESSOR_POWER_INFORMATION,
};
use windows::Win32::System::Threading::{GetActiveProcessorCount, ALL_PROCESSOR_GROUPS};

use super::pdh::{Counter, Query};
use super::PerCore;
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::SensorSnapshot;
use crate::usage::{clamp_pct, effective_mhz, per_processor};

struct Counters {
    // Declared before `query` so they drop first (they are freed with it anyway).
    usage_total: Counter,
    usage_all: Option<Counter>,
    perf_total: Option<Counter>,
    perf_all: Option<Counter>,
    nominal: Option<Counter>,
    query: Query,
}

struct Keys {
    usage: SensorKey,
    clock: SensorKey,
    core_usage: PerCore,
    core_clock: PerCore,
}

#[derive(Default)]
pub struct CpuProvider {
    counters: Option<Counters>,
    keys: Option<Keys>,
    base_mhz: Option<f64>,
    boost_mhz: Option<f64>,
}

impl CpuProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Logical processor count across all processor groups.
pub fn logical_processors() -> u32 {
    // SAFETY: plain query.
    unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) }.max(1)
}

/// Per-logical-processor clocks from the power manager: (max = nominal, current). Used when
/// the `% Processor Performance` counter is missing.
fn power_info() -> Vec<PROCESSOR_POWER_INFORMATION> {
    let n = logical_processors() as usize;
    let mut info = vec![PROCESSOR_POWER_INFORMATION::default(); n];
    let bytes = u32::try_from(std::mem::size_of_val(info.as_slice())).unwrap_or(u32::MAX);
    // SAFETY: the output buffer holds `bytes` writable bytes.
    let status = unsafe {
        CallNtPowerInformation(
            ProcessorInformation,
            None,
            0,
            Some(info.as_mut_ptr().cast()),
            bytes,
        )
    };
    if status.is_ok() {
        info
    } else {
        Vec::new()
    }
}

impl Provider for CpuProvider {
    fn name(&self) -> &'static str {
        "cpu"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        let query = Query::open()?;
        // `% Processor Utility` (Windows 8+) is what Task Manager shows; `% Processor Time`
        // is the fallback on older builds, and the legacy `Processor` object the last resort
        // (its instances are plain numbers, which `per_processor` does not read).
        let usage = |inst: &str| {
            query
                .add(&format!(
                    r"\Processor Information({inst})\% Processor Utility"
                ))
                .or_else(|_| {
                    query.add(&format!(r"\Processor Information({inst})\% Processor Time"))
                })
                .or_else(|_| query.add(&format!(r"\Processor({inst})\% Processor Time")))
        };
        let usage_total = usage("_Total")?;
        let usage_all = usage("*").ok();
        let perf_total = query
            .add(r"\Processor Information(_Total)\% Processor Performance")
            .ok();
        let perf_all = query
            .add(r"\Processor Information(*)\% Processor Performance")
            .ok();
        let nominal = query
            .add(r"\Processor Information(_Total)\Processor Frequency")
            .ok();
        query.collect()?; // rate counters need a first sample
        self.counters = Some(Counters {
            usage_total,
            usage_all,
            perf_total,
            perf_all,
            nominal,
            query,
        });

        let (base, boost) = crate::hwid::rated_cpu_mhz();
        let nominal_mhz = power_info().first().map(|p| f64::from(p.MaxMhz));
        self.base_mhz = base.or(nominal_mhz.filter(|&m| m > 0.0));
        self.boost_mhz = boost;

        let r = |reg: &mut Registry, id: &str, name: &str, unit| {
            reg.register(SensorDesc::new(id, Group::Cpu, name, unit, Source::Pdh).with_history())
        };
        self.keys = Some(Keys {
            usage: r(reg, "cpu.usage.total", "CPU Total", Unit::Percent),
            clock: r(reg, "cpu.clock.current", "CPU Clock", Unit::Mhz),
            core_usage: PerCore::new("usage", "Usage", Unit::Percent),
            core_clock: PerCore::new("clock", "Clock", Unit::Mhz),
        });
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let (Some(c), Some(k)) = (&self.counters, &mut self.keys) else {
            return Err("poll before init".into());
        };
        c.query.collect()?;
        let cpu = &mut snap.cpu;
        cpu.mhz_base = self.base_mhz;
        cpu.mhz_boost = self.boost_mhz;

        cpu.usage_total = reg.set(k.usage, c.usage_total.value().map(clamp_pct));
        let per = c.usage_all.as_ref().map_or_else(Vec::new, |all| {
            per_processor(all.array().iter().map(|(n, v)| (n.as_str(), *v)))
        });
        cpu.usage_per_core = k.core_usage.set(reg, per.into_iter().map(clamp_pct));

        let nominal = c
            .nominal
            .as_ref()
            .and_then(Counter::value)
            .filter(|&f| f > 0.0)
            .or(self.base_mhz);
        let clock = match (&c.perf_total, nominal) {
            (Some(p), Some(n)) => p.value().and_then(|pct| effective_mhz(pct, n)),
            _ => None,
        };
        let per_clock: Vec<f64> = match (&c.perf_all, nominal) {
            (Some(p), Some(n)) => {
                per_processor(p.array().iter().map(|(name, v)| (name.as_str(), *v)))
                    .into_iter()
                    .filter_map(|pct| effective_mhz(pct, n))
                    .collect()
            }
            _ => Vec::new(),
        };
        let (clock, per_clock) = if clock.is_some() {
            (clock, per_clock)
        } else {
            // Interim fallback: the power manager's current MHz (often pinned at nominal).
            let info = power_info();
            let per: Vec<f64> = info.iter().map(|p| f64::from(p.CurrentMhz)).collect();
            let max = per
                .iter()
                .copied()
                .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))));
            (max.filter(|&m| m > 0.0), per)
        };
        cpu.mhz_current = reg.set(k.clock, clock);
        cpu.mhz_per_core = k.core_clock.set(reg, per_clock);
        Ok(())
    }
}
