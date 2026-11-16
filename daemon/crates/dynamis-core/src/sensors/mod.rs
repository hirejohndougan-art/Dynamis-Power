//! Sensor plugins. Each module reads one kind of signal and never writes.
//!
//! A [`Snapshot`] is everything read at one moment. The logger takes snapshots
//! at an interval; values that need two readings (such as RAPL power) are
//! computed from consecutive snapshots.

pub mod backlight;
pub mod cpu;
pub mod idle;
pub mod power;
pub mod pressure;
pub mod rapl;
pub mod thermal;

use serde::Serialize;

use crate::fs::SysRoot;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Snapshot {
    /// Seconds since the Unix epoch, with fractions.
    pub time: f64,
    pub power: power::PowerSupplies,
    pub cpu: cpu::CpuState,
    pub pressure: pressure::Pressure,
    pub thermal: Vec<thermal::Zone>,
    pub rapl: Vec<rapl::Domain>,
    pub backlight: Option<backlight::Backlight>,
}

/// Read every sensor once.
pub fn snapshot(root: &SysRoot) -> Snapshot {
    Snapshot {
        time: now_secs(),
        power: power::read(root),
        cpu: cpu::read(root),
        pressure: pressure::read(root),
        thermal: thermal::read(root),
        rapl: rapl::read(root),
        backlight: backlight::read(root),
    }
}

pub fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// CPU package power in watts between two snapshots, if RAPL is readable.
pub fn package_power_w(before: &Snapshot, after: &Snapshot) -> Option<f64> {
    let secs = after.time - before.time;
    let a = after.rapl.iter().find(|d| d.name.starts_with("package"))?;
    let b = before.rapl.iter().find(|d| d.id == a.id)?;
    rapl::power_between(b, a, secs)
}
