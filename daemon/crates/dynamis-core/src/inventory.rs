//! Hardware and software inventory: what this machine is, and which settings
//! it exposes.
//!
//! The inventory never includes serial numbers, MAC addresses, usernames or
//! hostnames. Its hardware and software fields are shaped to match the shared
//! results schema (`schema/v1/results.schema.json`).

use serde::Serialize;

use crate::fs::{parse_bracketed, ReadError, SysRoot};
use crate::sensors::power;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Inventory {
    pub machine: Machine,
    pub cpu: Cpu,
    pub ram_gb: Option<u64>,
    pub storage: Vec<Storage>,
    pub wifi: Vec<Wifi>,
    pub battery_design_wh: Option<f64>,
    pub software: Software,
    pub zram: Option<Zram>,
    pub levers: Vec<Lever>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Machine {
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub firmware_version: Option<String>,
    /// "laptop", "desktop", "handheld" or "other", from the DMI chassis type.
    pub form_factor: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Cpu {
    pub model: Option<String>,
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    pub scaling_driver: Option<String>,
    /// Hardware-managed P-states (Intel Speed Shift).
    pub hwp: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Storage {
    pub device: String,
    /// "emmc", "sd", "nvme", "sata_ssd" or "hdd".
    pub kind: String,
    pub size_gb: u64,
    pub scheduler: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Wifi {
    pub interface: String,
    pub driver: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Software {
    pub kernel: Option<String>,
    pub distro: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Zram {
    pub device: String,
    pub size_gb: f64,
    pub algorithm: Option<String>,
}

/// One tunable setting and whether this machine exposes it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Lever {
    /// Permanent setting ID from `docs/SETTINGS.md`.
    pub id: &'static str,
    /// Plain-language name for the app.
    pub label: &'static str,
    pub available: bool,
    /// Current value, when available and readable.
    pub current: Option<String>,
    /// Why it is unavailable, in plain words.
    pub note: Option<String>,
}

pub fn read(root: &SysRoot) -> Inventory {
    let supplies = power::read(root);
    Inventory {
        machine: machine(root),
        cpu: cpu(root),
        ram_gb: ram_gb(root),
        storage: storage(root),
        wifi: wifi(root),
        battery_design_wh: supplies
            .batteries
            .first()
            .and_then(|b| b.energy_full_design_wh),
        software: Software {
            kernel: root.read_string("/proc/sys/kernel/osrelease").ok(),
            distro: os_release_name(root),
        },
        zram: zram(root),
        levers: levers(root, &supplies),
    }
}

fn machine(root: &SysRoot) -> Machine {
    let dmi = |leaf: &str| {
        root.read_string(&format!("/sys/class/dmi/id/{leaf}"))
            .ok()
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("to be filled by o.e.m."))
    };
    // SMBIOS chassis types: 8–10, 14, 31, 32 laptops; 30 tablet; 3–7, 13, 15, 35, 36 desktops.
    let form_factor = match dmi("chassis_type").and_then(|s| s.parse::<u32>().ok()) {
        Some(8..=10 | 14 | 31 | 32) => "laptop",
        Some(30) => "handheld",
        Some(3..=7 | 13 | 15 | 35 | 36) => "desktop",
        _ => "other",
    };
    Machine {
        vendor: dmi("sys_vendor"),
        model: dmi("product_name"),
        firmware_version: dmi("bios_version"),
        form_factor: form_factor.to_string(),
    }
}

fn cpu(root: &SysRoot) -> Cpu {
    let info = root.read_string("/proc/cpuinfo").unwrap_or_default();
    let field = |key: &str| {
        info.lines()
            .find(|l| l.split(':').next().map(str::trim) == Some(key))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
    };
    let threads = info.lines().filter(|l| l.starts_with("processor")).count() as u32;
    let flags = field("flags").unwrap_or_default();
    Cpu {
        model: field("model name"),
        cores: field("cpu cores").and_then(|v| v.parse().ok()),
        threads: (threads > 0).then_some(threads),
        scaling_driver: root
            .read_string("/sys/devices/system/cpu/cpufreq/policy0/scaling_driver")
            .ok(),
        hwp: flags.split_whitespace().any(|f| f == "hwp"),
    }
}

fn ram_gb(root: &SysRoot) -> Option<u64> {
    let meminfo = root.read_string("/proc/meminfo").ok()?;
    let kb: u64 = meminfo
        .lines()
        .find(|l| l.starts_with("MemTotal:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    // Usable memory is a little below installed; round to the nearest GB.
    Some(((kb as f64) / 1024.0 / 1024.0).round() as u64)
}

fn storage(root: &SysRoot) -> Vec<Storage> {
    root.list_dir("/sys/block")
        .into_iter()
        .filter(|n| {
            !(n.starts_with("loop")
                || n.starts_with("ram")
                || n.starts_with("zram")
                || n.starts_with("dm-")
                || n.contains("boot")
                || n.contains("rpmb"))
        })
        .filter_map(|dev| {
            let base = format!("/sys/block/{dev}");
            let sectors = root.read_u64(&format!("{base}/size")).ok()?;
            // Skip empty slots and tiny helper devices (under 1 GB).
            if sectors * 512 < 1_000_000_000 {
                return None;
            }
            let kind = if dev.starts_with("mmcblk") {
                match root.read_string(&format!("{base}/device/type")).as_deref() {
                    Ok("SD") => "sd",
                    _ => "emmc",
                }
            } else if dev.starts_with("nvme") {
                "nvme"
            } else if root.read_u64(&format!("{base}/queue/rotational")).ok() == Some(1) {
                "hdd"
            } else {
                "sata_ssd"
            };
            let scheduler = root
                .read_string(&format!("{base}/queue/scheduler"))
                .ok()
                .and_then(|s| parse_bracketed(&s).0);
            Some(Storage {
                size_gb: ((sectors as f64) * 512.0 / 1e9).round() as u64,
                kind: kind.to_string(),
                scheduler,
                device: dev,
            })
        })
        .collect()
}

fn wifi(root: &SysRoot) -> Vec<Wifi> {
    root.list_dir("/sys/class/net")
        .into_iter()
        .filter(|iface| root.exists(&format!("/sys/class/net/{iface}/wireless")))
        .map(|iface| Wifi {
            driver: root
                .link_name(&format!("/sys/class/net/{iface}/device/driver"))
                .ok(),
            interface: iface,
        })
        .collect()
}

fn os_release_name(root: &SysRoot) -> Option<String> {
    let text = root
        .read_string("/etc/os-release")
        .or_else(|_| root.read_string("/usr/lib/os-release"))
        .ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
}

fn zram(root: &SysRoot) -> Option<Zram> {
    root.list_dir("/sys/block")
        .into_iter()
        .filter(|n| n.starts_with("zram"))
        .find_map(|dev| {
            let base = format!("/sys/block/{dev}");
            let bytes = root
                .read_u64(&format!("{base}/disksize"))
                .ok()
                .filter(|&b| b > 0)?;
            Some(Zram {
                size_gb: (bytes as f64 / 1024f64.powi(3) * 10.0).round() / 10.0,
                algorithm: root
                    .read_string(&format!("{base}/comp_algorithm"))
                    .ok()
                    .and_then(|s| parse_bracketed(&s).0),
                device: dev,
            })
        })
}

/// Probe each known setting. Read-only: this only checks that the interface
/// exists and reads its current value.
fn levers(root: &SysRoot, supplies: &power::PowerSupplies) -> Vec<Lever> {
    let file =
        |id: &'static str, label: &'static str, path: &str| probe_file(root, id, label, path);
    let first_glob = |dir: &str, prefix: &str| {
        root.list_dir(dir)
            .into_iter()
            .find(|n| n.starts_with(prefix))
            .map(|n| format!("{dir}/{n}"))
    };

    let mut out = vec![
        file(
            "cpu.epp",
            "CPU speed preference",
            "/sys/devices/system/cpu/cpufreq/policy0/energy_performance_preference",
        ),
        file(
            "cpu.governor",
            "CPU speed governor",
            "/sys/devices/system/cpu/cpufreq/policy0/scaling_governor",
        ),
    ];

    // Turbo lives in one of two places depending on the driver.
    let turbo = if root.exists("/sys/devices/system/cpu/intel_pstate/no_turbo") {
        let mut l = file(
            "cpu.turbo",
            "Turbo boost",
            "/sys/devices/system/cpu/intel_pstate/no_turbo",
        );
        l.current = l
            .current
            .map(|v| if v == "1" { "off".into() } else { "on".into() });
        l
    } else {
        let mut l = file(
            "cpu.turbo",
            "Turbo boost",
            "/sys/devices/system/cpu/cpufreq/boost",
        );
        l.current = l
            .current
            .map(|v| if v == "1" { "on".into() } else { "off".into() });
        l
    };
    out.push(turbo);

    out.push(file(
        "cpu.max_perf_pct",
        "CPU speed limit",
        "/sys/devices/system/cpu/intel_pstate/max_perf_pct",
    ));

    out.push(match first_glob("/sys/class/powercap", "intel-rapl:") {
        Some(dir) => file(
            "cpu.rapl_long",
            "CPU power limit",
            &format!("{dir}/constraint_0_power_limit_uw"),
        ),
        None => missing("cpu.rapl_long", "CPU power limit"),
    });

    out.push(file(
        "platform.profile",
        "Firmware power mode",
        "/sys/firmware/acpi/platform_profile",
    ));

    out.push(match first_glob("/sys/class/backlight", "") {
        Some(dir) => file(
            "display.brightness",
            "Screen brightness",
            &format!("{dir}/brightness"),
        ),
        None => missing("display.brightness", "Screen brightness"),
    });

    out.push(file(
        "vm.swappiness",
        "Swap eagerness",
        "/proc/sys/vm/swappiness",
    ));
    out.push(file(
        "vm.dirty_writeback",
        "Disk write batching",
        "/proc/sys/vm/dirty_writeback_centisecs",
    ));
    out.push(file(
        "kernel.nmi_watchdog",
        "Watchdog wakeups",
        "/proc/sys/kernel/nmi_watchdog",
    ));
    out.push(file(
        "pcie.aspm",
        "PCIe link power saving",
        "/sys/module/pcie_aspm/parameters/policy",
    ));
    out.push(file(
        "audio.power_save",
        "Sound chip sleep",
        "/sys/module/snd_hda_intel/parameters/power_save",
    ));

    let bat = supplies
        .batteries
        .first()
        .map(|b| format!("/sys/class/power_supply/{}", b.name));
    match bat {
        Some(b) => {
            out.push(file(
                "battery.charge_limit",
                "Battery charge limit",
                &format!("{b}/charge_control_end_threshold"),
            ));
            out.push(file(
                "battery.charge_behaviour",
                "Run from battery while plugged in",
                &format!("{b}/charge_behaviour"),
            ));
        }
        None => {
            out.push(missing("battery.charge_limit", "Battery charge limit"));
            out.push(missing(
                "battery.charge_behaviour",
                "Run from battery while plugged in",
            ));
        }
    }

    // Bracketed lists (aspm, scheduler, charge_behaviour) show just the active choice.
    for l in &mut out {
        if let Some(v) = &l.current {
            if v.contains('[') {
                l.current = parse_bracketed(v).0;
            }
        }
    }
    out
}

fn probe_file(root: &SysRoot, id: &'static str, label: &'static str, path: &str) -> Lever {
    match root.read_string(path) {
        Ok(v) => Lever {
            id,
            label,
            available: true,
            current: Some(v),
            note: None,
        },
        Err(ReadError::PermissionDenied) => Lever {
            id,
            label,
            available: true,
            current: None,
            note: Some("present, but reading it needs root".into()),
        },
        Err(ReadError::Missing) => missing(id, label),
        Err(e) => Lever {
            id,
            label,
            available: false,
            current: None,
            note: Some(e.to_string()),
        },
    }
}

fn missing(id: &'static str, label: &'static str) -> Lever {
    Lever {
        id,
        label,
        available: false,
        current: None,
        note: Some("not offered by this machine".into()),
    }
}
