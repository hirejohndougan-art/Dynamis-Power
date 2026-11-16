//! Battery and charger readings from `/sys/class/power_supply`.
//!
//! Kernel units: power in microwatts (µW), energy in microwatt-hours (µWh),
//! current in microamps (µA), voltage in microvolts (µV), charge in
//! microamp-hours (µAh). Values are converted to W, Wh, V and % here.

use serde::Serialize;

use crate::fs::SysRoot;

const PS: &str = "/sys/class/power_supply";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Battery {
    pub name: String,
    /// "Charging", "Discharging", "Full", "Not charging" or "Unknown".
    pub status: Option<String>,
    /// Charge level reported by the battery, in percent.
    pub capacity_pct: Option<u8>,
    /// Power flowing in or out right now, in watts (always positive).
    pub power_w: Option<f64>,
    pub energy_now_wh: Option<f64>,
    pub energy_full_wh: Option<f64>,
    pub energy_full_design_wh: Option<f64>,
    /// Remaining capacity compared with when new, in percent.
    pub health_pct: Option<f64>,
    pub cycle_count: Option<u64>,
    /// Upper charge limit, where the firmware supports one.
    pub charge_limit_pct: Option<u8>,
    /// Whether the firmware exposes `charge_behaviour` (needed to run from
    /// battery while plugged in, for measurement).
    pub charge_behaviour_supported: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Charger {
    pub name: String,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct PowerSupplies {
    pub batteries: Vec<Battery>,
    pub chargers: Vec<Charger>,
}

/// The person's choice to ignore a charger that reports wrongly (TLP's
/// `TLP_PS_IGNORE`): 0 = believe it, 1 = always plugged in, 2 = always on
/// battery. Process-wide: the service sets it from its preferences.
static SOURCE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Set the power source override (`None` = believe the charger).
pub fn set_source_override(on_ac: Option<bool>) {
    let v = match on_ac {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    };
    SOURCE.store(v, std::sync::atomic::Ordering::Relaxed);
}

/// The power source override, if the person set one.
pub fn source_override() -> Option<bool> {
    match SOURCE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

impl PowerSupplies {
    /// True if any charger reports it is connected, unless the person chose
    /// to ignore what the charger reports ([`set_source_override`]).
    pub fn on_ac(&self) -> bool {
        match SOURCE.load(std::sync::atomic::Ordering::Relaxed) {
            1 => true,
            2 => false,
            _ => self.chargers.iter().any(|c| c.online),
        }
    }

    /// Total battery power across all batteries, in watts.
    pub fn total_battery_power_w(&self) -> Option<f64> {
        let vals: Vec<f64> = self.batteries.iter().filter_map(|b| b.power_w).collect();
        if vals.is_empty() {
            None
        } else {
            Some(vals.iter().sum())
        }
    }
}

pub fn read(root: &SysRoot) -> PowerSupplies {
    let mut out = PowerSupplies::default();
    for name in root.list_dir(PS) {
        let base = format!("{PS}/{name}");
        let kind = root
            .read_string(&format!("{base}/type"))
            .unwrap_or_default();
        match kind.as_str() {
            "Battery" => {
                // Peripheral batteries (mice, headsets) report scope "Device".
                let scope = root
                    .read_string(&format!("{base}/scope"))
                    .unwrap_or_default();
                if scope == "Device" {
                    continue;
                }
                out.batteries.push(read_battery(root, &name, &base));
            }
            "Mains" | "USB" => {
                let online = root
                    .read_u64(&format!("{base}/online"))
                    .map(|v| v == 1)
                    .unwrap_or(false);
                out.chargers.push(Charger { name, online });
            }
            _ => {}
        }
    }
    out
}

fn read_battery(root: &SysRoot, name: &str, base: &str) -> Battery {
    let f = |leaf: &str| format!("{base}/{leaf}");
    let micro = |leaf: &str| root.read_i64(&f(leaf)).ok().map(|v| v as f64 / 1e6);

    let voltage_v = micro("voltage_now");
    let design_voltage_v = micro("voltage_min_design").or(voltage_v);

    // Power: prefer power_now; otherwise current × voltage.
    let power_w = micro("power_now")
        .or_else(|| Some(micro("current_now")? * voltage_v?))
        .map(f64::abs);

    // Energy: prefer energy_*; otherwise charge_* × design voltage.
    let energy = |e: &str, c: &str| micro(e).or_else(|| Some(micro(c)? * design_voltage_v?));
    let energy_now_wh = energy("energy_now", "charge_now");
    let energy_full_wh = energy("energy_full", "charge_full");
    let energy_full_design_wh = energy("energy_full_design", "charge_full_design");

    let health_pct = match (energy_full_wh, energy_full_design_wh) {
        (Some(full), Some(design)) if design > 0.0 => Some((full / design * 1000.0).round() / 10.0),
        _ => None,
    };

    Battery {
        name: name.to_string(),
        status: root.read_string(&f("status")).ok(),
        capacity_pct: root.read_u64(&f("capacity")).ok().map(|v| v.min(100) as u8),
        power_w: power_w.map(round3),
        energy_now_wh: energy_now_wh.map(round3),
        energy_full_wh: energy_full_wh.map(round3),
        energy_full_design_wh: energy_full_design_wh.map(round3),
        health_pct,
        cycle_count: root.read_u64(&f("cycle_count")).ok().filter(|&c| c > 0),
        charge_limit_pct: root
            .read_u64(&f("charge_control_end_threshold"))
            .ok()
            .map(|v| v.min(100) as u8),
        charge_behaviour_supported: root.exists(&f("charge_behaviour")),
    }
}

/// A power supply name the kernel could have made (`BAT0`, `CMB1`): no
/// slashes or dots.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 32
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}
