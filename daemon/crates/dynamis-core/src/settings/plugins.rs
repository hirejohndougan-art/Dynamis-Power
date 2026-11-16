//! The first setting plugins. Each one maps between the plain value people see
//! (`on`, `balance_power`, `60`) and the kernel files that hold it.
//!
//! Adding a plugin: implement [`Setting`], add it to [`all`], document it in
//! `docs/SETTINGS.md`, and add fixture files and tests.

use crate::fs::{parse_bracketed, ReadError, ReadResult, SysRoot};

use super::{Allowed, Probe, RiskTier, Setting, Write};

const CPUFREQ: &str = "/sys/devices/system/cpu/cpufreq";
const PSTATE: &str = "/sys/devices/system/cpu/intel_pstate";

/// Every known setting plugin.
pub fn all() -> &'static [&'static dyn Setting] {
    &[
        &CpuEpp,
        &CpuTurbo,
        &CpuMaxPerfPct,
        &PlatformProfile,
        &VmSwappiness,
        &VmDirtyWriteback,
        &KernelNmiWatchdog,
        &AudioPowerSave,
        &PcieAspm,
        &BatteryChargeLimit,
    ]
}

fn unsupported(reason: impl Into<String>) -> Probe {
    Probe::Unsupported {
        reason: reason.into(),
    }
}

/// A setting stored as one number in one file.
fn single_file_range(root: &SysRoot, path: &str, min: i64, max: i64, unit: &'static str) -> Probe {
    match root.read_string(path) {
        Ok(_) => Probe::Supported {
            allowed: Allowed::Range { min, max, unit },
        },
        Err(ReadError::Missing) => unsupported("not offered by this machine"),
        Err(e) => unsupported(e.to_string()),
    }
}

fn policies(root: &SysRoot) -> Vec<String> {
    root.list_dir(CPUFREQ)
        .into_iter()
        .filter(|n| n.starts_with("policy"))
        .map(|n| format!("{CPUFREQ}/{n}"))
        .collect()
}

/// Read the same leaf from every CPU policy; `mixed` if they differ.
fn read_all_policies(root: &SysRoot, leaf: &str) -> ReadResult<String> {
    let mut vals = Vec::new();
    for p in policies(root) {
        vals.push(root.read_string(&format!("{p}/{leaf}"))?);
    }
    match vals.first() {
        None => Err(ReadError::Missing),
        Some(first) if vals.iter().all(|v| v == first) => Ok(first.clone()),
        Some(_) => Ok("mixed".into()),
    }
}

// --- CPU ------------------------------------------------------------------

pub struct CpuEpp;

impl Setting for CpuEpp {
    fn id(&self) -> &'static str {
        "cpu.epp"
    }
    fn label(&self) -> &'static str {
        "CPU speed preference"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        let Some(p0) = policies(root).into_iter().next() else {
            return unsupported("no CPU frequency control");
        };
        match root.read_string(&format!("{p0}/energy_performance_available_preferences")) {
            Ok(list) => {
                // "default" means "whatever firmware chose" and can't be read back.
                let values: Vec<String> = list
                    .split_whitespace()
                    .filter(|v| *v != "default")
                    .map(String::from)
                    .collect();
                Probe::Supported {
                    allowed: Allowed::Choices { values },
                }
            }
            Err(ReadError::Missing) => unsupported("this CPU driver has no speed preference"),
            Err(e) => unsupported(e.to_string()),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        read_all_policies(root, "energy_performance_preference")
    }
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        let ps = policies(root);
        if ps.is_empty() {
            return Err("no CPU policies".into());
        }
        // The kernel refuses EPP changes while the governor is "performance".
        if read_all_policies(root, "scaling_governor").as_deref() == Ok("performance") {
            return Err(
                "the CPU governor is 'performance', which locks the speed preference".into(),
            );
        }
        Ok(ps
            .into_iter()
            .map(|p| Write {
                path: format!("{p}/energy_performance_preference"),
                contents: value.into(),
            })
            .collect())
    }
}

