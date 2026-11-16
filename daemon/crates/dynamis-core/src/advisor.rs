//! Advisor mode (Advanced → Developer): write what Dynamis Power would set
//! as a configuration for another tool, instead of applying it.
//!
//! Useful for people who prefer TLP or TuneD, or who want to see the result
//! as a plain file. The output is text only: Dynamis Power never writes it
//! anywhere itself (persistent configuration is outside what it may touch).
//! The key names are those tools' public configuration formats; no code from
//! either is used.

use std::collections::BTreeMap;

/// Setting → value, for mains (`ac`) and for battery (`bat`).
pub struct Plan {
    pub ac: BTreeMap<String, String>,
    pub bat: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Tlp,
    Tuned,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        match s {
            "tlp" => Some(Format::Tlp),
            "tuned" => Some(Format::Tuned),
            _ => None,
        }
    }
}

pub fn write(plan: &Plan, format: Format, header: &str) -> String {
    match format {
        Format::Tlp => tlp(plan, header),
        Format::Tuned => tuned(plan, header),
    }
}

fn onoff(v: &str) -> &str {
    if v == "on" {
        "1"
    } else {
        "0"
    }
}

fn tlp(plan: &Plan, header: &str) -> String {
    let mut out = format!(
        "# {header}\n# For /etc/tlp.d/50-dynamis-power.conf (TLP 1.6 or later).\n# Settings with no TLP equivalent are listed as comments.\n\n"
    );
    let mut line = |key: &str, ac: Option<String>, bat: Option<String>| {
        if let Some(v) = ac {
            out.push_str(&format!("{key}_ON_AC={v}\n"));
        }
        if let Some(v) = bat {
            out.push_str(&format!("{key}_ON_BAT={v}\n"));
        }
    };
    let get = |m: &BTreeMap<String, String>, id: &str| m.get(id).cloned();
    let both = |id: &str, f: &dyn Fn(&str) -> String| {
        (
            get(&plan.ac, id).map(|v| f(&v)),
            get(&plan.bat, id).map(|v| f(&v)),
        )
    };
    let (a, b) = both("cpu.epp", &|v| v.to_string());
    line("CPU_ENERGY_PERF_POLICY", a, b);
    let (a, b) = both("platform.profile", &|v| v.to_string());
    line("PLATFORM_PROFILE", a, b);
    let (a, b) = both("cpu.turbo", &|v| onoff(v).to_string());
    line("CPU_BOOST", a, b);
    let (a, b) = both("cpu.max_perf_pct", &|v| v.to_string());
    line("CPU_MAX_PERF", a, b);
    let (a, b) = both("audio.power_save", &|v| v.to_string());
    line("SOUND_POWER_SAVE", a, b);
    let (a, b) = both("pcie.aspm", &|v| v.to_string());
    line("PCIE_ASPM", a, b);
    let (a, b) = both("vm.dirty_writeback", &|v| {
        let cs: u64 = v.parse().unwrap_or(500);
        (cs / 100).max(1).to_string()
    });
    line("MAX_LOST_WORK_SECS", a, b);
    // These have one value, not one per power source: battery wins, since
    // that is what learning measured.
    let one = |id: &str| get(&plan.bat, id).or_else(|| get(&plan.ac, id));
    if let Some(v) = one("kernel.nmi_watchdog") {
        out.push_str(&format!("NMI_WATCHDOG={}\n", onoff(&v)));
    }
    if let Some(v) = one("battery.charge_limit") {
        out.push_str(&format!("STOP_CHARGE_THRESH_BAT0={v}\n"));
    }
    if let Some(v) = one("vm.swappiness") {
        out.push_str(&format!(
            "# vm.swappiness={v} (no TLP setting; use /etc/sysctl.d)\n"
        ));
    }
    out
}

fn tuned(plan: &Plan, header: &str) -> String {
    // TuneD has no per-power-source switch inside one profile: this is the
    // battery profile; pair it with your distribution's balanced profile on
    // mains.
    let m = &plan.bat;
    let get = |id: &str| m.get(id).cloned();
    let mut out = format!(
        "# {header}\n# For /etc/tuned/dynamis-battery/tuned.conf (TuneD 2.x). Battery values.\n\n[main]\nsummary=Dynamis Power battery settings\ninclude=balanced\n\n"
    );
    let mut cpu = Vec::new();
    if let Some(v) = get("cpu.epp") {
        cpu.push(format!("energy_performance_preference={v}"));
    }
    if let Some(v) = get("cpu.turbo") {
        cpu.push(format!("boost={}", onoff(&v)));
    }
    if let Some(v) = get("cpu.max_perf_pct") {
        cpu.push(format!("max_perf_pct={v}"));
    }
    if !cpu.is_empty() {
        out.push_str(&format!("[cpu]\n{}\n\n", cpu.join("\n")));
    }
    if let Some(v) = get("audio.power_save") {
        out.push_str(&format!("[audio]\ntimeout={v}\n\n"));
    }
    if let Some(v) = get("platform.profile") {
        out.push_str(&format!("[acpi]\nplatform_profile={v}\n\n"));
    }
    let mut sysctl = Vec::new();
    if let Some(v) = get("vm.dirty_writeback") {
        sysctl.push(format!("vm.dirty_writeback_centisecs={v}"));
    }
    if let Some(v) = get("vm.swappiness") {
        sysctl.push(format!("vm.swappiness={v}"));
    }
    if let Some(v) = get("kernel.nmi_watchdog") {
        sysctl.push(format!("kernel.nmi_watchdog={}", onoff(&v)));
    }
    if !sysctl.is_empty() {
        out.push_str(&format!("[sysctl]\n{}\n\n", sysctl.join("\n")));
    }
    if let Some(v) = get("pcie.aspm") {
        out.push_str(&format!(
            "# PCIe link power saving: {v} (TuneD sets it on the kernel command line; not included)\n"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn tlp_and_tuned() {
        let plan = Plan {
            ac: m(&[("cpu.epp", "balance_performance"), ("cpu.turbo", "on")]),
            bat: m(&[
                ("cpu.epp", "balance_power"),
                ("cpu.turbo", "off"),
                ("cpu.max_perf_pct", "70"),
                ("kernel.nmi_watchdog", "off"),
                ("vm.dirty_writeback", "1500"),
            ]),
        };
        let t = write(&plan, Format::Tlp, "test");
        for want in [
            "CPU_ENERGY_PERF_POLICY_ON_AC=balance_performance",
            "CPU_ENERGY_PERF_POLICY_ON_BAT=balance_power",
            "CPU_BOOST_ON_AC=1",
            "CPU_BOOST_ON_BAT=0",
            "CPU_MAX_PERF_ON_BAT=70",
            "NMI_WATCHDOG=0",
            "MAX_LOST_WORK_SECS_ON_BAT=15",
        ] {
            assert!(t.contains(want), "{want} missing:\n{t}");
        }
        let d = write(&plan, Format::Tuned, "test");
        assert!(d.contains("boost=0"));
        assert!(d.contains("vm.dirty_writeback_centisecs=1500"));
        assert!(d.starts_with("# test"));
    }
}
