//! `AmdCpuProvider`: reads Zen sensors through PawnIO. Without PawnIO (or on other CPUs) `init`
//! fails and the provider stays off for the session; the CPU fields it owns remain `null`. The
//! reads run on a thread of their own ([`Sampler`]): the owner's log (2026-11-21) had them at
//! about 340 ms at times, up to 1,217 ms, and every other reading waited for them.

use std::time::{Duration, Instant};

use super::{
    ccd_base, ccd_temp, energy_unit, supported, svi3, tctl, tctl_offset, vid, vid_volts, watts,
    Rate, MAX_CCDS, MSR_APERF_RO, MSR_CORE_ENERGY, MSR_HW_PSTATE_STATUS, MSR_PKG_ENERGY,
    MSR_PWR_UNIT, SMN_THM_TCON_CUR_TMP,
};
use crate::background::{Sample, Sampler};
use crate::hwid::{cpuid, topology};
use crate::pawnio::{on_cpu, urgent, BusLock, Handle, Module, MUTEX_PCI};
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::{CpuSnapshot, SensorSnapshot};

/// Package power above this is a counter glitch, not a reading.
const MAX_PACKAGE_W: f64 = 1000.0;
const MAX_CORE_W: f64 = 200.0;
/// Longest wait for another tool's PCI/SMN access before skipping temperatures this tick. Kept
/// short: a background service in the field held the mutex for ~150 ms at a time, and waiting
/// that out stalled every poll it hit.
const LOCK_MS: u32 = 5;
/// Readings the last temperatures stand in while another tool keeps the bus (then `null`).
const MAX_STALE: u32 = 5;
/// Longest a tick waits for the CPU's reading; one that takes longer is used at the next tick.
const WAIT: Duration = Duration::from_millis(50);
/// Ticks in a row the last reading stands in while the next is late (then `null`).
const MAX_LATE: u32 = 5;

/// One logical processor's pinned MSR reads: APERF, when they were taken, and (first thread of
/// a core only) core energy and the P-state status holding the VID.
struct ThreadRead {
    aperf: Option<u64>,
    t: f64,
    core: Option<(Option<u64>, Option<u64>)>,
}

struct Core {
    /// Each logical processor's APERF.
    aperf: Vec<Rate>,
    energy: Rate,
    key: Option<SensorKey>,
}

struct Keys {
    tdie: SensorKey,
    tctl: Option<SensorKey>,
    ccd: Vec<Option<SensorKey>>,
    package: SensorKey,
    cores: SensorKey,
    vid: SensorKey,
    effective: SensorKey,
}

/// One reading of the CPU, raw: the arithmetic happens in the tick.
struct Raw {
    /// Tctl's and the CCDs' registers; `None` when another tool kept the bus.
    temps: Option<(Option<u32>, Vec<Option<u32>>)>,
    package: Option<u64>,
    /// When the package energy was read, in seconds from the reader's start.
    t: f64,
    /// Per core, per logical processor; `None` where the thread could not be pinned to it.
    threads: Vec<Vec<Option<ThreadRead>>>,
}

/// The reads, done on the reading thread.
struct Reader {
    io: Handle,
    pci: BusLock,
    start: Instant,
    ccd_base: Option<u32>,
    /// Each core's logical processors.
    cpus: Vec<Vec<u32>>,
}

impl Reader {
    fn read(&self) -> Raw {
        let io = &self.io;
        // Temperatures (SMN, shared index/data pair → PCI mutex). Held time-critical: a holder
        // preempted by a full-load benchmark would stall every other monitor waiting on the
        // mutex for whole scheduler quanta.
        let temps = urgent(|| {
            self.pci.hold(LOCK_MS, || {
                let tctl = smn(io, SMN_THM_TCON_CUR_TMP);
                let ccds: Vec<Option<u32>> = self
                    .ccd_base
                    .map(|base| (0..MAX_CCDS).map(|i| smn(io, base + i * 4)).collect())
                    .unwrap_or_default();
                (tctl, ccds)
            })
        });
        // Package energy is package-scoped: any processor will do.
        let package = msr(io, MSR_PKG_ENERGY);
        let t = self.start.elapsed().as_secs_f64();
        // Per core: energy and VID from the core's first thread; APERF from every thread. The
        // pinned reads run as one time-critical sweep.
        let threads = urgent(|| {
            self.cpus
                .iter()
                .map(|cpus| {
                    cpus.iter()
                        .enumerate()
                        .map(|(i, &cpu)| {
                            on_cpu(cpu, || ThreadRead {
                                aperf: msr(io, MSR_APERF_RO),
                                t: self.start.elapsed().as_secs_f64(),
                                core: (i == 0).then(|| {
                                    (msr(io, MSR_CORE_ENERGY), msr(io, MSR_HW_PSTATE_STATUS))
                                }),
                            })
                        })
                        .collect()
                })
                .collect()
        });
        Raw {
            temps,
            package,
            t,
            threads,
        }
    }
}

