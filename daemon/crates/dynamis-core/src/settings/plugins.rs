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
        &CpuMinPerfPct,
        &CpuMaxFreq,
        &CpuMinFreq,
        &CpuGovernor,
        &CpuHwpDynamicBoost,
        &PlatformProfile,
        &VmSwappiness,
        &VmDirtyWriteback,
        &KernelNmiWatchdog,
        &AudioPowerSave,
        &AudioControllerPower,
        &PcieAspm,
        &StorageIoScheduler,
        &StorageSataAlpm,
        &GpuAmdPowerLevel,
        &BatteryChargeLimit,
        &BatteryChargeStart,
        &SystemMemSleep,
    ]
}

/// The active entry of a "choice [active] choice" kernel file. A file
/// holding one bare word (what a test copy holds after a write) is that word.
fn bracket_active(s: &str) -> Option<String> {
    let (active, all) = parse_bracketed(s);
    active.or_else(|| (all.len() == 1).then(|| all[0].clone()))
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
        bracket_active(&s).ok_or(ReadError::Invalid(s))
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

// --- Added for replacing TLP (batch 1) ---------------------------------------

/// A yes/no kernel value that may be written as `1`/`0` or `Y`/`N`.
fn onoff_file(root: &SysRoot, path: &str) -> ReadResult<String> {
    Ok(match root.read_string(path)?.as_str() {
        "1" | "Y" | "y" => "on",
        _ => "off",
    }
    .into())
}

pub struct CpuMinPerfPct;

impl Setting for CpuMinPerfPct {
    fn id(&self) -> &'static str {
        "cpu.min_perf_pct"
    }
    fn label(&self) -> &'static str {
        "CPU lowest speed"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        // Up to the current speed limit: the driver refuses a floor above it.
        let max = root
            .read_u64(&format!("{PSTATE}/max_perf_pct"))
            .map(|v| v as i64)
            .unwrap_or(100);
        single_file_range(root, &format!("{PSTATE}/min_perf_pct"), 0, max, "%")
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        root.read_string(&format!("{PSTATE}/min_perf_pct"))
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: format!("{PSTATE}/min_perf_pct"),
            contents: value.into(),
        }])
    }
}

/// Highest CPU frequency, for drivers without a speed limit in percent
/// (`acpi-cpufreq` and others). Offered only where the driver lists its
/// frequencies, so the value read back is exactly the value written.
pub struct CpuMaxFreq;

impl CpuMaxFreq {
    fn choices(root: &SysRoot) -> Option<Vec<String>> {
        let p0 = policies(root).into_iter().next()?;
        let list = root
            .read_string(&format!("{p0}/scaling_available_frequencies"))
            .ok()?;
        let mut khz: Vec<u64> = list
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        khz.sort_unstable();
        khz.dedup();
        // Never below 30% of the top speed, as with the speed limit.
        let top = *khz.last()?;
        let values: Vec<String> = khz
            .into_iter()
            .filter(|f| *f * 10 >= top * 3)
            .map(|f| (f / 1000).to_string())
            .collect();
        (values.len() > 1).then_some(values)
    }
}

impl Setting for CpuMaxFreq {
    fn id(&self) -> &'static str {
        "cpu.max_freq"
    }
    fn label(&self) -> &'static str {
        "CPU top speed (MHz)"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        if root.exists(&format!("{PSTATE}/max_perf_pct")) {
            return unsupported("this CPU uses the speed limit instead");
        }
        match Self::choices(root) {
            Some(values) => Probe::Supported {
                allowed: Allowed::Choices { values },
            },
            None => unsupported("this CPU driver lists no frequencies"),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        let v = read_all_policies(root, "scaling_max_freq")?;
        Ok(match v.parse::<u64>() {
            Ok(khz) => (khz / 1000).to_string(),
            Err(_) => v,
        })
    }
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        let mhz: u64 = value
            .parse()
            .map_err(|_| format!("'{value}' is not a number"))?;
        Ok(policies(root)
            .into_iter()
            .map(|p| Write {
                path: format!("{p}/scaling_max_freq"),
                contents: (mhz * 1000).to_string(),
            })
            .collect())
    }
}

/// The lowest speed, for drivers without performance levels (TLP's
/// `CPU_SCALING_MIN_FREQ`). Offered where `cpu.max_freq` is, from the
/// driver's own list, up to the current top speed.
pub struct CpuMinFreq;

