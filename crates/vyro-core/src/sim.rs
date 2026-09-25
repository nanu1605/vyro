//! Simulated hardware, used on non-Windows hosts and with `VYRO_SIM=1`.
//!
//! Loads drift toward randomly chosen targets (mostly idle with occasional bursts); clocks,
//! temperatures, power and fans follow load with lag, so the UI sees plausible, correlated
//! motion through the real registry → snapshot → event pipeline.

use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::{
    DimmSnapshot, FanKind, FanSnapshot, GpuEngines, GpuSnapshot, NetKind, NetworkSnapshot,
    SensorSnapshot, Status, StorageKind, StorageSnapshot, VoltageReading,
};

const MHZ_BASE: f64 = 3600.0;
const MHZ_BOOST: f64 = 5400.0;
const MEM_TOTAL_MB: f64 = 32768.0;
const VRAM_TOTAL_MB: f64 = 12288.0;

/// A value that eases toward a target that is re-picked at random.
#[derive(Debug, Clone, Copy)]
struct Drift {
    value: f64,
    target: f64,
}

impl Drift {
    fn new(value: f64) -> Self {
        Self {
            value,
            target: value,
        }
    }

    fn step(&mut self, rng: &mut fastrand::Rng, rate: f64, noise: f64, lo: f64, hi: f64) -> f64 {
        self.value += (self.target - self.value) * rate + (rng.f64() - 0.5) * 2.0 * noise;
        self.value = self.value.clamp(lo, hi);
        self.value
    }
}

/// Mostly idle, sometimes busy.
fn pick_load(rng: &mut fastrand::Rng) -> f64 {
    if rng.f64() < 0.25 {
        55.0 + rng.f64() * 45.0
    } else {
        3.0 + rng.f64() * 22.0
    }
}

/// First-order lag toward `target`.
fn follow(cur: f64, target: f64, rate: f64) -> f64 {
    cur + (target - cur) * rate
}

struct Keys {
    cpu_usage: SensorKey,
    cpu_clock: SensorKey,
    cpu_temp: SensorKey,
    cpu_power: SensorKey,
    cores: Vec<(SensorKey, SensorKey, SensorKey)>,
    gpu_usage: SensorKey,
    gpu_clock: SensorKey,
    gpu_mem_clock: SensorKey,
    gpu_temp: SensorKey,
    gpu_hotspot: SensorKey,
    gpu_vram: SensorKey,
    gpu_power: SensorKey,
    gpu_fan: SensorKey,
    mem_used: SensorKey,
    mem_available: SensorKey,
    disk_temp: SensorKey,
    disk_read: SensorKey,
    disk_write: SensorKey,
    fan_cpu: SensorKey,
    fan_pump: SensorKey,
    volt_12v: SensorKey,
    volt_core: SensorKey,
    net_down: SensorKey,
    net_up: SensorKey,
    net_ping: SensorKey,
    power_total: SensorKey,
}

pub struct SimProvider {
    rng: fastrand::Rng,
    cores: usize,
    keys: Option<Keys>,
    cpu: Drift,
    gpu: Drift,
    mem: Drift,
    disk: Drift,
    net: Drift,
    core_bias: Vec<f64>,
    cpu_temp: f64,
    gpu_temp: f64,
}

impl SimProvider {
    pub fn new() -> Self {
        Self::build(fastrand::Rng::new(), 8)
    }

    /// Deterministic sequence, for tests.
    pub fn with_seed(seed: u64) -> Self {
        Self::build(fastrand::Rng::with_seed(seed), 8)
    }

    fn build(mut rng: fastrand::Rng, cores: usize) -> Self {
        let core_bias = (0..cores).map(|_| rng.f64() * 2.0 - 1.0).collect();
        Self {
            rng,
            cores,
            keys: None,
            cpu: Drift::new(10.0),
            gpu: Drift::new(5.0),
            mem: Drift::new(12000.0),
            disk: Drift::new(2.0),
            net: Drift::new(5.0),
            core_bias,
            cpu_temp: 40.0,
            gpu_temp: 36.0,
        }
    }
}

impl Default for SimProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn reg_sensor(
    reg: &mut Registry,
    id: &str,
    group: Group,
    name: &str,
    unit: Unit,
    history: bool,
) -> SensorKey {
    let desc = SensorDesc::new(id, group, name, unit, Source::Sim);
    reg.register(if history { desc.with_history() } else { desc })
}

