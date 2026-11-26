//! `IntelCpuProvider`: reads Intel Core sensors through PawnIO. Without PawnIO (or on other
//! CPUs) `init` fails and the provider stays off for the session; the CPU fields it owns remain
//! `null`. The reads run on a thread of their own ([`Sampler`]), as the AMD CPU's: they are the
//! same kind of pinned sweep over every logical processor, which on the owner's AMD PC
//! (2026-11-21) took about 340 ms at times, up to 1,217 ms, while every other reading waited.

use std::time::{Duration, Instant};

use super::{
    core_names, dts_temp, package_temp, perf_status_volts, power_limited, prochot, supported,
    tjmax, MSR_IA32_APERF, MSR_IA32_PACKAGE_THERM_STATUS, MSR_IA32_PERF_STATUS,
    MSR_IA32_THERM_STATUS, MSR_PKG_ENERGY_STATUS, MSR_PP0_ENERGY_STATUS, MSR_PP1_ENERGY_STATUS,
    MSR_RAPL_POWER_UNIT, MSR_TEMPERATURE_TARGET,
};
use crate::background::{Sample, Sampler};
use crate::cpu_msr::{energy_unit, watts, Rate};
use crate::hwid::{cpuid, topology};
use crate::pawnio::{on_cpu, urgent, Handle, Module};
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::{CpuSnapshot, SensorSnapshot};

/// Package or cores power above this is a counter glitch, not a reading.
const MAX_PACKAGE_W: f64 = 1000.0;
const MAX_GRAPHICS_W: f64 = 200.0;
/// Longest a tick waits for the CPU's reading; one that takes longer is used at the next tick.
const WAIT: Duration = Duration::from_millis(50);
/// Ticks in a row the last reading stands in while the next is late (then `null`).
const MAX_LATE: u32 = 5;