impl Setting for CpuMinFreq {
    fn id(&self) -> &'static str {
        "cpu.min_freq"
    }
    fn label(&self) -> &'static str {
        "CPU lowest speed (MHz)"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        if root.exists(&format!("{PSTATE}/max_perf_pct")) {
            return unsupported("this CPU uses performance levels instead");
        }
        let Some(p0) = policies(root).into_iter().next() else {
            return unsupported("no CPU frequency control");
        };
        let Some(list) = root
            .read_string(&format!("{p0}/scaling_available_frequencies"))
            .ok()
        else {
            return unsupported("this CPU driver lists no frequencies");
        };
        let top = root
            .read_u64(&format!("{p0}/scaling_max_freq"))
            .unwrap_or(u64::MAX);
        let mut khz: Vec<u64> = list
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .filter(|f| *f <= top)
            .collect();
        khz.sort_unstable();
        khz.dedup();
        if khz.len() < 2 {
            return unsupported("this CPU driver lists no frequencies");
        }
        Probe::Supported {
            allowed: Allowed::Choices {
                values: khz.into_iter().map(|f| (f / 1000).to_string()).collect(),
            },
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        let v = read_all_policies(root, "scaling_min_freq")?;
        Ok(match v.parse::<u64>() {
            Ok(khz) => (khz / 1000).to_string(),
            Err(_) => v,
        })
    }
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        let mhz: u64 = value
            .parse()
            .map_err(|_| format!("'{value}' is not a number"))?;
        Ok(policies(root)
            .into_iter()
            .map(|p| Write {
                path: format!("{p}/scaling_min_freq"),
                contents: (mhz * 1000).to_string(),
            })
            .collect())
    }
}

pub struct CpuGovernor;

impl Setting for CpuGovernor {
    fn id(&self) -> &'static str {
        "cpu.governor"
    }
    fn label(&self) -> &'static str {
        "CPU governor"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        let Some(p0) = policies(root).into_iter().next() else {
            return unsupported("no CPU frequency control");
        };
        match root.read_string(&format!("{p0}/scaling_available_governors")) {
            Ok(list) => {
                // "userspace" hands control to another program; never offered.
                let values: Vec<String> = list
                    .split_whitespace()
                    .filter(|g| *g != "userspace")
                    .map(String::from)
                    .collect();
                if values.len() < 2 {
                    return unsupported("only one governor here");
                }
                Probe::Supported {
                    allowed: Allowed::Choices { values },
                }
            }
            Err(ReadError::Missing) => unsupported("no governor choice here"),
            Err(e) => unsupported(e.to_string()),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        read_all_policies(root, "scaling_governor")
    }
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(policies(root)
            .into_iter()
            .map(|p| Write {
                path: format!("{p}/scaling_governor"),
                contents: value.into(),
            })
            .collect())
    }
}

pub struct CpuHwpDynamicBoost;

impl Setting for CpuHwpDynamicBoost {
    fn id(&self) -> &'static str {
        "cpu.hwp_dynamic_boost"
    }
    fn label(&self) -> &'static str {
        "Quick speed-up after waiting for the disk"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        match root.read_string(&format!("{PSTATE}/hwp_dynamic_boost")) {
            Ok(_) => Probe::Supported {
                allowed: Allowed::OnOff,
            },
            Err(ReadError::Missing) => unsupported("not offered by this CPU"),
            Err(e) => unsupported(e.to_string()),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        onoff_file(root, &format!("{PSTATE}/hwp_dynamic_boost"))
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: format!("{PSTATE}/hwp_dynamic_boost"),
            contents: if value == "on" { "1" } else { "0" }.into(),
        }])
    }
}

const SND_CONTROLLER: &str = "/sys/module/snd_hda_intel/parameters/power_save_controller";

pub struct AudioControllerPower;

impl Setting for AudioControllerPower {
    fn id(&self) -> &'static str {
        "audio.controller_power"
    }
    fn label(&self) -> &'static str {
        "Sound controller sleep"
    }
    fn risk(&self) -> RiskTier {
        // As with the sound chip: a pop when sound starts, on some hardware.
        RiskTier::UsuallySafe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        match root.read_string(SND_CONTROLLER) {
            Ok(_) => Probe::Supported {
                allowed: Allowed::OnOff,
            },
            Err(ReadError::Missing) => unsupported("not offered by this sound driver"),
            Err(e) => unsupported(e.to_string()),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        onoff_file(root, SND_CONTROLLER)
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: SND_CONTROLLER.into(),
            contents: if value == "on" { "Y" } else { "N" }.into(),
        }])
    }
}