impl Provider for SimProvider {
    fn name(&self) -> &'static str {
        "sim"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        use Group::*;
        use Unit::*;
        let mut r = |id: &str, g, n: &str, u, h| reg_sensor(reg, id, g, n, u, h);
        let cpu_usage = r("cpu.usage.total", Cpu, "CPU Total", Percent, true);
        let cpu_clock = r("cpu.clock.current", Cpu, "CPU Clock", Mhz, true);
        let cpu_temp = r("cpu.temp.package", Cpu, "CPU Package", Celsius, true);
        let cpu_power = r("cpu.power.package", Cpu, "CPU Package Power", Watt, true);
        let cores = (0..self.cores)
            .map(|i| {
                (
                    r(
                        &format!("cpu.core{i}.usage"),
                        Cpu,
                        &format!("Core #{i} Usage"),
                        Percent,
                        false,
                    ),
                    r(
                        &format!("cpu.core{i}.clock"),
                        Cpu,
                        &format!("Core #{i} Clock"),
                        Mhz,
                        false,
                    ),
                    r(
                        &format!("cpu.core{i}.temp"),
                        Cpu,
                        &format!("Core #{i} Temp"),
                        Celsius,
                        false,
                    ),
                )
            })
            .collect();
        let keys = Keys {
            cpu_usage,
            cpu_clock,
            cpu_temp,
            cpu_power,
            cores,
            gpu_usage: r("gpu.0.usage", Gpu, "GPU Usage", Percent, true),
            gpu_clock: r("gpu.0.clock.core", Gpu, "GPU Core Clock", Mhz, true),
            gpu_mem_clock: r("gpu.0.clock.mem", Gpu, "GPU Memory Clock", Mhz, false),
            gpu_temp: r("gpu.0.temp.edge", Gpu, "GPU Temperature", Celsius, true),
            gpu_hotspot: r("gpu.0.temp.hotspot", Gpu, "GPU Hotspot", Celsius, true),
            gpu_vram: r("gpu.0.vram.used", Gpu, "VRAM Used", Megabyte, true),
            gpu_power: r("gpu.0.power", Gpu, "GPU Power", Watt, true),
            gpu_fan: r("gpu.0.fan", Gpu, "GPU Fan", Percent, false),
            mem_used: r("memory.used", Memory, "Memory Used", Megabyte, true),
            mem_available: r(
                "memory.available",
                Memory,
                "Memory Available",
                Megabyte,
                false,
            ),
            disk_temp: r("storage.0.temp", Storage, "SSD Temperature", Celsius, true),
            disk_read: r("storage.0.read", Storage, "SSD Read", MegabytePerSec, false),
            disk_write: r(
                "storage.0.write",
                Storage,
                "SSD Write",
                MegabytePerSec,
                false,
            ),
            fan_cpu: r("fans.cpu", Fans, "CPU_FAN", Rpm, true),
            fan_pump: r("fans.pump", Fans, "AIO_PUMP", Rpm, false),
            volt_12v: r("voltages.12v", Voltages, "+12V", Volt, false),
            volt_core: r("voltages.vcore", Voltages, "Vcore", Volt, true),
            net_down: r("net.0.down", Network, "Download", Megabit, true),
            net_up: r("net.0.up", Network, "Upload", Megabit, true),
            net_ping: r("net.0.ping", Network, "Ping", Millisecond, false),
            power_total: r("power.total", Power, "Estimated System Power", Watt, true),
        };
        self.keys = Some(keys);
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let Some(k) = &self.keys else {
            return Err("poll before init".into());
        };
        let rng = &mut self.rng;
        for d in [&mut self.cpu, &mut self.gpu] {
            if rng.f64() < 0.08 {
                d.target = pick_load(rng);
            }
        }
        if rng.f64() < 0.05 {
            self.mem.target = 9000.0 + rng.f64() * 9000.0;
        }
        for d in [&mut self.disk, &mut self.net] {
            d.target = if rng.f64() < 0.1 {
                50.0 + rng.f64() * 400.0
            } else {
                1.0
            };
        }

