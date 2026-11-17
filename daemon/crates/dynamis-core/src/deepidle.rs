//! "Check deep idle": over a quiet minute, how much of the time the
//! processor rested in its deepest idle states, and which devices may be
//! keeping it from resting deeper.
//!
//! Read-only, and only from sysfs (no debugfs, which is often not mounted):
//!
//! - every core's idle states (`cpu*/cpuidle/state*/{name,time}`, time in
//!   microseconds): the share of time the cores spent in their deepest
//!   state. Works on Intel and AMD.
//! - where the firmware reports it (Intel, through the kernel's LPIT
//!   support): `cpuidle/low_power_idle_cpu_residency_us`, time the whole
//!   processor package spent in its deepest state, and
//!   `low_power_idle_system_residency_us`, time the whole platform did.
//! - PCI devices held awake (`power/control` = `on`, `runtime_status` =
//!   `active`), links with link sleep (ASPM L1) off, and links able to use
//!   the deeper link sleep substates (L1.1, L1.2) with them off. The kernel
//!   only shows `link/l1_*` files a link supports, so an existing file at 0
//!   is a capability left unused.
//!
//! A device on these lists is a possible reason, not a proven one: the
//! report says so.

use serde::Serialize;

use crate::devices::{self, Kind};
use crate::fs::SysRoot;

pub const REPORT_VERSION: u32 = 1;
/// How long the check measures.
pub const CHECK_S: f64 = 60.0;
/// Below this share in the deepest core state, the report suggests looking
/// at the devices.
pub const GOOD_PCT: f64 = 80.0;

const CPU_DIR: &str = "/sys/devices/system/cpu";
const LPI_CPU: &str = "/sys/devices/system/cpu/cpuidle/low_power_idle_cpu_residency_us";
const LPI_SYS: &str = "/sys/devices/system/cpu/cpuidle/low_power_idle_system_residency_us";

/// Counters at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub time_s: f64,
    /// Per core: (deepest state's name, its total time in µs).
    cores: Vec<(String, String, u64)>,
    package_us: Option<u64>,
    platform_us: Option<u64>,
}

pub fn snapshot(root: &SysRoot, time_s: f64) -> Snapshot {
    let mut cores = Vec::new();
    let mut cpus = root.list_dir(CPU_DIR);
    cpus.sort();
    for cpu in cpus {
        if !cpu
            .strip_prefix("cpu")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        let dir = format!("{CPU_DIR}/{cpu}/cpuidle");
        // The deepest state is the highest-numbered one that is not disabled.
        let mut states: Vec<(u32, String)> = root
            .list_dir(&dir)
            .into_iter()
            .filter_map(|s| Some((s.strip_prefix("state")?.parse().ok()?, s)))
            .collect();
        states.sort();
        let deepest = states.iter().rev().find(|(_, s)| {
            root.read_string(&format!("{dir}/{s}/disable"))
                .map_or(true, |d| d.trim() == "0")
        });
        let Some((_, state)) = deepest else {
            continue;
        };
        let name = root
            .read_string(&format!("{dir}/{state}/name"))
            .map(|n| n.trim().to_string())
            .unwrap_or_else(|_| state.clone());
        if let Ok(t) = root.read_string(&format!("{dir}/{state}/time")) {
            if let Ok(t) = t.trim().parse() {
                cores.push((cpu, name, t));
            }
        }
    }
    let num = |p: &str| root.read_string(p).ok().and_then(|v| v.trim().parse().ok());
    Snapshot {
        time_s,
        cores,
        package_us: num(LPI_CPU),
        platform_us: num(LPI_SYS),
    }
}

/// A device that may keep the processor from resting deeper.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Blocker {
    /// `pci:0000:00:14.0`
    pub id: String,
    /// Its name from the PCI id list, or its kind (`what`, translated by
    /// the app) when there is none.
    pub name: Option<String>,
    pub what: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub v: u32,
    pub seconds: f64,
    /// The deepest core idle state's name (e.g. `C10`), if all cores agree.
    pub deepest: Option<String>,
    /// Average share of time the cores spent in their deepest state.
    pub core_deepest_pct: Option<f64>,
    /// Share of time the whole processor package rested deepest (Intel).
    pub package_pct: Option<f64>,
    /// Share of time the whole platform rested deepest (Intel).
    pub platform_pct: Option<f64>,
    /// Devices kept awake (not let sleep when idle).
    pub held_awake: Vec<Blocker>,
    /// Devices whose link never sleeps (ASPM L1 off).
    pub no_link_sleep: Vec<Blocker>,
    /// Devices able to use deeper link sleep (L1.1/L1.2) with it off.
    pub no_deeper_link_sleep: Vec<Blocker>,
    /// The laptop was used during the check, so the numbers are lower than
    /// at rest.
    pub touched: bool,
    /// The deepest state allowed is only a light one (C1, C1E or polling),
    /// so "rested deeply" would be misleading.
    #[serde(default)]
    pub shallow: bool,
    /// Why deeper states are not allowed, when it can be seen: the kernel
    /// command line setting that caps them (`intel_idle.max_cstate=1`), or
    /// `disabled` when deeper states exist but are switched off.
    #[serde(default)]
    pub limited_by: Option<String>,
}

