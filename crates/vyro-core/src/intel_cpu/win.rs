//! `IntelCpuProvider`: reads Intel Core sensors through PawnIO. Without PawnIO (or on other
//! CPUs) `init` fails and the provider stays off for the session; the CPU fields it owns remain
//! `null`.

use std::time::Instant;

use super::{
    at_tjmax, core_names, dts_temp, package_temp, perf_status_volts, power_limited, supported,
    tjmax, MSR_IA32_APERF, MSR_IA32_PACKAGE_THERM_STATUS, MSR_IA32_PERF_STATUS,
    MSR_IA32_THERM_STATUS, MSR_PKG_ENERGY_STATUS, MSR_PP0_ENERGY_STATUS, MSR_PP1_ENERGY_STATUS,
    MSR_RAPL_POWER_UNIT, MSR_TEMPERATURE_TARGET,
};
use crate::cpu_msr::{energy_unit, watts, Rate};
use crate::hwid::{cpuid, topology};
use crate::pawnio::{on_cpu, urgent, Handle, Module};
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::SensorSnapshot;

/// Package or cores power above this is a counter glitch, not a reading.
const MAX_PACKAGE_W: f64 = 1000.0;
const MAX_GRAPHICS_W: f64 = 200.0;

struct Thread {
    cpu: u32,
    aperf: Rate,
}

struct Core {
    threads: Vec<Thread>,
    tjmax: Option<f64>,
    name: String,
    key: Option<SensorKey>,
}

/// One logical processor's pinned reads: APERF, when it was taken, and (first thread of a core
/// only) the core's thermal status and `PERF_STATUS`.
struct ThreadRead {
    aperf: Option<u64>,
    t: f64,
    core: Option<(Option<u64>, Option<u64>)>,
}

struct Keys {
    package: SensorKey,
    power_package: SensorKey,
    power_cores: SensorKey,
    power_graphics: Option<SensorKey>,
    vid: SensorKey,
    effective: SensorKey,
}

pub struct IntelCpuProvider {
    io: Option<Handle>,
    start: Instant,
    /// The package's TjMax; `None` when unreadable (no temperatures then).
    tjmax: Option<f64>,
    /// Joules per RAPL increment; `None` without RAPL (no power then).
    unit: Option<f64>,
    package: Rate,
    cores_energy: Rate,
    graphics: Rate,
    cores: Vec<Core>,
    keys: Option<Keys>,
}

impl IntelCpuProvider {
    pub fn new() -> Self {
        Self {
            io: None,
            start: Instant::now(),
            tjmax: None,
            unit: None,
            package: Rate::default(),
            cores_energy: Rate::default(),
            graphics: Rate::default(),
            cores: Vec::new(),
            keys: None,
        }
    }
}

impl Default for IntelCpuProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn msr(io: &Handle, index: u32) -> Option<u64> {
    io.exec1("ioctl_read_msr", &[u64::from(index)]).ok()
}

fn sensor(reg: &mut Registry, id: String, name: String, unit: Unit, history: bool) -> SensorKey {
    let d = SensorDesc::new(id, Group::Cpu, name, unit, Source::Driver);
    reg.register(if history { d.with_history() } else { d })
}

impl Provider for IntelCpuProvider {
    fn name(&self) -> &'static str {
        "intel-cpu"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        let id = cpuid::read_live();
        if !supported(&id.vendor, id.family) {
            return Err(format!(
                "not an Intel Core CPU ({} family {:#x})",
                id.vendor, id.family
            )
            .into());
        }
        let io = Handle::open(Module::IntelMsr)?;
        self.tjmax = msr(&io, MSR_TEMPERATURE_TARGET).and_then(tjmax);
        self.unit = msr(&io, MSR_RAPL_POWER_UNIT)
            .map(energy_unit)
            .filter(|&u| u > 0.0);
        if self.tjmax.is_none() && self.unit.is_none() {
            return Err("neither TjMax nor the RAPL unit is readable".into());
        }

        let topo = crate::hwid::win::topology_raw()
            .map(|b| topology::parse(&b))
            .unwrap_or_default();
        let cores: Vec<_> = topo
            .cores
            .iter()
            .filter(|c| !c.logical.is_empty())
            .collect();
        let names = core_names(&cores.iter().map(|c| c.kind).collect::<Vec<_>>());
        let package_tjmax = self.tjmax;
        self.cores = cores
            .iter()
            .zip(names)
            .map(|(c, name)| Core {
                threads: c
                    .logical
                    .iter()
                    .map(|&cpu| Thread {
                        cpu,
                        aperf: Rate::default(),
                    })
                    .collect(),
                // Each core's own TjMax where it reads, as Linux `coretemp` takes it.
                tjmax: on_cpu(c.logical[0], || msr(&io, MSR_TEMPERATURE_TARGET))
                    .flatten()
                    .and_then(tjmax)
                    .or(package_tjmax),
                name,
                key: None,
            })
            .collect();

