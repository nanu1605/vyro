//! Unified sensor registry (spec §4.1).
//!
//! Every metric any provider can produce is registered here once. The poller updates values
//! each tick; min / max / avg are tracked centrally. The registry backs the Sensor Explorer,
//! the history store, health and alerts.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Display grouping. Declaration order is the order groups appear in the explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Group {
    Cpu,
    Gpu,
    Memory,
    Storage,
    Board,
    Fans,
    Voltages,
    Network,
    Power,
    System,
    App,
}

/// Unit, serialized as its display symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum Unit {
    #[serde(rename = "%")]
    Percent,
    #[serde(rename = "MHz")]
    Mhz,
    #[serde(rename = "°C")]
    Celsius,
    #[serde(rename = "W")]
    Watt,
    #[serde(rename = "V")]
    Volt,
    #[serde(rename = "RPM")]
    Rpm,
    #[serde(rename = "MB")]
    Megabyte,
    #[serde(rename = "MB/s")]
    MegabytePerSec,
    #[serde(rename = "Mbps")]
    Megabit,
    #[serde(rename = "ms")]
    Millisecond,
    #[serde(rename = "/s")]
    PerSecond,
    #[serde(rename = "")]
    Count,
}

/// Where a reading comes from (shown in the explorer; useful for field debugging).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Source {
    Sim,
    Os,
    Pdh,
    Nvml,
    Nvapi,
    Adl,
    Adlx,
    Igcl,
    Smart,
    Driver,
    App,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Sensor {
    /// Stable dotted id, e.g. `cpu.usage.total`, `gpu.0.temp.edge`.
    pub id: String,
    pub group: Group,
    pub name: String,
    pub unit: Unit,
    pub value: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Mean of all readings since start (or the last stats reset).
    pub avg: Option<f64>,
    pub source: Source,
    /// Whether the latest poll produced a value.
    pub available: bool,
    /// Readings persist to the history store (queryable with `query_history`).
    pub history: bool,
    #[serde(skip)]
    #[ts(skip)]
    sum: f64,
    #[serde(skip)]
    #[ts(skip)]
    count: u64,
}

/// What a provider declares when registering a sensor.
#[derive(Debug, Clone)]
pub struct SensorDesc {
    pub id: String,
    pub group: Group,
    pub name: String,
    pub unit: Unit,
    pub source: Source,
    /// Persist readings to the history store.
    pub history: bool,
}

impl SensorDesc {
    pub fn new(
        id: impl Into<String>,
        group: Group,
        name: impl Into<String>,
        unit: Unit,
        source: Source,
    ) -> Self {
        Self {
            id: id.into(),
            group,
            name: name.into(),
            unit,
            source,
            history: false,
        }
    }

    pub fn with_history(mut self) -> Self {
        self.history = true;
        self
    }
}

/// Cheap handle to a registered sensor; avoids hashing the id on every update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SensorKey(usize);

#[derive(Debug, Default)]
pub struct Registry {
    sensors: IndexMap<String, Sensor>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a sensor. Idempotent: registering an existing id updates its metadata and
    /// returns the existing key, keeping accumulated stats.
    pub fn register(&mut self, desc: SensorDesc) -> SensorKey {
        let entry = self.sensors.entry(desc.id.clone());
        let index = entry.index();
        let sensor = entry.or_insert_with(|| Sensor {
            id: desc.id.clone(),
            group: desc.group,
            name: String::new(),
            unit: desc.unit,
            value: None,
            min: None,
            max: None,
            avg: None,
            source: desc.source,
            available: false,
            history: false,
            sum: 0.0,
            count: 0,
        });
        sensor.group = desc.group;
        sensor.name = desc.name;
        sensor.unit = desc.unit;
        sensor.source = desc.source;
        sensor.history = desc.history;
        SensorKey(index)
    }

    /// Records a reading and returns it unchanged, so providers can write the registry and
    /// the snapshot in one expression: `snap.cpu.usage_total = reg.set(k, v);`.
    /// `None` and non-finite values mark the sensor unavailable and leave stats untouched.
    pub fn set(&mut self, key: SensorKey, value: Option<f64>) -> Option<f64> {
        let value = value.filter(|v| v.is_finite());
        let Some((_, sensor)) = self.sensors.get_index_mut(key.0) else {
            return value;
        };
        sensor.value = value;
        sensor.available = value.is_some();
        if let Some(v) = value {
            sensor.min = Some(sensor.min.map_or(v, |m| m.min(v)));
            sensor.max = Some(sensor.max.map_or(v, |m| m.max(v)));
            sensor.sum += v;
            sensor.count += 1;
            sensor.avg = Some(sensor.sum / sensor.count as f64);
        }
        value
    }

    pub fn key(&self, id: &str) -> Option<SensorKey> {
        self.sensors.get_index_of(id).map(SensorKey)
    }

    pub fn get(&self, id: &str) -> Option<&Sensor> {
        self.sensors.get(id)
    }

