//! Summaries of a recorded log: the phase 1 "power report".
//!
//! Columns are found by name from the CSV header, so reports work across log
//! format versions (a column missing from an older log is simply skipped).

use serde::Serialize;

use crate::gauge::{self, GaugeProfile};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Stat {
    pub mean: f64,
    pub median: f64,
    pub min: f64,
    pub max: f64,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Report {
    pub log_format: Option<u32>,
    pub rows: usize,
    pub duration_h: f64,
    pub on_battery_share_pct: f64,
    /// Whole-system power, battery rows only.
    pub battery_w: Option<Stat>,
    /// Battery charge used, in percentage points, over the battery rows.
    pub battery_pct_used: Option<i64>,
    pub cpu_pkg_w: Option<Stat>,
    pub cpu_avg_mhz: Option<Stat>,
    pub stall_cpu_pct: Option<Stat>,
    pub stall_mem_pct: Option<Stat>,
    pub stall_io_pct: Option<Stat>,
    pub max_temp_c: Option<Stat>,
    pub brightness_pct: Option<Stat>,
    pub self_cpu_pct: Option<Stat>,
    pub self_rss_mb: Option<Stat>,
    pub gauge: GaugeProfile,
}

#[derive(Debug)]
pub struct ReportError(pub String);

impl std::fmt::Display for ReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReportError {}

/// Summarise the text of a CSV log written by `dynamis log`.
pub fn from_csv(text: &str) -> Result<Report, ReportError> {
    let mut log_format = None;
    let mut header: Option<Vec<&str>> = None;
    let mut rows: Vec<Vec<&str>> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("# dynamis log format ") {
            log_format = rest.trim().parse().ok();
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if header.is_none() {
            header = Some(line.split(',').collect());
        } else {
            let row: Vec<&str> = line.split(',').collect();
            // A row with the wrong number of fields is a line cut short by a
            // freeze or power cut. Skip it rather than misread it.
            if header.as_ref().is_some_and(|h| h.len() == row.len()) {
                rows.push(row);
            }
        }
    }
    let header = header
        .ok_or_else(|| ReportError("no header row found; is this a dynamis CSV log?".into()))?;
    if !header.contains(&"time") {
        return Err(ReportError(
            "header has no 'time' column; is this a dynamis CSV log?".into(),
        ));
    }
    if rows.is_empty() {
        return Err(ReportError("the log has no readings yet".into()));
    }

    let col = |name: &str| header.iter().position(|h| *h == name);
    let num = |row: &Vec<&str>, name: &str| -> Option<f64> { row.get(col(name)?)?.parse().ok() };
    let on_ac = |row: &Vec<&str>| {
        col("on_ac")
            .and_then(|i| row.get(i))
            .map(|v| *v == "true")
            .unwrap_or(false)
    };

    let battery_rows: Vec<&Vec<&str>> = rows.iter().filter(|r| !on_ac(r)).collect();
    let series = |name: &str, only_battery: bool| -> Vec<f64> {
        let src: Box<dyn Iterator<Item = &Vec<&str>>> = if only_battery {
            Box::new(battery_rows.iter().copied())
        } else {
            Box::new(rows.iter())
        };
        src.filter_map(|r| num(r, name)).collect()
    };

    let intervals: Vec<f64> = rows.iter().filter_map(|r| num(r, "interval_s")).collect();
    let duration_s = intervals.iter().sum::<f64>();

    let battery_pct: Vec<f64> = series("battery_pct", true);
    let battery_pct_used = match (battery_pct.first(), battery_pct.last()) {
        (Some(a), Some(b)) if battery_pct.len() > 1 => Some((a - b).round() as i64),
        _ => None,
    };

    let gauge_series: Vec<(f64, f64)> = battery_rows
        .iter()
        .filter_map(|r| Some((num(r, "time")?, num(r, "battery_w")?)))
        .collect();

    Ok(Report {
        log_format,
        rows: rows.len(),
        duration_h: round(duration_s / 3600.0, 3),
        on_battery_share_pct: round(battery_rows.len() as f64 / rows.len() as f64 * 100.0, 1),
        battery_w: stat(series("battery_w", true)),
        battery_pct_used,
        cpu_pkg_w: stat(series("cpu_pkg_w", false)),
        cpu_avg_mhz: stat(series("cpu_avg_mhz", false)),
        stall_cpu_pct: stat(series("stall_cpu_pct", false)),
        stall_mem_pct: stat(series("stall_mem_pct", false)),
        stall_io_pct: stat(series("stall_io_pct", false)),
        max_temp_c: stat(series("max_temp_c", false)),
        brightness_pct: stat(series("brightness_pct", false)),
        self_cpu_pct: stat(series("self_cpu_pct", false)),
        self_rss_mb: stat(series("self_rss_mb", false)),
        gauge: gauge::analyse(&gauge_series),
    })
}

/// Hours a full battery of `capacity_wh` would last at the mean battery draw.
pub fn hours_at_mean(report: &Report, capacity_wh: f64) -> Option<f64> {
    let w = report.battery_w.as_ref()?.mean;
    (w > 0.05).then(|| round(capacity_wh / w, 1))
}

fn stat(mut v: Vec<f64>) -> Option<Stat> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    let median = if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    };
    Some(Stat {
        mean: round(v.iter().sum::<f64>() / n as f64, 3),
        median: round(median, 3),
        min: round(v[0], 3),
        max: round(v[n - 1], 3),
        samples: n,
    })
}