        let r = |reg: &mut Registry, id: &str, name: &str, unit| {
            sensor(reg, id.into(), name.into(), unit, true)
        };
        // The graphics plane counts only on a CPU with its graphics in use; an F model's stays 0.
        let graphics = msr(&io, MSR_PP1_ENERGY_STATUS).is_some_and(|e| e & 0xFFFF_FFFF != 0);
        self.keys = Some(Keys {
            package: r(reg, "cpu.temp.package", "CPU Package", Unit::Celsius),
            power_package: r(reg, "cpu.power.package", "CPU Package Power", Unit::Watt),
            power_cores: r(reg, "cpu.power.cores", "CPU Cores Power", Unit::Watt),
            power_graphics: graphics
                .then(|| r(reg, "cpu.power.graphics", "CPU Graphics Power", Unit::Watt)),
            vid: r(reg, "cpu.vid", "CPU Core VID", Unit::Volt),
            effective: r(reg, "cpu.clock.effective", "CPU Effective Clock", Unit::Mhz),
        });
        log::info!(
            "intel-cpu: family {:#x} model {:#x}, TjMax {:?} °C, RAPL unit {:?} J, {} cores",
            id.family,
            id.model,
            self.tjmax,
            self.unit,
            self.cores.len(),
        );
        self.io = Some(io);
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let (Some(io), Some(k)) = (&self.io, &self.keys) else {
            return Err("poll before init".into());
        };
        let cpu = &mut snap.cpu;

        // Package-scoped registers: any processor will do.
        let pkg_status = msr(io, MSR_IA32_PACKAGE_THERM_STATUS);
        let now = self.start.elapsed().as_secs_f64();
        let energy = |rate: &mut Rate, index: u32, max_w: f64| {
            let unit = self.unit?;
            let raw = msr(io, index)?;
            watts(rate.update(raw, now, 32), unit, max_w)
        };
        let package_w = energy(&mut self.package, MSR_PKG_ENERGY_STATUS, MAX_PACKAGE_W);
        let cores_w = energy(&mut self.cores_energy, MSR_PP0_ENERGY_STATUS, MAX_PACKAGE_W);
        let graphics_w = k
            .power_graphics
            .and_then(|_| energy(&mut self.graphics, MSR_PP1_ENERGY_STATUS, MAX_GRAPHICS_W));

        // Per core: thermal status and voltage from the core's first thread; APERF from every
        // thread. The pinned reads run as one time-critical sweep; the arithmetic happens after.
        let start = self.start;
        let reads: Vec<Vec<Option<ThreadRead>>> = urgent(|| {
            self.cores
                .iter()
                .map(|core| {
                    core.threads
                        .iter()
                        .enumerate()
                        .map(|(i, th)| {
                            on_cpu(th.cpu, || ThreadRead {
                                aperf: msr(io, MSR_IA32_APERF),
                                t: start.elapsed().as_secs_f64(),
                                core: (i == 0).then(|| {
                                    (
                                        msr(io, MSR_IA32_THERM_STATUS),
                                        msr(io, MSR_IA32_PERF_STATUS),
                                    )
                                }),
                            })
                        })
                        .collect()
                })
                .collect()
        });
        let mut temps = Vec::with_capacity(self.cores.len());
        let mut vids = Vec::new();
        let mut effective = Vec::new();
        let mut hot = pkg_status.map(at_tjmax);
        for (n, (core, reads)) in self.cores.iter_mut().zip(reads).enumerate() {
            for (th, read) in core.threads.iter_mut().zip(reads) {
                let Some(ThreadRead {
                    aperf,
                    t,
                    core: extra,
                }) = read
                else {
                    continue;
                };
                if let Some(hz) = aperf.and_then(|a| th.aperf.update(a, t, 64)) {
                    effective.push(hz / 1e6);
                }
                let Some((status, perf)) = extra else {
                    continue;
                };
                if let Some(s) = status {
                    hot = Some(hot.unwrap_or(false) || at_tjmax(s));
                }
                if let Some(t) = status.zip(core.tjmax).and_then(|(s, tj)| dts_temp(s, tj)) {
                    let key = *core.key.get_or_insert_with(|| {
                        sensor(
                            reg,
                            format!("cpu.core{n}.temp"),
                            core.name.clone(),
                            Unit::Celsius,
                            false,
                        )
                    });
                    temps.push(reg.set(key, Some(t)).unwrap_or(t));
                }
                if let Some(v) = perf.and_then(perf_status_volts) {
                    vids.push(v);
                }
            }
        }
        let package = pkg_status
            .zip(self.tjmax)
            .and_then(|(s, tj)| dts_temp(s, tj));
        cpu.temp_package = reg.set(k.package, package_temp(package, &temps));
        // Per-core temperatures only when every core read: the list is in core order.
        cpu.temp_per_core = if temps.len() == self.cores.len() {
            temps
        } else {
            Vec::new()
        };
        cpu.power_pkg_w = reg.set(k.power_package, package_w);
        cpu.power_core_w = reg.set(k.power_cores, cores_w);
        if let Some(key) = k.power_graphics {
            reg.set(key, graphics_w);
        }
        cpu.vid_core = reg.set(k.vid, vids.iter().copied().reduce(f64::max));
        #[allow(clippy::cast_precision_loss)]
        let avg =
            (!effective.is_empty()).then(|| effective.iter().sum::<f64>() / effective.len() as f64);
        cpu.mhz_effective = reg.set(k.effective, avg);
        cpu.throttle_thermal = hot;
        cpu.throttle_power_limit = pkg_status.map(power_limited);

        if cpu.temp_package.is_none() && cpu.power_pkg_w.is_none() && pkg_status.is_none() {
            return Err("no Intel sensor readable".into());
        }
        Ok(())
    }
}
