//! Flat log rows built from consecutive snapshots, for CSV and JSON Lines logs.
//!
//! A row covers the interval between two snapshots. Values that need two
//! readings — CPU package power, and stall time over exactly this interval —
//! are computed here. Column names are part of the log format: add new ones
//! at the end and bump [`LOG_FORMAT_VERSION`]; never rename or reorder.

use serde::Serialize;

use crate::sensors::{self, cpu::Turbo, pressure::Psi, thermal, Snapshot};

/// Version of the row layout below.
pub const LOG_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Row {
    /// End of the interval, seconds since the Unix epoch.
    pub time: f64,
    /// Length of the interval in seconds.
    pub interval_s: f64,
    pub on_ac: bool,
    pub battery_pct: Option<u8>,
    /// Whole-system power from the battery gauge (meaningful on battery).
    pub battery_w: Option<f64>,
    /// CPU package power from RAPL over the interval.
    pub cpu_pkg_w: Option<f64>,
    pub cpu_avg_mhz: Option<u64>,
    pub turbo: Option<bool>,
    pub epp: Option<String>,
    /// Percent of the interval in which some tasks waited for CPU.
    pub stall_cpu_pct: Option<f64>,
    /// Percent of the interval in which some tasks waited for memory.
    pub stall_mem_pct: Option<f64>,
    /// Percent of the interval in which some tasks waited for disk I/O.
    pub stall_io_pct: Option<f64>,
    pub max_temp_c: Option<f64>,
    pub brightness_pct: Option<f64>,
}

impl Row {
    pub const CSV_HEADER: &'static str = "time,interval_s,on_ac,battery_pct,battery_w,cpu_pkg_w,cpu_avg_mhz,turbo,epp,stall_cpu_pct,stall_mem_pct,stall_io_pct,max_temp_c,brightness_pct";

    pub fn between(before: &Snapshot, after: &Snapshot) -> Row {
        let interval = (after.time - before.time).max(0.0);
        let bat = after.power.batteries.first();
        Row {
            time: round(after.time, 3),
            interval_s: round(interval, 3),
            on_ac: after.power.on_ac(),
            battery_pct: bat.and_then(|b| b.capacity_pct),
            battery_w: after.power.total_battery_power_w(),
            cpu_pkg_w: sensors::package_power_w(before, after).map(|w| round(w, 3)),
            cpu_avg_mhz: after.cpu.avg_mhz(),
            turbo: match after.cpu.turbo {
                Turbo::On => Some(true),
                Turbo::Off => Some(false),
                Turbo::Unsupported => None,
            },
            epp: after.cpu.policies.first().and_then(|p| p.epp.clone()),
            stall_cpu_pct: stall_pct(before.pressure.cpu, after.pressure.cpu, interval),
            stall_mem_pct: stall_pct(before.pressure.memory, after.pressure.memory, interval),
            stall_io_pct: stall_pct(before.pressure.io, after.pressure.io, interval),
            max_temp_c: thermal::max_c(&after.thermal),
            brightness_pct: after.backlight.as_ref().map(|b| b.percent),
        }
    }

    /// Number of fields in [`Row::to_csv`]; must equal the header's.
    pub fn csv_field_count(&self) -> usize {
        self.to_csv().split(',').count()
    }

    pub fn to_csv(&self) -> String {
        fn o<T: ToString>(v: &Option<T>) -> String {
            v.as_ref().map(|x| x.to_string()).unwrap_or_default()
        }
        [
            self.time.to_string(),
            self.interval_s.to_string(),
            self.on_ac.to_string(),
            o(&self.battery_pct),
            o(&self.battery_w),
            o(&self.cpu_pkg_w),
            o(&self.cpu_avg_mhz),
            o(&self.turbo),
            o(&self.epp),
            o(&self.stall_cpu_pct),
            o(&self.stall_mem_pct),
            o(&self.stall_io_pct),
            o(&self.max_temp_c),
            o(&self.brightness_pct),
        ]
        .join(",")
    }
}

/// Stall share over exactly this interval, from the change in total stall time.
fn stall_pct(before: Option<Psi>, after: Option<Psi>, interval_s: f64) -> Option<f64> {
    let (b, a) = (before?, after?);
    if interval_s <= 0.0 || a.some.total_us < b.some.total_us {
        return None;
    }
    let stalled_s = (a.some.total_us - b.some.total_us) as f64 / 1e6;
    Some(round((stalled_s / interval_s * 100.0).min(100.0), 2))
}

fn round(v: f64, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (v * m).round() / m
}