        // CPU
        let load = self.cpu.step(rng, 0.35, 3.0, 0.5, 100.0);
        let mhz = MHZ_BASE - 1200.0
            + load / 100.0 * (MHZ_BOOST - MHZ_BASE + 1200.0)
            + (rng.f64() - 0.5) * 120.0;
        self.cpu_temp = follow(self.cpu_temp, 36.0 + load * 0.52, 0.15) + (rng.f64() - 0.5) * 0.6;
        let cpu_w = 11.0 + load * 1.35;
        let cpu = &mut snap.cpu;
        cpu.usage_total = reg.set(k.cpu_usage, Some(load));
        cpu.mhz_current = reg.set(k.cpu_clock, Some(mhz));
        cpu.mhz_effective = Some(mhz * 0.985);
        cpu.multiplier = Some((mhz / 100.0).round());
        cpu.bclk_mhz = Some(100.0);
        cpu.mhz_base = Some(MHZ_BASE);
        cpu.mhz_boost = Some(MHZ_BOOST);
        cpu.temp_package = reg.set(k.cpu_temp, Some(self.cpu_temp));
        cpu.power_pkg_w = reg.set(k.cpu_power, Some(cpu_w));
        cpu.power_core_w = Some(cpu_w * 0.8);
        cpu.voltage_core = Some(0.9 + load * 0.0035);
        cpu.throttle_thermal = Some(self.cpu_temp > 95.0);
        cpu.throttle_power_limit = Some(false);
        cpu.throttle_current_limit = Some(false);
        for (i, (ku, kc, kt)) in k.cores.iter().enumerate() {
            let bias = self.core_bias[i];
            let u = (load + bias * 12.0 + (rng.f64() - 0.5) * 16.0).clamp(0.0, 100.0);
            let c = (mhz + bias * 150.0).clamp(800.0, MHZ_BOOST + 200.0);
            let t = self.cpu_temp + bias * 3.0;
            cpu.usage_per_core.extend(reg.set(*ku, Some(u)));
            cpu.mhz_per_core.extend(reg.set(*kc, Some(c)));
            cpu.temp_per_core.extend(reg.set(*kt, Some(t)));
        }

        // GPU
        let gload = self.gpu.step(rng, 0.3, 2.5, 0.0, 100.0);
        self.gpu_temp = follow(self.gpu_temp, 33.0 + gload * 0.42, 0.12) + (rng.f64() - 0.5) * 0.4;
        let gpu_w = 16.0 + gload * 1.9;
        let fan_pct = ((self.gpu_temp - 50.0) * 3.0 + 30.0).clamp(0.0, 100.0);
        let fan_pct = if self.gpu_temp < 50.0 { 0.0 } else { fan_pct };
        snap.gpu.push(GpuSnapshot {
            name: "Simulated GPU".into(),
            usage: reg.set(k.gpu_usage, Some(gload)),
            engine: GpuEngines {
                graphics3d: Some(gload),
                decode: Some(0.0),
                encode: Some(if gload > 50.0 { 3.0 } else { 0.0 }),
                compute: Some(0.0),
                copy: Some(gload * 0.05),
                tensor: None,
                raytrace: None,
            },
            mhz_core: reg.set(k.gpu_clock, Some(210.0 + gload / 100.0 * 2500.0)),
            mhz_mem: reg.set(
                k.gpu_mem_clock,
                Some(if gload > 8.0 { 10501.0 } else { 405.0 }),
            ),
            temp_edge: reg.set(k.gpu_temp, Some(self.gpu_temp)),
            temp_hotspot: reg.set(k.gpu_hotspot, Some(self.gpu_temp + 8.0 + gload * 0.1)),
            temp_mem: Some(self.gpu_temp + 6.0),
            vram_used_mb: reg.set(k.gpu_vram, Some(1100.0 + gload * 75.0)),
            vram_total_mb: Some(VRAM_TOTAL_MB),
            power_w: reg.set(k.gpu_power, Some(gpu_w)),
            power_limit_w: Some(200.0),
            voltage_v: Some(0.7 + gload * 0.0028),
            fan_pct: reg.set(k.gpu_fan, Some(fan_pct)),
            pcie_gen: Some(4),
            pcie_width: Some(16),
            throttle_thermal: Some(false),
            throttle_power_limit: Some(gpu_w > 195.0),
        });

        // Memory
        let used = self.mem.step(rng, 0.2, 40.0, 4000.0, MEM_TOTAL_MB - 1000.0);
        let mem = &mut snap.memory;
        mem.total_mb = Some(MEM_TOTAL_MB);
        mem.used_mb = reg.set(k.mem_used, Some(used));
        mem.available_mb = reg.set(k.mem_available, Some(MEM_TOTAL_MB - used));
        mem.cached_mb = Some(8100.0);
        mem.committed_mb = Some(used + 3000.0);
        mem.commit_limit_mb = Some(40000.0);
        mem.page_file_used_mb = Some(900.0);
        mem.compressed_mb = Some(420.0);
        mem.page_faults_per_sec = Some(800.0 + rng.f64() * 800.0);
        mem.channels = Some(2);
        mem.dimms = ["DIMM_A2", "DIMM_B2"]
            .iter()
            .map(|slot| DimmSnapshot {
                slot: (*slot).into(),
                manufacturer: Some("Simulated".into()),
                model: Some("SIM-6000-16".into()),
                size_mb: Some(16384.0),
                speed_mhz: Some(6000.0),
                temp_c: Some(38.0 + load * 0.05),
            })
            .collect();

