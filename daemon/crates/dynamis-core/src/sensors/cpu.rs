//! CPU frequency policies and turbo state from `/sys/devices/system/cpu`.

use serde::Serialize;

use crate::fs::{ReadError, SysRoot};

const CPUFREQ: &str = "/sys/devices/system/cpu/cpufreq";
const PSTATE: &str = "/sys/devices/system/cpu/intel_pstate";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Policy {
    pub name: String,
    pub driver: Option<String>,
    pub governor: Option<String>,
    /// Energy-performance preference, where the driver supports it.
    pub epp: Option<String>,
    pub epp_choices: Vec<String>,
    pub cur_mhz: Option<u64>,
    pub min_mhz: Option<u64>,
    pub max_mhz: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum Turbo {
    On,
    Off,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CpuState {
    pub policies: Vec<Policy>,
    pub turbo: Turbo,
}

impl CpuState {
    /// Average current frequency across policies, in MHz.
    pub fn avg_mhz(&self) -> Option<u64> {
        let v: Vec<u64> = self.policies.iter().filter_map(|p| p.cur_mhz).collect();
        if v.is_empty() {
            None
        } else {
            Some(v.iter().sum::<u64>() / v.len() as u64)
        }
    }
}

pub fn read(root: &SysRoot) -> CpuState {
    let policies = root
        .list_dir(CPUFREQ)
        .into_iter()
        .filter(|n| n.starts_with("policy"))
        .map(|name| read_policy(root, name))
        .collect();
    CpuState {
        policies,
        turbo: read_turbo(root),
    }
}

fn read_policy(root: &SysRoot, name: String) -> Policy {
    let base = format!("{CPUFREQ}/{name}");
    let s = |leaf: &str| root.read_string(&format!("{base}/{leaf}")).ok();
    let mhz = |leaf: &str| {
        root.read_u64(&format!("{base}/{leaf}"))
            .ok()
            .map(|khz| khz / 1000)
    };
    Policy {
        driver: s("scaling_driver"),
        governor: s("scaling_governor"),
        epp: s("energy_performance_preference"),
        epp_choices: s("energy_performance_available_preferences")
            .map(|v| v.split_whitespace().map(String::from).collect())
            .unwrap_or_default(),
        cur_mhz: mhz("scaling_cur_freq"),
        min_mhz: mhz("scaling_min_freq"),
        max_mhz: mhz("scaling_max_freq"),
        name,
    }
}

fn read_turbo(root: &SysRoot) -> Turbo {
    // intel_pstate: no_turbo = 1 means turbo is OFF.
    match root.read_u64(&format!("{PSTATE}/no_turbo")) {
        Ok(1) => return Turbo::Off,
        Ok(_) => return Turbo::On,
        Err(ReadError::Missing) => {}
        Err(_) => return Turbo::Unsupported,
    }
    // acpi-cpufreq / amd-pstate: boost = 1 means turbo is ON.
    match root.read_u64(&format!("{CPUFREQ}/boost")) {
        Ok(1) => Turbo::On,
        Ok(_) => Turbo::Off,
        Err(_) => Turbo::Unsupported,
    }
}