pub struct CpuTurbo;

impl CpuTurbo {
    /// (path, value that means "on")
    fn location(root: &SysRoot) -> Option<(String, &'static str)> {
        let pstate = format!("{PSTATE}/no_turbo");
        if root.exists(&pstate) {
            return Some((pstate, "0"));
        }
        let boost = format!("{CPUFREQ}/boost");
        root.exists(&boost).then_some((boost, "1"))
    }
}

impl Setting for CpuTurbo {
    fn id(&self) -> &'static str {
        "cpu.turbo"
    }
    fn label(&self) -> &'static str {
        "Turbo boost"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        match Self::location(root) {
            Some(_) => Probe::Supported {
                allowed: Allowed::OnOff,
            },
            None => unsupported("no turbo control"),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        let (path, on) = Self::location(root).ok_or(ReadError::Missing)?;
        Ok(if root.read_string(&path)? == on {
            "on"
        } else {
            "off"
        }
        .into())
    }
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        let (path, on) = Self::location(root).ok_or("no turbo control")?;
        let off = if on == "1" { "0" } else { "1" };
        Ok(vec![Write {
            path,
            contents: if value == "on" { on } else { off }.into(),
        }])
    }
}

pub struct CpuMaxPerfPct;

impl Setting for CpuMaxPerfPct {
    fn id(&self) -> &'static str {
        "cpu.max_perf_pct"
    }
    fn label(&self) -> &'static str {
        "CPU speed limit"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        let min = root
            .read_u64(&format!("{PSTATE}/min_perf_pct"))
            .map(|v| v as i64)
            .unwrap_or(20);
        // Never allow a limit so low the machine becomes unusable.
        single_file_range(
            root,
            &format!("{PSTATE}/max_perf_pct"),
            min.max(30),
            100,
            "%",
        )
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        root.read_string(&format!("{PSTATE}/max_perf_pct"))
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: format!("{PSTATE}/max_perf_pct"),
            contents: value.into(),
        }])
    }
}

pub struct PlatformProfile;

impl Setting for PlatformProfile {
    fn id(&self) -> &'static str {
        "platform.profile"
    }
    fn label(&self) -> &'static str {
        "Firmware power mode"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        match root.read_string("/sys/firmware/acpi/platform_profile_choices") {
            Ok(list) => Probe::Supported {
                allowed: Allowed::Choices {
                    values: list.split_whitespace().map(String::from).collect(),
                },
            },
            Err(ReadError::Missing) => unsupported("not offered by this machine"),
            Err(e) => unsupported(e.to_string()),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        root.read_string("/sys/firmware/acpi/platform_profile")
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: "/sys/firmware/acpi/platform_profile".into(),
            contents: value.into(),
        }])
    }
}

// --- Memory and kernel ----------------------------------------------------

pub struct VmSwappiness;

impl Setting for VmSwappiness {
    fn id(&self) -> &'static str {
        "vm.swappiness"
    }
    fn label(&self) -> &'static str {
        "Swap eagerness"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        single_file_range(root, "/proc/sys/vm/swappiness", 0, 200, "")
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        root.read_string("/proc/sys/vm/swappiness")
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: "/proc/sys/vm/swappiness".into(),
            contents: value.into(),
        }])
    }
}

pub struct VmDirtyWriteback;

impl Setting for VmDirtyWriteback {
    fn id(&self) -> &'static str {
        "vm.dirty_writeback"
    }
    fn label(&self) -> &'static str {
        "Disk write batching"
    }
    fn risk(&self) -> RiskTier {
        // Longer batching means more unsaved data at risk in a crash.
        RiskTier::UsuallySafe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        // Centiseconds: 1 to 60 seconds.
        single_file_range(
            root,
            "/proc/sys/vm/dirty_writeback_centisecs",
            100,
            6000,
            " cs",
        )
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        root.read_string("/proc/sys/vm/dirty_writeback_centisecs")
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: "/proc/sys/vm/dirty_writeback_centisecs".into(),
            contents: value.into(),
        }])
    }
}