/// What the snapshot shows, from the last reading: it stands in while a reading is late.
#[derive(Default)]
struct Shown {
    temp_package: Option<f64>,
    temp_ccd: Vec<f64>,
    power_pkg_w: Option<f64>,
    power_core_w: Option<f64>,
    power_per_core_w: Vec<f64>,
    vid_core: Option<f64>,
    mhz_effective: Option<f64>,
}

impl Shown {
    fn fill(&self, cpu: &mut CpuSnapshot) {
        cpu.temp_package = self.temp_package;
        cpu.temp_ccd.clone_from(&self.temp_ccd);
        cpu.power_pkg_w = self.power_pkg_w;
        cpu.power_core_w = self.power_core_w;
        cpu.power_per_core_w.clone_from(&self.power_per_core_w);
        cpu.vid_core = self.vid_core;
        cpu.mhz_effective = self.mhz_effective;
    }
}

pub struct AmdCpuProvider {
    /// Takes the readings on a thread of its own.
    reader: Option<Sampler<(), Raw>>,
    family: u32,
    model: u32,
    offset: f64,
    unit: f64,
    package: Rate,
    cores: Vec<Core>,
    keys: Option<Keys>,
    /// Readings in a row whose temperatures another tool's hold on the bus kept out.
    stale: u32,
    shown: Shown,
    /// The last reading's outcome, given again while one is late.
    outcome: Result<(), String>,
}

impl AmdCpuProvider {
    pub fn new() -> Self {
        Self {
            reader: None,
            family: 0,
            model: 0,
            offset: 0.0,
            unit: 0.0,
            package: Rate::default(),
            cores: Vec::new(),
            keys: None,
            stale: 0,
            shown: Shown::default(),
            outcome: Ok(()),
        }
    }
}

impl Default for AmdCpuProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn msr(io: &Handle, index: u32) -> Option<u64> {
    io.exec1("ioctl_read_msr", &[u64::from(index)]).ok()
}

fn smn(io: &Handle, addr: u32) -> Option<u32> {
    io.exec1("ioctl_read_smn", &[u64::from(addr)])
        .ok()
        .and_then(|v| u32::try_from(v & 0xFFFF_FFFF).ok())
}

fn sensor(reg: &mut Registry, id: String, name: String, unit: Unit, history: bool) -> SensorKey {
    let d = SensorDesc::new(id, Group::Cpu, name, unit, Source::Driver);
    reg.register(if history { d.with_history() } else { d })
}