/// The disk I/O scheduler, for every real disk at once.
pub struct StorageIoScheduler;

impl StorageIoScheduler {
    fn disks(root: &SysRoot) -> Vec<String> {
        root.list_dir("/sys/block")
            .into_iter()
            .filter(|d| {
                !["loop", "ram", "zram", "dm-", "md", "sr"]
                    .iter()
                    .any(|p| d.starts_with(p))
            })
            .map(|d| format!("/sys/block/{d}/queue/scheduler"))
            .filter(|p| root.exists(p))
            .collect()
    }
}

impl Setting for StorageIoScheduler {
    fn id(&self) -> &'static str {
        "storage.io_scheduler"
    }
    fn label(&self) -> &'static str {
        "Disk scheduler"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        let disks = Self::disks(root);
        let mut common: Option<Vec<String>> = None;
        for d in &disks {
            let Ok(text) = root.read_string(d) else {
                continue;
            };
            let all = parse_bracketed(&text).1;
            common = Some(match common {
                None => all,
                Some(c) => c.into_iter().filter(|v| all.contains(v)).collect(),
            });
        }
        match common {
            Some(values) if values.len() > 1 => Probe::Supported {
                allowed: Allowed::Choices { values },
            },
            _ => unsupported("no disk with a choice of scheduler"),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        let mut vals = Vec::new();
        for d in Self::disks(root) {
            let s = root.read_string(&d)?;
            vals.push(bracket_active(&s).ok_or(ReadError::Invalid(s))?);
        }
        match vals.first() {
            None => Err(ReadError::Missing),
            Some(f) if vals.iter().all(|v| v == f) => Ok(f.clone()),
            Some(_) => Ok("mixed".into()),
        }
    }
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(Self::disks(root)
            .into_iter()
            .map(|path| Write {
                path,
                contents: value.into(),
            })
            .collect())
    }
}

/// SATA link power management (TLP's `SATA_LINKPWR_ON_BAT`). Every SATA
/// host that has the control. `min_power` is not offered: some drives lose
/// data or hang with it.
pub struct StorageSataAlpm;

const SATA_POLICIES: &[&str] = &["max_performance", "medium_power", "med_power_with_dipm"];

impl StorageSataAlpm {
    fn hosts(root: &SysRoot) -> Vec<String> {
        let mut hosts: Vec<String> = root
            .list_dir("/sys/class/scsi_host")
            .into_iter()
            .map(|h| format!("/sys/class/scsi_host/{h}/link_power_management_policy"))
            .filter(|p| root.exists(p))
            .collect();
        hosts.sort();
        hosts
    }
}

impl Setting for StorageSataAlpm {
    fn id(&self) -> &'static str {
        "storage.sata_alpm"
    }
    fn label(&self) -> &'static str {
        "SATA link power saving"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::UsuallySafe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        if Self::hosts(root).is_empty() {
            return unsupported("no SATA controller with link power control");
        }
        Probe::Supported {
            allowed: Allowed::Choices {
                values: SATA_POLICIES.iter().map(|v| v.to_string()).collect(),
            },
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        let mut vals = Vec::new();
        for h in Self::hosts(root) {
            vals.push(root.read_string(&h)?.trim().to_string());
        }
        match vals.first() {
            None => Err(ReadError::Missing),
            Some(f) if vals.iter().all(|v| v == f) => Ok(f.clone()),
            Some(_) => Ok("mixed".into()),
        }
    }
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        if !SATA_POLICIES.contains(&value) {
            return Err(format!("'{value}' is not offered"));
        }
        Ok(Self::hosts(root)
            .into_iter()
            .map(|path| Write {
                path,
                contents: value.into(),
            })
            .collect())
    }
}

/// AMD graphics power level (`auto`, `low`, `high`).
pub struct GpuAmdPowerLevel;

impl GpuAmdPowerLevel {
    fn paths(root: &SysRoot) -> Vec<String> {
        root.list_dir("/sys/class/drm")
            .into_iter()
            .filter(|c| c.starts_with("card") && !c.contains('-'))
            .map(|c| format!("/sys/class/drm/{c}/device/power_dpm_force_performance_level"))
            .filter(|p| root.exists(p))
            .collect()
    }
}

