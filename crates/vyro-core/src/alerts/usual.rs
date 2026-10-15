//! Usual readings for "above usual" conditions (P18). A CPU or GPU temperature is compared with
//! what the past week says it runs at for the load of the moment (the CPU's load for `cpu.*`,
//! GPU n's for `gpu.n.*`); anything else with its median over the week. The intelligence pass
//! works these out every ten minutes; the rules apply them each tick with that tick's load.

use crate::intel::stats::{fit, join, median};
use crate::intel::SeriesMap;

/// How much history a usual reading needs (about a day of week buckets).
pub const MIN_SPAN_SECS: i64 = 23 * 3600;
/// The last hour is left out, so what is happening now does not set its own usual.
pub const GAP_SECS: i64 = 3600;
/// Fewest week buckets (about 17 minutes each) behind a usual reading.
const MIN_POINTS: usize = 24;

#[derive(Debug, Clone, PartialEq)]
pub enum Usual {
    /// `a + b · load`, with `load` the reading of `load_id`.
    ForLoad { a: f64, b: f64, load_id: String },
    /// One level whatever the load.
    Level(f64),
}

impl Usual {
    /// The usual reading now; `None` without the load reading it depends on.
    pub fn now(&self, read: impl Fn(&str) -> Option<f64>) -> Option<f64> {
        match self {
            Self::Level(v) => Some(*v),
            Self::ForLoad { a, b, load_id } => {
                read(load_id).filter(|l| l.is_finite()).map(|l| a + b * l)
            }
        }
    }
}

/// The load a sensor's temperature follows: the CPU's for `cpu.*`, GPU n's for `gpu.n.*`.
pub fn load_of(sensor: &str) -> Option<String> {
    let mut it = sensor.split('.');
    match it.next()? {
        "cpu" => Some("cpu.usage.total".into()),
        "gpu" => {
            let n = it.next()?;
            n.parse::<u32>().ok()?;
            Some(format!("gpu.{n}.usage"))
        }
        _ => None,
    }
}

/// `sensor`'s usual reading from the week's series, as of `now`; `celsius`: it is a
/// temperature, so its load counts. `None` until there is about a day of history.
pub fn usual(week: &SeriesMap, sensor: &str, celsius: bool, now: i64) -> Option<Usual> {
    let before = now - GAP_SECS;
    let pts: Vec<(i64, f64)> = week
        .get(sensor)?
        .iter()
        .filter(|p| p.ts < before && p.avg.is_finite())
        .map(|p| (p.ts, p.avg))
        .collect();
    let (first, last) = (pts.first()?.0, pts.last()?.0);
    if pts.len() < MIN_POINTS || last - first < MIN_SPAN_SECS {
        return None;
    }
    if celsius {
        if let Some(load_id) = load_of(sensor).filter(|l| week.contains_key(l)) {
            let pairs: Vec<(f64, f64)> = join(week, &load_id, sensor)
                .into_iter()
                .filter(|(ts, l, t)| *ts < before && l.is_finite() && t.is_finite())
                .map(|(_, l, t)| (l, t))
                .collect();
            if pairs.len() >= MIN_POINTS {
                let (a, b, _) = fit(&pairs)?;
                return Some(Usual::ForLoad { a, b, load_id });
            }
        }
    }
    median(pts.into_iter().map(|p| p.1).collect()).map(Usual::Level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryPoint;

    const NOW: i64 = 1_800_000_000;
    /// The week query's bucket.
    const STEP: i64 = 1020;

    fn series(hours: i64, f: impl Fn(i64, usize) -> f64) -> Vec<HistoryPoint> {
        let n = hours * 3600 / STEP;
        (0..n)
            .map(|i| {
                let ts = NOW - (n - i) * STEP;
                let avg = f(ts, usize::try_from(i).unwrap());
                HistoryPoint {
                    ts,
                    avg,
                    min: avg,
                    max: avg,
                }
            })
            .collect()
    }

    #[test]
    fn a_fan_against_its_median_a_temperature_against_its_load() {
        let mut week = SeriesMap::new();
        week.insert("fan.0".into(), series(72, |_, _| 1200.0));
        // Load swings 10–90 %; the CPU runs at 40 °C plus 0.4 °C per percent.
        let load = |i: usize| 10.0 + f64::from(u8::try_from(i % 9).unwrap()) * 10.0;
        week.insert("cpu.usage.total".into(), series(72, |_, i| load(i)));
        week.insert(
            "cpu.temp.package".into(),
            series(72, |_, i| 40.0 + 0.4 * load(i)),
        );
        assert_eq!(
            usual(&week, "fan.0", false, NOW),
            Some(Usual::Level(1200.0))
        );
        let u = usual(&week, "cpu.temp.package", true, NOW).unwrap();
        let at = |l: f64| u.now(|id| (id == "cpu.usage.total").then_some(l)).unwrap();
        assert!((at(50.0) - 60.0).abs() < 0.01, "{u:?}");
        assert!((at(90.0) - 76.0).abs() < 0.01);
        assert_eq!(u.now(|_| None), None, "no load reading, no usual");
    }

    #[test]
    fn needs_about_a_day_and_leaves_out_the_last_hour() {
        let mut week = SeriesMap::new();
        week.insert("fan.0".into(), series(12, |_, _| 900.0));
        assert_eq!(
            usual(&week, "fan.0", false, NOW),
            None,
            "12 h is too little"
        );
        // A day at 900 RPM, then a stall in the last hour: the usual stays 900.
        week.insert(
            "fan.0".into(),
            series(30, |ts, _| if ts >= NOW - 3600 { 0.0 } else { 900.0 }),
        );
        assert_eq!(usual(&week, "fan.0", false, NOW), Some(Usual::Level(900.0)));
    }

    #[test]
    fn which_load_a_temperature_follows() {
        assert_eq!(load_of("cpu.temp.ccd0").as_deref(), Some("cpu.usage.total"));
        assert_eq!(
            load_of("gpu.1.temp.hotspot").as_deref(),
            Some("gpu.1.usage")
        );
        assert_eq!(load_of("board.temp1"), None);
        assert_eq!(load_of("gpu.x.temp"), None);
    }
}