    /// Clears min / max / avg for every sensor; current values are kept.
    pub fn reset_stats(&mut self) {
        for sensor in self.sensors.values_mut() {
            sensor.min = sensor.value;
            sensor.max = sensor.value;
            sensor.avg = sensor.value;
            sensor.sum = sensor.value.unwrap_or(0.0);
            sensor.count = u64::from(sensor.value.is_some());
        }
    }

    pub fn len(&self) -> usize {
        self.sensors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sensors.is_empty()
    }

    pub fn available_count(&self) -> usize {
        self.sensors.values().filter(|s| s.available).count()
    }

    /// Current values of history-tracked sensors that have a reading.
    pub fn history_values(&self) -> impl Iterator<Item = (&str, f64)> {
        self.sensors
            .values()
            .filter(|s| s.history)
            .filter_map(|s| s.value.map(|v| (s.id.as_str(), v)))
    }

    /// Grouped copy for the Sensor Explorer.
    pub fn tree(&self) -> SensorTree {
        let mut groups: Vec<SensorGroup> = Vec::new();
        for sensor in self.sensors.values() {
            match groups.iter_mut().find(|g| g.group == sensor.group) {
                Some(g) => g.sensors.push(sensor.clone()),
                None => groups.push(SensorGroup {
                    group: sensor.group,
                    sensors: vec![sensor.clone()],
                }),
            }
        }
        groups.sort_by_key(|g| g.group);
        SensorTree { groups }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SensorTree {
    pub groups: Vec<SensorGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SensorGroup {
    pub group: Group,
    pub sensors: Vec<Sensor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(id: &str) -> SensorDesc {
        SensorDesc::new(id, Group::Cpu, "Temp", Unit::Celsius, Source::Sim)
    }

    #[test]
    fn tracks_min_max_avg() {
        let mut reg = Registry::new();
        let k = reg.register(temp("cpu.temp"));
        for v in [50.0, 70.0, 60.0] {
            reg.set(k, Some(v));
        }
        let s = reg.get("cpu.temp").unwrap();
        assert_eq!(s.value, Some(60.0));
        assert_eq!(s.min, Some(50.0));
        assert_eq!(s.max, Some(70.0));
        assert_eq!(s.avg, Some(60.0));
        assert!(s.available);
    }

    #[test]
    fn missing_and_non_finite_readings_mark_unavailable_without_touching_stats() {
        let mut reg = Registry::new();
        let k = reg.register(temp("cpu.temp"));
        reg.set(k, Some(40.0));
        assert_eq!(reg.set(k, None), None);
        assert_eq!(reg.set(k, Some(f64::NAN)), None);
        let s = reg.get("cpu.temp").unwrap();
        assert!(!s.available);
        assert_eq!(s.value, None);
        assert_eq!((s.min, s.max, s.avg), (Some(40.0), Some(40.0), Some(40.0)));
        assert_eq!(reg.available_count(), 0);
    }

    #[test]
    fn register_is_idempotent_and_keeps_stats() {
        let mut reg = Registry::new();
        let a = reg.register(temp("cpu.temp"));
        reg.set(a, Some(55.0));
        let b = reg.register(temp("cpu.temp").with_history());
        assert_eq!(a, b);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("cpu.temp").unwrap().max, Some(55.0));
        assert_eq!(
            reg.history_values().collect::<Vec<_>>(),
            [("cpu.temp", 55.0)]
        );
    }

    #[test]
    fn reset_stats_restarts_from_current_value() {
        let mut reg = Registry::new();
        let k = reg.register(temp("cpu.temp"));
        reg.set(k, Some(90.0));
        reg.set(k, Some(50.0));
        reg.reset_stats();
        reg.set(k, Some(60.0));
        let s = reg.get("cpu.temp").unwrap();
        assert_eq!((s.min, s.max, s.avg), (Some(50.0), Some(60.0), Some(55.0)));
    }

    #[test]
    fn history_values_only_include_tracked_sensors_with_readings() {
        let mut reg = Registry::new();
        let tracked = reg.register(temp("a").with_history());
        let untracked = reg.register(temp("b"));
        reg.register(temp("c").with_history());
        reg.set(tracked, Some(1.0));
        reg.set(untracked, Some(2.0));
        assert_eq!(reg.history_values().collect::<Vec<_>>(), [("a", 1.0)]);
    }

    #[test]
    fn tree_groups_in_declaration_order() {
        let mut reg = Registry::new();
        reg.register(SensorDesc::new(
            "gpu.0.usage",
            Group::Gpu,
            "Usage",
            Unit::Percent,
            Source::Sim,
        ));
        reg.register(temp("cpu.temp"));
        reg.register(SensorDesc::new(
            "cpu.usage",
            Group::Cpu,
            "Usage",
            Unit::Percent,
            Source::Sim,
        ));
        let tree = reg.tree();
        let order: Vec<_> = tree.groups.iter().map(|g| g.group).collect();
        assert_eq!(order, [Group::Cpu, Group::Gpu]);
        let cpu_ids: Vec<_> = tree.groups[0]
            .sensors
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(cpu_ids, ["cpu.temp", "cpu.usage"]);
    }
}
