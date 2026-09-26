//! `AmdCpuProvider`: reads Zen sensors through PawnIO. Without PawnIO (or on other CPUs) `init`
//! fails and the provider stays off for the session; the CPU fields it owns remain `null`.

use std::time::Instant;

use super::{
    ccd_base, ccd_temp, energy_unit, supported, svi3, tctl, tctl_offset, vid, vid_volts, watts,
    Rate, MAX_CCDS, MSR_APERF_RO, MSR_CORE_ENERGY, MSR_HW_PSTATE_STATUS, MSR_PKG_ENERGY,
    MSR_PWR_UNIT, SMN_THM_TCON_CUR_TMP,
};
use crate::hwid::{cpuid, topology};
use crate::pawnio::{on_cpu, BusLock, Handle, Module, MUTEX_PCI};
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::SensorSnapshot;

/// Package power above this is a counter glitch, not a reading.
const MAX_PACKAGE_W: f64 = 1000.0;
const MAX_CORE_W: f64 = 200.0;
/// Longest wait for another tool's PCI/SMN access before skipping temperatures this tick.
const LOCK_MS: u32 = 50;

struct Thread {
    cpu: u32,
    aperf: Rate,
}

struct Core {
    threads: Vec<Thread>,
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

pub struct AmdCpuProvider {
    io: Option<Handle>,
    pci: BusLock,
    start: Instant,
    family: u32,
    model: u32,
    offset: f64,
    ccd_base: Option<u32>,
    unit: f64,
    package: Rate,
    cores: Vec<Core>,
    keys: Option<Keys>,
}

impl AmdCpuProvider {
    pub fn new() -> Self {
        Self {
            io: None,
            pci: BusLock::new(MUTEX_PCI),
            start: Instant::now(),
            family: 0,
            model: 0,
            offset: 0.0,
            ccd_base: None,
            unit: 0.0,
            package: Rate::default(),
            cores: Vec::new(),
            keys: None,
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
        self.ccd_base = ccd_base(id.family, id.model);
        self.unit = energy_unit(msr(&io, MSR_PWR_UNIT).ok_or("PWR_UNIT unreadable")?);

        let topo = crate::hwid::win::topology_raw()
            .map(|b| topology::parse(&b))
            .unwrap_or_default();
        self.cores = topo
            .cores
            .iter()
            .filter(|c| !c.logical.is_empty())
            .map(|c| Core {
                threads: c
                    .logical
                    .iter()
                    .map(|&cpu| Thread {
                        cpu,
                        aperf: Rate::default(),
                    })
                    .collect(),
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
        self.io = Some(io);
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let (Some(io), Some(k)) = (&self.io, &mut self.keys) else {
            return Err("poll before init".into());
        };
        let cpu = &mut snap.cpu;

        // Temperatures (SMN, shared index/data pair → PCI mutex).
        let ccd_base = self.ccd_base;
        let temps = self.pci.hold(LOCK_MS, || {
            let t = smn(io, SMN_THM_TCON_CUR_TMP);
            let ccds: Vec<Option<u32>> = ccd_base
                .map(|base| (0..MAX_CCDS).map(|i| smn(io, base + i * 4)).collect())
                .unwrap_or_default();
            (t, ccds)
        });
        if let Some((t, ccds)) = temps {
            let t = t.map(tctl);
            if let Some(key) = k.tctl {
                reg.set(key, t);
            }
            cpu.temp_package = reg.set(k.tdie, t.map(|t| t - self.offset));
            cpu.temp_ccd = ccds
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
        }

        // Package energy is package-scoped: any processor will do.
        let pkg_raw = msr(io, MSR_PKG_ENERGY);
        let now = self.start.elapsed().as_secs_f64();
        let package = watts(
            pkg_raw.and_then(|raw| self.package.update(raw, now, 32)),
            self.unit,
            MAX_PACKAGE_W,
        );
        cpu.power_pkg_w = reg.set(k.package, package);

        // Per core: energy and VID from the core's first thread; APERF from every thread.
        let start = self.start;
        let unit = self.unit;
        let mut core_w = Vec::with_capacity(self.cores.len());
        let mut vids = Vec::new();
        let mut effective = Vec::new();
        for (n, core) in self.cores.iter_mut().enumerate() {
            for (i, th) in core.threads.iter_mut().enumerate() {
                let read = on_cpu(th.cpu, || {
                    let aperf = msr(io, MSR_APERF_RO);
                    let t = start.elapsed().as_secs_f64();
                    let extra =
                        (i == 0).then(|| (msr(io, MSR_CORE_ENERGY), msr(io, MSR_HW_PSTATE_STATUS)));
                    (aperf, t, extra)
                });
                let Some((aperf, t, extra)) = read else {
                    continue;
                };
                if let Some(hz) = aperf.and_then(|a| th.aperf.update(a, t, 64)) {
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
        cpu.power_core_w = reg.set(k.cores, complete.then(|| core_w.iter().sum()));
        cpu.power_per_core_w = if complete { core_w } else { Vec::new() };
        let svi3 = svi3(self.family, self.model);
        cpu.vid_core = reg.set(
            k.vid,
            vids.iter()
                .max()
                .map(|&v| vid_volts(v, svi3))
                .filter(|v| (0.2..=2.0).contains(v)),
        );
        #[allow(clippy::cast_precision_loss)]
        let avg =
            (!effective.is_empty()).then(|| effective.iter().sum::<f64>() / effective.len() as f64);
        cpu.mhz_effective = reg.set(k.effective, avg);

        if cpu.temp_package.is_none() && cpu.power_pkg_w.is_none() && pkg_raw.is_none() {
            return Err("no AMD sensor readable".into());
        }
        Ok(())
    }
}