fn round(v: f64, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (v * m).round() / m
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# dynamis log format 2
time,interval_s,on_ac,battery_pct,battery_w,cpu_pkg_w,cpu_avg_mhz,turbo,epp,stall_cpu_pct,stall_mem_pct,stall_io_pct,max_temp_c,brightness_pct,self_cpu_pct,self_rss_mb
100,5,false,80,6,1.5,1100,true,balance_power,1,0,0,50,50,0.1,3.2
105,5,false,80,6,1.7,1200,true,balance_power,2,0,0,51,50,0.1,3.2
110,5,false,79,4,1.1,900,true,balance_power,0,0,0,49,50,0.1,3.2
115,5,true,79,,2.0,1500,true,balance_power,0,0,0,55,80,0.1,3.3
";

    #[test]
    fn summarises_log() {
        let r = from_csv(LOG).unwrap();
        assert_eq!(r.log_format, Some(2));
        assert_eq!(r.rows, 4);
        assert_eq!(r.on_battery_share_pct, 75.0);
        let b = r.battery_w.as_ref().unwrap();
        assert_eq!(b.samples, 3, "charger rows are excluded from battery power");
        assert_eq!(b.median, 6.0);
        assert_eq!(r.battery_pct_used, Some(1));
        assert_eq!(r.cpu_pkg_w.as_ref().unwrap().max, 2.0);
        assert_eq!(hours_at_mean(&r, 42.0), Some(7.9));
    }

    #[test]
    fn reads_older_format_without_new_columns() {
        let v1 =
            "# dynamis log format 1\ntime,interval_s,on_ac,battery_w\n1,5,false,5\n2,5,false,5\n";
        let r = from_csv(v1).unwrap();
        assert_eq!(r.log_format, Some(1));
        assert!(r.self_cpu_pct.is_none());
        assert_eq!(r.battery_w.unwrap().mean, 5.0);
    }

    #[test]
    fn skips_a_line_cut_short_by_a_freeze() {
        let torn = format!("{LOG}120,5,fal");
        let r = from_csv(&torn).unwrap();
        assert_eq!(r.rows, 4);
    }

    #[test]
    fn rejects_non_logs() {
        assert!(from_csv("").is_err());
        assert!(from_csv("a,b,c\n1,2,3\n").is_err());
        assert!(from_csv("time,interval_s\n").is_err());
    }
}