impl AmdCpuProvider {
    /// Works out the readings from one raw reading, into the registry and [`Shown`].
    fn apply(&mut self, reg: &mut Registry, raw: Raw) -> Result<(), String> {
        let Some(k) = &mut self.keys else {
            return Err("poll before init".into());
        };
        let Raw {
            temps,
            package,
            t,
            threads,
        } = raw;
        let shown = &mut self.shown;

        if let Some((tctl_raw, ccds)) = temps {
            let control = tctl_raw.map(tctl);
            if let Some(key) = k.tctl {
                reg.set(key, control);
            }
            shown.temp_ccd = ccds
                .iter()
                .enumerate()
                .filter_map(|(i, raw)| Some((i, ccd_temp((*raw)?)?)))
                .map(|(i, v)| {
                    if k.ccd.len() <= i {
                        k.ccd.resize(i + 1, None);
                    }
                    let key = *k.ccd[i].get_or_insert_with(|| {
                        sensor(
                            reg,
                            format!("cpu.temp.ccd{i}"),
                            format!("CPU CCD{}", i + 1),
                            Unit::Celsius,
                            true,
                        )
                    });
                    reg.set(key, Some(v));
                    v
                })
                .collect();
            shown.temp_package = reg.set(k.tdie, control.map(|c| c - self.offset));
            self.stale = 0;
        } else if self.stale < MAX_STALE {
            // Another monitor held the bus for this reading: keep showing the last one.
            self.stale += 1;
        } else {
            shown.temp_package = reg.set(k.tdie, None);
            shown.temp_ccd = Vec::new();
            if let Some(key) = k.tctl {
                reg.set(key, None);
            }
            for key in k.ccd.iter().flatten() {
                reg.set(*key, None);
            }
        }

        let watts_pkg = watts(
            package.and_then(|raw| self.package.update(raw, t, 32)),
            self.unit,
            MAX_PACKAGE_W,
        );
        shown.power_pkg_w = reg.set(k.package, watts_pkg);

        let unit = self.unit;
        let mut core_w = Vec::with_capacity(self.cores.len());
        let mut vids = Vec::new();
        let mut effective = Vec::new();
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
                if let Some((energy, pstate)) = extra {
                    let w = watts(
                        energy.and_then(|e| core.energy.update(e, t, 32)),
                        unit,
                        MAX_CORE_W,
                    );
                    if let Some(w) = w {
                        let key = *core.key.get_or_insert_with(|| {
                            sensor(
                                reg,
                                format!("cpu.core{n}.power"),
                                format!("Core #{n} Power"),
                                Unit::Watt,
                                false,
                            )
                        });
                        core_w.push(reg.set(key, Some(w)).unwrap_or(w));
                    }
                    if let Some(p) = pstate {
                        vids.push(vid(p));
                    }
                }
            }
        }
        let complete = !core_w.is_empty() && core_w.len() == self.cores.len();
        shown.power_core_w = reg.set(k.cores, complete.then(|| core_w.iter().sum()));
        shown.power_per_core_w = if complete { core_w } else { Vec::new() };
        let svi3 = svi3(self.family, self.model);
        shown.vid_core = reg.set(
            k.vid,
            vids.iter()
                .max()
                .map(|&v| vid_volts(v, svi3))
                .filter(|v| (0.2..=2.0).contains(v)),
        );
        #[allow(clippy::cast_precision_loss)]
        let avg =
            (!effective.is_empty()).then(|| effective.iter().sum::<f64>() / effective.len() as f64);
        shown.mhz_effective = reg.set(k.effective, avg);

        if shown.temp_package.is_none() && shown.power_pkg_w.is_none() && package.is_none() {
            return Err("no AMD sensor readable".into());
        }
        Ok(())
    }

    /// No reading to show: every sensor unavailable.
    fn clear(&mut self, reg: &mut Registry) {
        if let Some(k) = &self.keys {
            let keys = [k.tdie, k.package, k.cores, k.vid, k.effective]
                .into_iter()
                .chain(k.tctl)
                .chain(k.ccd.iter().flatten().copied())
                .chain(self.cores.iter().filter_map(|c| c.key));
            for key in keys {
                reg.set(key, None);
            }
        }
        self.shown = Shown::default();
    }
}

impl Provider for AmdCpuProvider {
    fn name(&self) -> &'static str {
        "amd-cpu"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        let id = cpuid::read_live();
        if !supported(&id.vendor, id.family) {
            return Err(
                format!("not an AMD Zen CPU ({} family {:#x})", id.vendor, id.family).into(),
            );
        }
        let io = Handle::open(Module::AmdFamily17)?;
        self.family = id.family;
        self.model = id.model;
        self.offset = tctl_offset(id.brand.as_deref().unwrap_or(""));
        self.unit = energy_unit(msr(&io, MSR_PWR_UNIT).ok_or("PWR_UNIT unreadable")?);

        let topo = crate::hwid::win::topology_raw()
            .map(|b| topology::parse(&b))
            .unwrap_or_default();
        let cpus: Vec<Vec<u32>> = topo
            .cores
            .iter()
            .filter(|c| !c.logical.is_empty())
            .map(|c| c.logical.clone())
            .collect();
        self.cores = cpus
            .iter()
            .map(|c| Core {
                aperf: vec![Rate::default(); c.len()],
                energy: Rate::default(),
                key: None,
            })
            .collect();

        let r = |reg: &mut Registry, id: &str, name: &str, unit| {
            sensor(reg, id.into(), name.into(), unit, true)
        };
        self.keys = Some(Keys {
            tdie: r(reg, "cpu.temp.package", "CPU (Tdie)", Unit::Celsius),
            tctl: (self.offset > 0.0).then(|| r(reg, "cpu.temp.tctl", "CPU (Tctl)", Unit::Celsius)),
            ccd: Vec::new(),
            package: r(reg, "cpu.power.package", "CPU Package Power", Unit::Watt),
            cores: r(reg, "cpu.power.cores", "CPU Cores Power", Unit::Watt),
            vid: r(reg, "cpu.vid", "CPU Core VID", Unit::Volt),
            effective: r(reg, "cpu.clock.effective", "CPU Effective Clock", Unit::Mhz),
        });
        let reader = Reader {
            io,
            pci: BusLock::new(MUTEX_PCI),
            start: Instant::now(),
            ccd_base: ccd_base(id.family, id.model),
            cpus,
        };
        self.reader = Some(Sampler::start(
            "clokrix-amd-cpu",
            "amd-cpu: reading",
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