pub struct KernelNmiWatchdog;

impl Setting for KernelNmiWatchdog {
    fn id(&self) -> &'static str {
        "kernel.nmi_watchdog"
    }
    fn label(&self) -> &'static str {
        "Watchdog wakeups"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        match root.read_string("/proc/sys/kernel/nmi_watchdog") {
            Ok(_) => Probe::Supported {
                allowed: Allowed::OnOff,
            },
            Err(ReadError::Missing) => unsupported("not offered by this kernel"),
            Err(e) => unsupported(e.to_string()),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        Ok(
            if root.read_string("/proc/sys/kernel/nmi_watchdog")? == "0" {
                "off"
            } else {
                "on"
            }
            .into(),
        )
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: "/proc/sys/kernel/nmi_watchdog".into(),
            contents: if value == "on" { "1" } else { "0" }.into(),
        }])
    }
}

// --- Devices --------------------------------------------------------------

pub struct AudioPowerSave;

impl Setting for AudioPowerSave {
    fn id(&self) -> &'static str {
        "audio.power_save"
    }
    fn label(&self) -> &'static str {
        "Sound chip sleep (seconds idle, 0 = never)"
    }
    fn risk(&self) -> RiskTier {
        // Can cause a pop when sound starts on some hardware.
        RiskTier::UsuallySafe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        single_file_range(
            root,
            "/sys/module/snd_hda_intel/parameters/power_save",
            0,
            60,
            " s",
        )
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        root.read_string("/sys/module/snd_hda_intel/parameters/power_save")
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: "/sys/module/snd_hda_intel/parameters/power_save".into(),
            contents: value.into(),
        }])
    }
}

pub struct PcieAspm;

const ASPM: &str = "/sys/module/pcie_aspm/parameters/policy";

impl Setting for PcieAspm {
    fn id(&self) -> &'static str {
        "pcie.aspm"
    }
    fn label(&self) -> &'static str {
        "PCIe link power saving"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::UsuallySafe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        match root.read_string(ASPM) {
            Ok(list) => {
                // "powersupersave" forces the deepest link states and is known
                // to hang some devices; it is deliberately not offered here.
                let values: Vec<String> = parse_bracketed(&list)
                    .1
                    .into_iter()
                    .filter(|v| matches!(v.as_str(), "default" | "performance" | "powersave"))
                    .collect();
                Probe::Supported {
                    allowed: Allowed::Choices { values },
                }
            }
            Err(ReadError::Missing) => unsupported("not offered by this kernel"),
            Err(e) => unsupported(e.to_string()),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        let s = root.read_string(ASPM)?;
        parse_bracketed(&s).0.ok_or(ReadError::Invalid(s))
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: ASPM.into(),
            contents: value.into(),
        }])
    }
}

// --- Battery --------------------------------------------------------------

pub struct BatteryChargeLimit;

impl BatteryChargeLimit {
    fn path(root: &SysRoot) -> Option<String> {
        root.list_dir("/sys/class/power_supply")
            .into_iter()
            .filter(|n| n.starts_with("BAT"))
            .map(|n| format!("/sys/class/power_supply/{n}/charge_control_end_threshold"))
            .find(|p| root.exists(p))
    }
}

impl Setting for BatteryChargeLimit {
    fn id(&self) -> &'static str {
        "battery.charge_limit"
    }
    fn label(&self) -> &'static str {
        "Battery charge limit"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        match Self::path(root) {
            // Below 50% would leave too little charge for normal use.
            Some(_) => Probe::Supported {
                allowed: Allowed::Range {
                    min: 50,
                    max: 100,
                    unit: "%",
                },
            },
            None => unsupported("this battery has no charge limit control"),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        root.read_string(&Self::path(root).ok_or(ReadError::Missing)?)
    }
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        let path = Self::path(root).ok_or("no charge limit control")?;
        Ok(vec![Write {
            path,
            contents: value.into(),
        }])
    }
}