        // Storage
        let io = self.disk.step(rng, 0.6, 0.5, 0.0, 3500.0);
        let disk_temp = 38.0 + io * 0.02;
        snap.storage.push(StorageSnapshot {
            model: "Simulated NVMe SSD 1TB".into(),
            kind: StorageKind::Nvme,
            temp_c: reg.set(k.disk_temp, Some(disk_temp)),
            temp_controller_c: Some(disk_temp + 2.0),
            health_pct: Some(99.0),
            capacity_mb: Some(1_000_204.0),
            used_mb: Some(420_000.0),
            free_mb: Some(580_204.0),
            read_mbs: reg.set(k.disk_read, Some(io * 0.7)),
            write_mbs: reg.set(k.disk_write, Some(io * 0.3)),
            read_iops: Some(io * 25.0),
            write_iops: Some(io * 8.0),
            activity_pct: Some((io / 35.0).min(100.0)),
            total_read_gb: Some(18200.0),
            total_write_gb: Some(9100.0),
            power_on_hours: Some(1180.0),
            power_cycles: Some(420.0),
            nvme_pct_used: Some(1.0),
            nvme_available_spare_pct: Some(100.0),
            nvme_critical_warning: Some(0),
            ..Default::default()
        });

        // Fans & voltages
        let cpu_rpm = 450.0 + (self.cpu_temp - 30.0).max(0.0) * 22.0;
        snap.fans.push(FanSnapshot {
            name: "CPU_FAN".into(),
            rpm: reg.set(k.fan_cpu, Some(cpu_rpm)),
            pct: Some((cpu_rpm / 2000.0 * 100.0).min(100.0)),
            pwm: Some((cpu_rpm / 2000.0 * 255.0).min(255.0).round()),
            min_rpm: Some(300.0),
            max_rpm: Some(2000.0),
            kind: FanKind::Fan,
            status: Status::Ok,
        });
        snap.fans.push(FanSnapshot {
            name: "AIO_PUMP".into(),
            rpm: reg.set(k.fan_pump, Some(2400.0 + (rng.f64() - 0.5) * 30.0)),
            pct: Some(80.0),
            pwm: Some(204.0),
            min_rpm: Some(1800.0),
            max_rpm: Some(3000.0),
            kind: FanKind::Pump,
            status: Status::Ok,
        });
        snap.voltages.push(VoltageReading {
            name: "+12V".into(),
            value: reg.set(k.volt_12v, Some(12.06 + (rng.f64() - 0.5) * 0.04)),
        });
        snap.voltages.push(VoltageReading {
            name: "Vcore".into(),
            value: reg.set(k.volt_core, snap.cpu.voltage_core),
        });

        // Network
        let down = self.net.step(rng, 0.5, 0.5, 0.0, 940.0);
        snap.network.push(NetworkSnapshot {
            adapter: "Ethernet (simulated)".into(),
            kind: NetKind::Ethernet,
            down_mbps: reg.set(k.net_down, Some(down)),
            up_mbps: reg.set(k.net_up, Some(down * 0.08)),
            utilization_pct: Some(down / 10.0),
            link_mbps: Some(1000.0),
            ping_ms: reg.set(k.net_ping, Some(12.0 + (rng.f64() - 0.5) * 6.0)),
            packet_loss_pct: Some(0.0),
            wifi_signal_pct: None,
            wifi_channel: None,
        });

        // Power
        let total = cpu_w + gpu_w + 45.0;
        snap.power.cpu_w = Some(cpu_w);
        snap.power.gpu_w = Some(gpu_w);
        snap.power.est_total_w = reg.set(k.power_total, Some(total));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_within_physical_bounds() {
        let mut reg = Registry::new();
        let mut sim = SimProvider::with_seed(42);
        sim.init(&mut reg).unwrap();
        for _ in 0..500 {
            let mut snap = SensorSnapshot::default();
            sim.poll(&mut reg, &mut snap).unwrap();
            let cpu = &snap.cpu;
            assert!((0.0..=100.0).contains(&cpu.usage_total.unwrap()));
            assert_eq!(cpu.usage_per_core.len(), 8);
            assert!((800.0..=MHZ_BOOST + 200.0).contains(&cpu.mhz_current.unwrap()));
            assert!((20.0..=100.0).contains(&cpu.temp_package.unwrap()));
            let gpu = &snap.gpu[0];
            assert!(gpu.vram_used_mb.unwrap() <= gpu.vram_total_mb.unwrap());
            assert!(gpu.engine.tensor.is_none());
            let mem = &snap.memory;
            assert!(mem.used_mb.unwrap() + mem.available_mb.unwrap() <= MEM_TOTAL_MB + 0.1);
        }
        assert!(reg.len() > 30);
        assert_eq!(reg.available_count(), reg.len());
    }

    #[test]
    fn poll_before_init_errors() {
        let mut sim = SimProvider::with_seed(0);
        assert!(sim
            .poll(&mut Registry::new(), &mut SensorSnapshot::default())
            .is_err());
    }
}
