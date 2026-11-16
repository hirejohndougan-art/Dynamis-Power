//! "Runs hotter than it used to": the temperature at the same light use,
//! month by month. A steady rise of several degrees usually means dust in
//! the fan or old thermal paste; the app says so in plain words. Pure: the
//! service adds readings and saves the totals.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 1;
/// Readings a month needs before it counts.
const MIN_READINGS: u64 = 200;
/// How much hotter is worth saying, °C.
const NOTABLE_C: f64 = 5.0;

/// "2026-11" for a Unix time (UTC; a month is long enough not to care).
pub fn month_key(unix: f64) -> String {
    // Days to civil date (Howard Hinnant's algorithm).
    let z = (unix / 86_400.0).floor() as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}")
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeatHistory {
    pub v: u32,
    /// Month → (sum of °C, readings), at light use only.
    pub months: BTreeMap<String, (f64, u64)>,
}

/// What the app shows when the laptop runs notably hotter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hotter {
    pub by_c: f64,
    /// The month compared with.
    pub since: String,
}

impl HeatHistory {
    /// One reading at light use (the caller decides what light use is).
    pub fn add(&mut self, unix: f64, temp_c: f64) {
        if !(10.0..=110.0).contains(&temp_c) {
            return;
        }
        self.v = FORMAT_VERSION;
        let e = self.months.entry(month_key(unix)).or_insert((0.0, 0));
        e.0 += temp_c;
        e.1 += 1;
        // Keep about three years.
        while self.months.len() > 36 {
            let first = self.months.keys().next().cloned();
            if let Some(k) = first {
                self.months.remove(&k);
            }
        }
    }

    /// The first month with enough readings against the latest one, if the
    /// latest is notably hotter (at least two months apart).
    pub fn hotter(&self) -> Option<Hotter> {
        let full: Vec<(&String, f64)> = self
            .months
            .iter()
            .filter(|(_, (_, n))| *n >= MIN_READINGS)
            .map(|(k, (s, n))| (k, s / *n as f64))
            .collect();
        let (first, last) = (full.first()?, full.last()?);
        if full.len() < 3 {
            return None;
        }
        let by = last.1 - first.1;
        (by >= NOTABLE_C).then(|| Hotter {
            by_c: (by * 10.0).round() / 10.0,
            since: first.0.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months() {
        assert_eq!(month_key(0.0), "1970-01");
        assert_eq!(month_key(1_795_000_000.0), "2026-11");
    }

    #[test]
    fn notably_hotter_after_months() {
        let mut h = HeatHistory::default();
        let month = 31.0 * 86_400.0;
        for (i, t) in [50.0, 51.0, 52.0, 57.0].iter().enumerate() {
            for _ in 0..250 {
                h.add(1_780_000_000.0 + i as f64 * month, *t);
            }
        }
        let x = h.hotter().unwrap();
        assert_eq!(x.by_c, 7.0);
        let mut cool = HeatHistory::default();
        for i in 0..4 {
            for _ in 0..250 {
                cool.add(1_780_000_000.0 + i as f64 * month, 50.0);
            }
        }
        assert!(cool.hotter().is_none());
    }
}
