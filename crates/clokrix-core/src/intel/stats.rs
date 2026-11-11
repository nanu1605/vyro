//! Small statistics on history series: joining two metrics, means and percentiles, a
//! straight-line fit and the search for a lasting shift.

use std::collections::{BTreeMap, HashMap};

use super::SeriesMap;

/// Bucket averages of `a` and `b` at the same timestamps.
pub fn join(map: &SeriesMap, a: &str, b: &str) -> Vec<(i64, f64, f64)> {
    let (Some(a), Some(b)) = (map.get(a), map.get(b)) else {
        return Vec::new();
    };
    let by_ts: HashMap<i64, f64> = b.iter().map(|p| (p.ts, p.avg)).collect();
    a.iter()
        .filter_map(|p| by_ts.get(&p.ts).map(|&v| (p.ts, p.avg, v)))
        .collect()
}

#[allow(clippy::cast_precision_loss)]
pub fn mean(v: impl IntoIterator<Item = f64>) -> Option<f64> {
    let (sum, n) = v
        .into_iter()
        .fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
    (n > 0).then(|| sum / n as f64)
}

pub fn percentile(mut v: Vec<f64>, p: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let i = ((v.len() - 1) as f64 * p).round() as usize;
    v.get(i).copied()
}

pub fn median(v: Vec<f64>) -> Option<f64> {
    percentile(v, 0.5)
}

/// Median absolute deviation from the median.
pub fn mad(v: &[f64]) -> Option<f64> {
    let m = median(v.to_vec())?;
    median(v.iter().map(|x| (x - m).abs()).collect())
}

/// Standard deviation over the mean.
pub fn variation(v: &[f64]) -> Option<f64> {
    let m = mean(v.iter().copied())?;
    if v.len() < 2 || m.abs() < f64::EPSILON {
        return None;
    }
    let var = mean(v.iter().map(|x| (x - m).powi(2)))?;
    Some(var.sqrt() / m)
}

/// Least-squares `y = a + b·x`, and the residual standard deviation.
#[allow(clippy::cast_precision_loss)]
pub fn fit(points: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = points.len() as f64;
    let mx = mean(points.iter().map(|p| p.0))?;
    let my = mean(points.iter().map(|p| p.1))?;
    let sxx: f64 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    // Too little spread in load to tell its effect: compare with the plain average.
    let b = if sxx / n >= 25.0 { sxy / sxx } else { 0.0 };
    let a = my - b * mx;
    let var = points
        .iter()
        .map(|p| (p.1 - a - b * p.0).powi(2))
        .sum::<f64>()
        / n;
    Some((a, b, var.sqrt()))
}

/// Means of `points` (time, value) per `width` seconds, in time order.
pub fn buckets(points: &[(i64, f64)], width: i64) -> Vec<(i64, f64)> {
    let mut by: BTreeMap<i64, (f64, u32)> = BTreeMap::new();
    for &(ts, v) in points {
        let e = by.entry(ts.div_euclid(width) * width).or_default();
        e.0 += v;
        e.1 += 1;
    }
    by.into_iter()
        .map(|(ts, (s, n))| (ts, s / f64::from(n)))
        .collect()
}

/// A lasting shift found in a series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shift {
    /// Means before and after the split.
    pub before: f64,
    pub after: f64,
    /// Spread of the points around their own side's mean.
    pub sigma: f64,
    /// The last point before the split and the first one after it (time).
    pub last_before: i64,
    pub first_after: i64,
}

/// The split of a time-ordered series (points `res` seconds wide) that best separates the
/// mean before it from the mean after it, with at least `min_before` and `min_after` seconds
/// on each side. Gaps (the PC off) are fine: the split then falls in the gap.
#[allow(clippy::cast_precision_loss)]
pub fn change_point(
    points: &[(i64, f64)],
    res: i64,
    min_before: i64,
    min_after: i64,
) -> Option<Shift> {
    let n = points.len();
    if n < 4 {
        return None;
    }
    let mut prefix = vec![(0.0, 0.0); n + 1];
    for (i, &(_, v)) in points.iter().enumerate() {
        prefix[i + 1] = (prefix[i].0 + v, prefix[i].1 + v * v);
    }
    let end = points[n - 1].0 + res;
    let mut best: Option<(f64, usize)> = None;
    for k in 2..n - 1 {
        if points[k].0 - points[0].0 < min_before || end - points[k].0 < min_after {
            continue;
        }
        let (kb, ka) = (k as f64, (n - k) as f64);
        let mb = prefix[k].0 / kb;
        let ma = (prefix[n].0 - prefix[k].0) / ka;
        // Weighted by the sizes of both sides, so a lone odd point does not win.
        let score = (ma - mb).abs() * (kb * ka / n as f64).sqrt();
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, k));
        }
    }
    let (_, k) = best?;
    let (kb, ka) = (k as f64, (n - k) as f64);
    let before = prefix[k].0 / kb;
    let after = (prefix[n].0 - prefix[k].0) / ka;
    let ss_before = prefix[k].1 - kb * before * before;
    let ss_after = (prefix[n].1 - prefix[k].1) - ka * after * after;
    let sigma = ((ss_before + ss_after).max(0.0) / (n as f64 - 2.0)).sqrt();
    Some(Shift {
        before,
        after,
        sigma,
        last_before: points[k - 1].0,
        first_after: points[k].0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_where_a_series_stepped_and_how_far() {
        // Hourly points: 40 for two days, then 48 for a day, with some wobble.
        let pts: Vec<(i64, f64)> = (0..72)
            .map(|h| {
                let wobble = if h % 2 == 0 { 0.5 } else { -0.5 };
                (h * 3600, if h < 48 { 40.0 } else { 48.0 } + wobble)
            })
            .collect();
        let s = change_point(&pts, 3600, 6 * 3600, 6 * 3600).unwrap();
        assert_eq!(s.first_after, 48 * 3600);
        assert_eq!(s.last_before, 47 * 3600);
        assert!((s.after - s.before - 8.0).abs() < 0.1, "{s:?}");
        assert!(s.sigma < 0.6, "{s:?}");
        // A step in the last two hours is too recent for a 6-hour minimum.
        let late: Vec<(i64, f64)> = (0..72)
            .map(|h| (h * 3600, if h < 70 { 40.0 } else { 48.0 }))
            .collect();
        let s = change_point(&late, 3600, 6 * 3600, 6 * 3600).unwrap();
        assert!(s.first_after <= 66 * 3600);
        assert!((s.after - s.before) < 4.0, "{s:?}");
    }

    #[test]
    fn helpers() {
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(mad(&[1.0, 2.0, 3.0, 4.0, 100.0]), Some(1.0));
        assert_eq!(
            buckets(&[(0, 1.0), (10, 3.0), (60, 5.0)], 60),
            [(0, 2.0), (60, 5.0)]
        );
        assert!(variation(&[100.0, 100.0]).unwrap() < 1e-9);
        let (a, b, s) = fit(&[(0.0, 35.0), (50.0, 55.0), (100.0, 75.0)]).unwrap();
        assert!((a - 35.0).abs() < 1e-9 && (b - 0.4).abs() < 1e-9 && s < 1e-9);
    }
}
