//! Temperatures from `/sys/class/thermal` (values are in millidegrees Celsius).

use serde::Serialize;

use crate::fs::SysRoot;

const THERMAL: &str = "/sys/class/thermal";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Zone {
    pub name: String,
    /// What the zone measures, such as `x86_pkg_temp` or `acpitz`.
    pub kind: String,
    pub temp_c: f64,
}

pub fn read(root: &SysRoot) -> Vec<Zone> {
    root.list_dir(THERMAL)
        .into_iter()
        .filter(|n| n.starts_with("thermal_zone"))
        .filter_map(|name| {
            let base = format!("{THERMAL}/{name}");
            let milli = root.read_i64(&format!("{base}/temp")).ok()?;
            // Disabled or broken zones sometimes report nonsense; skip them.
            if !(-40_000..=150_000).contains(&milli) {
                return None;
            }
            Some(Zone {
                kind: root
                    .read_string(&format!("{base}/type"))
                    .unwrap_or_default(),
                temp_c: milli as f64 / 1000.0,
                name,
            })
        })
        .collect()
}

/// The hottest valid reading, which is what guardrails care about.
pub fn max_c(zones: &[Zone]) -> Option<f64> {
    zones
        .iter()
        .map(|z| z.temp_c)
        .fold(None, |acc, t| Some(acc.map_or(t, |a: f64| a.max(t))))
}