impl Setting for GpuAmdPowerLevel {
    fn id(&self) -> &'static str {
        "gpu.amd_power_level"
    }
    fn label(&self) -> &'static str {
        "AMD graphics power level"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::UsuallySafe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        if Self::paths(root).is_empty() {
            return unsupported("no AMD graphics here");
        }
        // The manual and profiling levels need further files; not offered.
        Probe::Supported {
            allowed: Allowed::Choices {
                values: vec!["auto".into(), "low".into(), "high".into()],
            },
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        let mut vals = Vec::new();
        for p in Self::paths(root) {
            vals.push(root.read_string(&p)?);
        }
        match vals.first() {
            None => Err(ReadError::Missing),
            Some(f) if vals.iter().all(|v| v == f) => Ok(f.clone()),
            Some(_) => Ok("mixed".into()),
        }
    }
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(Self::paths(root)
            .into_iter()
            .map(|path| Write {
                path,
                contents: value.into(),
            })
            .collect())
    }
}

/// Start charging only below this level (the other half of the charge limit).
pub struct BatteryChargeStart;

impl BatteryChargeStart {
    fn path(root: &SysRoot) -> Option<String> {
        root.list_dir("/sys/class/power_supply")
            .into_iter()
            .filter(|n| n.starts_with("BAT"))
            .map(|n| format!("/sys/class/power_supply/{n}/charge_control_start_threshold"))
            .find(|p| root.exists(p))
    }
}

impl Setting for BatteryChargeStart {
    fn id(&self) -> &'static str {
        "battery.charge_start"
    }
    fn label(&self) -> &'static str {
        "Start charging below"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Safe
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        match Self::path(root) {
            // At least 40%, so a plugged-in laptop never sits nearly empty.
            Some(_) => Probe::Supported {
                allowed: Allowed::Range {
                    min: 40,
                    max: 95,
                    unit: "%",
                },
            },
            None => unsupported("this battery has no charge start control"),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        root.read_string(&Self::path(root).ok_or(ReadError::Missing)?)
    }
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        let path = Self::path(root).ok_or("no charge start control")?;
        // The battery refuses a start at or above the stop level.
        if let Some(stop) = BatteryChargeLimit::path(root).and_then(|p| root.read_u64(&p).ok()) {
            let start: u64 = value
                .parse()
                .map_err(|_| format!("'{value}' is not a number"))?;
            if start >= stop {
                return Err(format!(
                    "start ({start}%) must be below the charge limit ({stop}%)"
                ));
            }
        }
        Ok(vec![Write {
            path,
            contents: value.into(),
        }])
    }
}

const MEM_SLEEP: &str = "/sys/power/mem_sleep";

/// Which kind of sleep the laptop uses. Some firmware wakes badly from one
/// kind, so this is offered only where both exist, and only after a yes.
pub struct SystemMemSleep;

impl Setting for SystemMemSleep {
    fn id(&self) -> &'static str {
        "system.mem_sleep"
    }
    fn label(&self) -> &'static str {
        "Sleep type"
    }
    fn risk(&self) -> RiskTier {
        RiskTier::Risky
    }
    fn probe(&self, root: &SysRoot) -> Probe {
        match root.read_string(MEM_SLEEP) {
            Ok(text) => {
                let all = parse_bracketed(&text).1;
                let values: Vec<String> = ["s2idle", "deep"]
                    .iter()
                    .filter(|v| all.iter().any(|a| a == *v))
                    .map(|v| v.to_string())
                    .collect();
                if values.len() < 2 {
                    return unsupported("only one sleep type here");
                }
                Probe::Supported {
                    allowed: Allowed::Choices { values },
                }
            }
            Err(ReadError::Missing) => unsupported("not offered by this kernel"),
            Err(e) => unsupported(e.to_string()),
        }
    }
    fn read(&self, root: &SysRoot) -> ReadResult<String> {
        let s = root.read_string(MEM_SLEEP)?;
        bracket_active(&s).ok_or(ReadError::Invalid(s))
    }
    fn plan(&self, _root: &SysRoot, value: &str) -> Result<Vec<Write>, String> {
        Ok(vec![Write {
            path: MEM_SLEEP.into(),
            contents: value.into(),
        }])
    }
}
