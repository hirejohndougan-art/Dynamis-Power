//! CPU and GPU energy counters (RAPL) from `/sys/class/powercap`.
//!
//! Each domain exposes `energy_uj`, a counter in microjoules that wraps at
//! `max_energy_range_uj`. Power is the change in energy between two readings
//! divided by the time between them, so a single reading gives energy, not
//! power; [`power_between`] turns two readings into watts.
//!
//! On recent kernels `energy_uj` is often readable only by root.

use serde::Serialize;

use crate::fs::{ReadError, SysRoot};

const POWERCAP: &str = "/sys/class/powercap";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Domain {
    /// Directory name, such as `intel-rapl:0` or `intel-rapl:0:0`.
    pub id: String,
    /// Domain name, such as `package-0`, `core`, `uncore` or `dram`.
    pub name: String,
    pub energy_uj: Option<u64>,
    pub max_energy_range_uj: Option<u64>,
    /// Set when the counter exists but could not be read.
    pub unreadable: Option<String>,
}

pub fn read(root: &SysRoot) -> Vec<Domain> {
    root.list_dir(POWERCAP)
        .into_iter()
        .filter(|n| n.contains("rapl:"))
        .map(|id| {
            let base = format!("{POWERCAP}/{id}");
            let (energy_uj, unreadable) = match root.read_u64(&format!("{base}/energy_uj")) {
                Ok(v) => (Some(v), None),
                Err(ReadError::Missing) => (None, None),
                Err(e) => (None, Some(e.to_string())),
            };
            Domain {
                name: root
                    .read_string(&format!("{base}/name"))
                    .unwrap_or_default(),
                max_energy_range_uj: root.read_u64(&format!("{base}/max_energy_range_uj")).ok(),
                energy_uj,
                unreadable,
                id,
            }
        })
        .collect()
}

/// Average power in watts for one domain between two readings taken
/// `seconds` apart, handling counter wrap-around.
pub fn power_between(before: &Domain, after: &Domain, seconds: f64) -> Option<f64> {
    if seconds <= 0.0 {
        return None;
    }
    let (b, a) = (before.energy_uj?, after.energy_uj?);
    let delta = if a >= b {
        a - b
    } else {
        // Counter wrapped: distance to the top plus the new value.
        after.max_energy_range_uj?.checked_sub(b)? + a
    };
    Some(delta as f64 / 1e6 / seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(e: u64) -> Domain {
        Domain {
            id: "intel-rapl:0".into(),
            name: "package-0".into(),
            energy_uj: Some(e),
            max_energy_range_uj: Some(1_000_000),
            unreadable: None,
        }
    }

    #[test]
    fn power_from_delta() {
        // 0.5 J over 0.5 s = 1 W
        assert_eq!(power_between(&d(0), &d(500_000), 0.5), Some(1.0));
    }

    #[test]
    fn handles_wraparound() {
        // 900_000 -> wraps at 1_000_000 -> 100_000 means 200_000 µJ in 1 s = 0.2 W
        let w = power_between(&d(900_000), &d(100_000), 1.0).unwrap();
        assert!((w - 0.2).abs() < 1e-9);
    }
}
