//! Characterising the battery gauge.
//!
//! Gauges differ: some update power every second or two, others every 30 s
//! or more, and resolution varies. Experiments must be sized to the gauge
//! (see `docs/MEASUREMENT.md`), so phase 1 measures it.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GaugeProfile {
    pub samples: usize,
    /// How many times the reading changed.
    pub changes: usize,
    /// Typical (median) seconds between changes.
    pub update_interval_s: Option<f64>,
    /// Smallest change seen, in watts.
    pub resolution_w: Option<f64>,
    /// Plain-language verdict for test sizing.
    pub verdict: String,
}

/// Analyse a series of (time in seconds, power in watts) readings.
pub fn analyse(series: &[(f64, f64)]) -> GaugeProfile {
    let mut change_times = Vec::new();
    let mut steps = Vec::new();
    for w in series.windows(2) {
        let (_, a) = w[0];
        let (t, b) = w[1];
        let d = (b - a).abs();
        if d > 1e-9 {
            change_times.push(t);
            steps.push(d);
        }
    }
    let mut gaps: Vec<f64> = change_times.windows(2).map(|w| w[1] - w[0]).collect();
    let update_interval_s = median(&mut gaps).map(|v| (v * 10.0).round() / 10.0);
    let resolution_w = steps
        .iter()
        .cloned()
        .fold(None, |acc: Option<f64>, d| {
            Some(acc.map_or(d, |a| a.min(d)))
        })
        .map(|v| (v * 1000.0).round() / 1000.0);

    let verdict = match update_interval_s {
        None if series.len() < 3 => "Not enough readings yet.".to_string(),
        None => "The reading never changed. On battery, try a longer check; plugged in, many gauges report nothing useful.".to_string(),
        Some(i) if i <= 5.0 => format!("Fast gauge (updates about every {i} s): short tests work well."),
        Some(i) if i <= 20.0 => format!("Medium gauge (about every {i} s): tests need a few minutes each."),
        Some(i) => format!("Slow gauge (about every {i} s): tests need long blocks; CPU counters will matter more."),
    };

    GaugeProfile {
        samples: series.len(),
        changes: change_times.len(),
        update_interval_s,
        resolution_w,
        verdict,
    }
}

fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_update_interval_and_resolution() {
        // One reading per second; the gauge changes every 10 s by at least 0.05 W.
        let series: Vec<(f64, f64)> = (0..60)
            .map(|t| (t as f64, 5.0 + 0.05 * ((t / 10) % 3) as f64))
            .collect();
        let g = analyse(&series);
        assert_eq!(g.update_interval_s, Some(10.0));
        assert_eq!(g.resolution_w, Some(0.05));
        assert!(g.verdict.starts_with("Medium"));
    }

    #[test]
    fn constant_reading() {
        let series: Vec<(f64, f64)> = (0..10).map(|t| (t as f64, 5.0)).collect();
        let g = analyse(&series);
        assert_eq!(g.changes, 0);
        assert_eq!(g.update_interval_s, None);
    }
}