struct Core {
    /// Each logical processor's APERF.
    aperf: Vec<Rate>,
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

/// One reading of the CPU, raw: the arithmetic happens in the tick.
struct Raw {
    /// The package's thermal status.
    pkg_status: Option<u64>,
    /// The package's, the cores' and the graphics' energy counters (`None` without RAPL, and the
    /// graphics' without a graphics plane in use).
    package: Option<u64>,
    cores: Option<u64>,
    graphics: Option<u64>,
    /// When the energy counters were read, in seconds from the reader's start.
    t: f64,
    /// Per core, per logical processor; `None` where the thread could not be pinned to it.
    threads: Vec<Vec<Option<ThreadRead>>>,
}

/// The reads, done on the reading thread.
struct Reader {
    io: Handle,
    start: Instant,
    /// Whether to read the energy counters, and the graphics' too.
    rapl: bool,
    graphics: bool,
    /// Each core's logical processors.
    cpus: Vec<Vec<u32>>,
}

impl Reader {
    fn read(&self) -> Raw {
        let io = &self.io;
        // Package-scoped registers: any processor will do.
        let pkg_status = msr(io, MSR_IA32_PACKAGE_THERM_STATUS);
        let t = self.start.elapsed().as_secs_f64();
        let energy = |on: bool, index: u32| if on { msr(io, index) } else { None };
        let package = energy(self.rapl, MSR_PKG_ENERGY_STATUS);
        let cores = energy(self.rapl, MSR_PP0_ENERGY_STATUS);
        let graphics = energy(self.rapl && self.graphics, MSR_PP1_ENERGY_STATUS);
        // Per core: thermal status and voltage from the core's first thread; APERF from every
        // thread. The pinned reads run as one time-critical sweep.
        let threads = urgent(|| {
            self.cpus
                .iter()
                .map(|cpus| {
                    cpus.iter()
                        .enumerate()
                        .map(|(i, &cpu)| {
                            on_cpu(cpu, || ThreadRead {
                                aperf: msr(io, MSR_IA32_APERF),
                                t: self.start.elapsed().as_secs_f64(),
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
        Raw {
            pkg_status,
            package,
            cores,
            graphics,
            t,
            threads,
        }
    }
}

/// What the snapshot shows, from the last reading: it stands in while a reading is late.
#[derive(Default)]
struct Shown {
    temp_package: Option<f64>,
    temp_per_core: Vec<f64>,
    power_pkg_w: Option<f64>,
    power_core_w: Option<f64>,
    vid_core: Option<f64>,
    mhz_effective: Option<f64>,
    throttle_thermal: Option<bool>,
    throttle_power_limit: Option<bool>,
}

impl Shown {
    fn fill(&self, cpu: &mut CpuSnapshot) {
        cpu.temp_package = self.temp_package;
        cpu.temp_per_core.clone_from(&self.temp_per_core);
        cpu.power_pkg_w = self.power_pkg_w;
        cpu.power_core_w = self.power_core_w;
        cpu.vid_core = self.vid_core;
        cpu.mhz_effective = self.mhz_effective;
        cpu.throttle_thermal = self.throttle_thermal;
        cpu.throttle_power_limit = self.throttle_power_limit;
    }
}

pub struct IntelCpuProvider {
    /// Takes the readings on a thread of its own.
    reader: Option<Sampler<(), Raw>>,
    /// The package's TjMax; `None` when unreadable (no temperatures then).
    tjmax: Option<f64>,
    /// Joules per RAPL increment; `None` without RAPL (no power then).
    unit: Option<f64>,
    package: Rate,
    cores_energy: Rate,
    graphics: Rate,
    cores: Vec<Core>,
    keys: Option<Keys>,
    shown: Shown,
    /// The last reading's outcome, given again while one is late.
    outcome: Result<(), String>,
}

impl IntelCpuProvider {
    pub fn new() -> Self {
        Self {
            reader: None,
            tjmax: None,
            unit: None,
            package: Rate::default(),
            cores_energy: Rate::default(),
            graphics: Rate::default(),
            cores: Vec::new(),
            keys: None,
            shown: Shown::default(),
            outcome: Ok(()),
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

impl IntelCpuProvider {
    /// Works out the readings from one raw reading, into the registry and [`Shown`].
    fn apply(&mut self, reg: &mut Registry, raw: Raw) -> Result<(), String> {
        let Some(k) = &self.keys else {
            return Err("poll before init".into());
        };
        let Raw {
            pkg_status,
            package: package_energy,
            cores: cores_energy,
            graphics: graphics_energy,
            t,
            threads,
        } = raw;
        let unit = self.unit;
        let energy = |rate: &mut Rate, raw: Option<u64>, max_w: f64| {
            let unit = unit?;
            watts(rate.update(raw?, t, 32), unit, max_w)
        };
        let package_w = energy(&mut self.package, package_energy, MAX_PACKAGE_W);
        let cores_w = energy(&mut self.cores_energy, cores_energy, MAX_PACKAGE_W);
        let graphics_w = energy(&mut self.graphics, graphics_energy, MAX_GRAPHICS_W);

        let mut temps = Vec::with_capacity(self.cores.len());
        let mut vids = Vec::new();
        let mut effective = Vec::new();
        let mut hot = pkg_status.map(prochot);
        for (n, (core, reads)) in self.cores.iter_mut().zip(threads).enumerate() {
            for (aperf_rate, read) in core.aperf.iter_mut().zip(reads) {
                let Some(ThreadRead {
                    aperf,
                    t,
                    core: extra,
                }) = read
                else {
                    continue;
                };
                if let Some(hz) = aperf.and_then(|a| aperf_rate.update(a, t, 64)) {
                    effective.push(hz / 1e6);
                }
                let Some((status, perf)) = extra else {
                    continue;
                };
                if let Some(s) = status {
                    hot = Some(hot.unwrap_or(false) || prochot(s));
                }
                match status.zip(core.tjmax).and_then(|(s, tj)| dts_temp(s, tj)) {
                    Some(t) => {
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
                    // A core that read before and does not now shows nothing, not its last value.
                    None => {
                        if let Some(key) = core.key {
                            reg.set(key, None);
                        }
                    }
                }
                if let Some(v) = perf.and_then(perf_status_volts) {
                    vids.push(v);
                }
            }
        }
        let shown = &mut self.shown;
        let package = pkg_status
            .zip(self.tjmax)
            .and_then(|(s, tj)| dts_temp(s, tj));
        shown.temp_package = reg.set(k.package, package_temp(package, &temps));
        // Per-core temperatures only when every core read: the list is in core order.
        shown.temp_per_core = if temps.len() == self.cores.len() {
            temps
        } else {
            Vec::new()
        };
        shown.power_pkg_w = reg.set(k.power_package, package_w);
        shown.power_core_w = reg.set(k.power_cores, cores_w);
        if let Some(key) = k.power_graphics {
            reg.set(key, graphics_w);
        }
        shown.vid_core = reg.set(k.vid, vids.iter().copied().reduce(f64::max));
        #[allow(clippy::cast_precision_loss)]
        let avg =
            (!effective.is_empty()).then(|| effective.iter().sum::<f64>() / effective.len() as f64);
        shown.mhz_effective = reg.set(k.effective, avg);
        shown.throttle_thermal = hot;
        shown.throttle_power_limit = pkg_status.map(power_limited);

        if shown.temp_package.is_none() && shown.power_pkg_w.is_none() && pkg_status.is_none() {
            return Err("no Intel sensor readable".into());
        }
        Ok(())
    }

    /// No reading to show: every sensor unavailable.
    fn clear(&mut self, reg: &mut Registry) {
        if let Some(k) = &self.keys {
            let keys = [
                k.package,
                k.power_package,
                k.power_cores,
                k.vid,
                k.effective,
            ]
            .into_iter()
            .chain(k.power_graphics)
            .chain(self.cores.iter().filter_map(|c| c.key));
            for key in keys {
                reg.set(key, None);
            }
        }
        self.shown = Shown::default();
    }
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
                aperf: vec![Rate::default(); c.logical.len()],
                // Each core's own TjMax where it reads, as Linux `coretemp` takes it.
                tjmax: on_cpu(c.logical[0], || msr(&io, MSR_TEMPERATURE_TARGET))
                    .flatten()
                    .and_then(tjmax)
                    .or(package_tjmax),
                name,
                key: None,
            })
            .collect();
        let cpus: Vec<Vec<u32>> = cores.iter().map(|c| c.logical.clone()).collect();

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
        let reader = Reader {
            io,
            start: Instant::now(),
            rapl: self.unit.is_some(),
            graphics,
            cpus,
        };
        self.reader = Some(Sampler::start(
            "clokrix-intel-cpu",
            "intel-cpu: reading",
            move |()| reader.read(),
        )?);
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let sample = match self.reader.as_mut() {
            Some(r) => r.tick((), WAIT),
            None => return Err("poll before init".into()),
        };
        match sample {
            Sample::Came(raw) => {
                self.outcome = raw
                    .map_err(|e| format!("a reading failed: {e}"))
                    .and_then(|raw| self.apply(reg, raw));
            }
            Sample::Late(n) if n > MAX_LATE => {
                if n == MAX_LATE + 1 {
                    self.clear(reg);
                }
                self.outcome = Err(format!(
                    "no reading from the CPU in more than {MAX_LATE} updates"
                ));
            }
            Sample::Late(_) => {}
            Sample::Gone => {
                self.clear(reg);
                self.outcome = Err("the reading thread stopped".into());
            }
        }
        self.shown.fill(&mut snap.cpu);
        self.outcome.clone().map_err(Into::into)
    }
}