/// Light idle states: a core in one of these is barely resting.
fn is_light(name: &str) -> bool {
    let n = name.trim().to_ascii_uppercase();
    n == "POLL" || n == "C1" || n == "C1E" || n.starts_with("C1-") || n.starts_with("C1E-")
}

/// What keeps deeper idle states away: a cap on the kernel command line, or
/// deeper states present but disabled.
fn limit(root: &SysRoot) -> Option<String> {
    if let Ok(cmd) = root.read_string("/proc/cmdline") {
        if let Some(p) = cmd.split_whitespace().find(|p| {
            p.starts_with("intel_idle.max_cstate=")
                || p.starts_with("processor.max_cstate=")
                || p.starts_with("idle=")
        }) {
            return Some(p.to_string());
        }
    }
    let dir = format!("{CPU_DIR}/cpu0/cpuidle");
    let disabled = root.list_dir(&dir).into_iter().any(|s| {
        s.starts_with("state")
            && root
                .read_string(&format!("{dir}/{s}/disable"))
                .is_ok_and(|d| d.trim() != "0")
    });
    disabled.then(|| "disabled".to_string())
}

fn pct(delta: u64, seconds: f64) -> f64 {
    (delta as f64 / (seconds * 1e6) * 100.0).clamp(0.0, 100.0)
}

