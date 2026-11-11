//! Display units. The engine measures, stores and compares temperatures in °C; °F is only a
//! way of showing them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum TempUnit {
    #[default]
    C,
    F,
}

pub fn c_to_f(c: f64) -> f64 {
    c * 9.0 / 5.0 + 32.0
}

/// Rewrites every "<number> °C" in a message (health findings, alert texts) in `unit`,
/// keeping the number of decimals. Numbers without their own "°C" are left alone.
pub fn temps_in(text: &str, unit: TempUnit) -> String {
    if unit == TempUnit::C {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("°C") {
        let (before, after) = rest.split_at(at);
        let trimmed = before.trim_end_matches(' ');
        let start = trimmed
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_digit() || *c == '.' || *c == '-')
            .last()
            .map_or(trimmed.len(), |(i, _)| i);
        let number = &trimmed[start..];
        match number.parse::<f64>() {
            Ok(c) if !number.is_empty() => {
                let decimals = number.split_once('.').map_or(0, |(_, d)| d.len());
                out.push_str(&trimmed[..start]);
                out.push_str(&format!("{:.*}", decimals, c_to_f(c)));
                out.push_str(&before[trimmed.len()..]);
                out.push_str("°F");
            }
            _ => {
                out.push_str(before);
                out.push_str("°C");
            }
        }
        rest = &after["°C".len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_each_celsius_reading_and_keeps_its_precision() {
        assert_eq!(
            temps_in(
                "CPU (Tdie) at 95.4 °C (above 95.0 °C for 10 s)",
                TempUnit::F
            ),
            "CPU (Tdie) at 203.7 °F (above 203.0 °F for 10 s)"
        );
        assert_eq!(
            temps_in("CPU running hot: 91 °C", TempUnit::F),
            "CPU running hot: 196 °F"
        );
        assert_eq!(temps_in("DIMM at -5°C", TempUnit::F), "DIMM at 23°F");
        assert_eq!(temps_in("no reading °C", TempUnit::F), "no reading °C");
        assert_eq!(temps_in("12 V and 40 °C", TempUnit::C), "12 V and 40 °C");
        assert_eq!(temps_in("fans 3 of 4", TempUnit::F), "fans 3 of 4");
    }
}