/// Compare two snapshots and list the devices that may be in the way.
pub fn report(root: &SysRoot, a: &Snapshot, b: &Snapshot, touched: bool) -> Report {
    let seconds = (b.time_s - a.time_s).max(0.001);
    let mut shares = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    for (cpu, name, t) in &b.cores {
        let Some((_, _, before)) = a.cores.iter().find(|(c, n, _)| c == cpu && n == name) else {
            continue;
        };
        shares.push(pct(t.saturating_sub(*before), seconds));
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    let delta = |x: Option<u64>, y: Option<u64>| Some(pct(y?.checked_sub(x?)?, seconds));
    let shallow = !names.is_empty() && names.iter().all(|n| is_light(n));

    let mut list = devices::scan(root);
    list.retain(|d| d.kind == Kind::Pci);
    devices::add_friendly_names(root, &mut list);
    let blocker = |d: &devices::Device| Blocker {
        id: d.id.clone(),
        name: d.name.clone(),
        what: d.what,
    };
    let read = |d: &devices::Device, f: &str| {
        root.read_string(&format!("{}/{f}", d.path))
            .ok()
            .map(|v| v.trim().to_string())
    };
    let mut held_awake = Vec::new();
    let mut no_link_sleep = Vec::new();
    let mut no_deeper_link_sleep = Vec::new();
    for d in &list {
        // Graphics manages its own sleep and is awake whenever the screen
        // is on: listing it would only mislead.
        if d.what != "graphics"
            && d.control == "on"
            && read(d, "power/runtime_status").as_deref() == Some("active")
        {
            held_awake.push(blocker(d));
        }
        if read(d, "link/l1_aspm").as_deref() == Some("0") {
            no_link_sleep.push(blocker(d));
        } else if ["link/l1_1_aspm", "link/l1_2_aspm"]
            .iter()
            .any(|f| read(d, f).as_deref() == Some("0"))
        {
            no_deeper_link_sleep.push(blocker(d));
        }
    }
    Report {
        v: REPORT_VERSION,
        seconds,
        deepest: (names.len() == 1).then(|| names[0].to_string()),
        core_deepest_pct: (!shares.is_empty())
            .then(|| shares.iter().sum::<f64>() / shares.len() as f64),
        package_pct: delta(a.package_us, b.package_us),
        platform_pct: delta(a.platform_us, b.platform_us),
        held_awake,
        no_link_sleep,
        no_deeper_link_sleep,
        touched,
        shallow,
        limited_by: if shallow { limit(root) } else { None },
    }
}

pub mod fixture {
    use std::fs;
    use std::path::Path;

    /// A core with idle states named `names`, the last one deepest, and the
    /// deepest one's total time.
    pub fn core(root: &Path, cpu: u32, names: &[&str], deepest_us: u64) {
        for (i, n) in names.iter().enumerate() {
            let d = root.join(format!("sys/devices/system/cpu/cpu{cpu}/cpuidle/state{i}"));
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("name"), format!("{n}\n")).unwrap();
            fs::write(d.join("disable"), "0\n").unwrap();
            let t = if i + 1 == names.len() { deepest_us } else { 0 };
            fs::write(d.join("time"), format!("{t}\n")).unwrap();
        }
    }

    /// A PCI link's sleep files.
    pub fn link(root: &Path, dev: &str, files: &[(&str, &str)]) {
        let d = root.join(format!("sys/bus/pci/devices/{dev}/link"));
        fs::create_dir_all(&d).unwrap();
        for (f, v) in files {
            fs::write(d.join(f), format!("{v}\n")).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepest_share_and_what_may_be_in_the_way() {
        let dir = std::env::temp_dir().join(format!("dynamis-deepidle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        fixture::core(&dir, 0, &["POLL", "C1", "C10"], 1_000_000);
        fixture::core(&dir, 1, &["POLL", "C1", "C10"], 2_000_000);
        crate::devices::fixture::pci(&dir, "0000:00:14.0", "0x0c0330", "on", "xhci_hcd");
        std::fs::write(
            dir.join("sys/bus/pci/devices/0000:00:14.0/power/runtime_status"),
            "active\n",
        )
        .unwrap();
        crate::devices::fixture::pci(&dir, "0000:01:00.0", "0x010802", "auto", "nvme");
        fixture::link(
            &dir,
            "0000:01:00.0",
            &[("l1_aspm", "1"), ("l1_2_aspm", "0")],
        );
        crate::devices::fixture::pci(&dir, "0000:02:00.0", "0x028000", "auto", "iwlwifi");
        fixture::link(&dir, "0000:02:00.0", &[("l1_aspm", "0")]);
        let lpi = dir.join("sys/devices/system/cpu/cpuidle");
        std::fs::create_dir_all(&lpi).unwrap();
        std::fs::write(lpi.join("low_power_idle_cpu_residency_us"), "100\n").unwrap();
        let root = SysRoot::at(&dir);
        let a = snapshot(&root, 0.0);

        fixture::core(&dir, 0, &["POLL", "C1", "C10"], 1_000_000 + 54_000_000);
        fixture::core(&dir, 1, &["POLL", "C1", "C10"], 2_000_000 + 42_000_000);
        std::fs::write(lpi.join("low_power_idle_cpu_residency_us"), "30000100\n").unwrap();
        let b = snapshot(&root, 60.0);
        let r = report(&root, &a, &b, false);

        assert_eq!(r.deepest.as_deref(), Some("C10"));
        assert!((r.core_deepest_pct.unwrap() - 80.0).abs() < 1e-9, "{r:?}");
        assert!((r.package_pct.unwrap() - 50.0).abs() < 1e-9);
        assert_eq!(r.platform_pct, None, "not reported here");
        assert_eq!(r.held_awake.len(), 1);
        assert_eq!(r.held_awake[0].id, "pci:0000:00:14.0");
        assert_eq!(r.no_link_sleep[0].id, "pci:0000:02:00.0");
        assert_eq!(r.no_deeper_link_sleep[0].id, "pci:0000:01:00.0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_light_rest_allowed_and_why() {
        let dir = std::env::temp_dir().join(format!("dynamis-deepidle3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        fixture::core(&dir, 0, &["POLL", "C1"], 1_000);
        std::fs::create_dir_all(dir.join("proc")).unwrap();
        std::fs::write(
            dir.join("proc/cmdline"),
            "quiet splash intel_idle.max_cstate=1\n",
        )
        .unwrap();
        crate::devices::fixture::pci(&dir, "0000:00:02.0", "0x030000", "on", "i915");
        std::fs::write(
            dir.join("sys/bus/pci/devices/0000:00:02.0/power/runtime_status"),
            "active\n",
        )
        .unwrap();
        let root = SysRoot::at(&dir);
        let a = snapshot(&root, 0.0);
        fixture::core(&dir, 0, &["POLL", "C1"], 47_000_000);
        let r = report(&root, &a, &snapshot(&root, 60.0), false);
        assert!(r.shallow);
        assert_eq!(r.limited_by.as_deref(), Some("intel_idle.max_cstate=1"));
        assert!(r.held_awake.is_empty(), "graphics is not listed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_disabled_deepest_state_is_skipped() {
        let dir = std::env::temp_dir().join(format!("dynamis-deepidle2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        fixture::core(&dir, 0, &["POLL", "C6", "C8"], 5);
        std::fs::write(
            dir.join("sys/devices/system/cpu/cpu0/cpuidle/state2/disable"),
            "1\n",
        )
        .unwrap();
        let s = snapshot(&SysRoot::at(&dir), 0.0);
        assert_eq!(s.cores[0].1, "C6");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
