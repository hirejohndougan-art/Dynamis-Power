//! A full diagnostic report, like `tlp-stat`: everything useful for a bug
//! report in one plain-text page. Read-only.
//!
//! Built from the machine itself (inventory, battery, every setting, devices,
//! other power tools), from the service's state directory when readable
//! (originals, blocks, the last changes), and from the running service's
//! `Status` when it can be reached (mode, auto mode, learning, kept settings,
//! the last unclean shutdown). Holds no serial numbers, user names, network
//! names or paths in home directories.

use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

use crate::devices;
use crate::engine::{baseline_at, blocked_at, journal_at};
use crate::fs::SysRoot;
use crate::sensors::power;
use crate::settings::{self, Allowed, Probe};
use crate::{inventory, tools};

fn or_unknown(v: Option<impl ToString>) -> String {
    v.map_or("unknown".into(), |v| v.to_string())
}

fn allowed_text(a: &Allowed) -> String {
    match a {
        Allowed::Choices { values } => values.join(" "),
        Allowed::Range { min, max, unit } => format!("{min}–{max}{unit}"),
        Allowed::OnOff => "on off".into(),
    }
}

/// The report. `state` is the service's state directory (read only if this
/// user can); `status` is the service's `Status` JSON, if it answered.
pub fn report(
    root: &SysRoot,
    state: Option<&Path>,
    status: Option<&Value>,
    version: &str,
) -> String {
    let mut o = String::new();
    let inv = inventory::read(root);
    let _ = writeln!(o, "Dynamis Power diagnostic report (version {version})");
    let _ = writeln!(o);

    let _ = writeln!(o, "== System ==");
    let m = &inv.machine;
    let _ = writeln!(
        o,
        "Model:          {} {} ({})",
        or_unknown(m.vendor.as_deref()),
        or_unknown(m.model.as_deref()),
        m.form_factor
    );
    let _ = writeln!(
        o,
        "Firmware:       {}",
        or_unknown(m.firmware_version.as_deref())
    );
    let _ = writeln!(
        o,
        "Operating sys.: {}",
        or_unknown(inv.software.distro.as_deref())
    );
    let _ = writeln!(
        o,
        "Kernel:         {}",
        or_unknown(inv.software.kernel.as_deref())
    );
    let _ = writeln!(
        o,
        "Processor:      {} ({} cores, {} threads)",
        or_unknown(inv.cpu.model.as_deref()),
        or_unknown(inv.cpu.cores),
        or_unknown(inv.cpu.threads)
    );
    let _ = writeln!(
        o,
        "Speed driver:   {}{}",
        or_unknown(inv.cpu.scaling_driver.as_deref()),
        if inv.cpu.hwp {
            " (hardware-managed)"
        } else {
            ""
        }
    );
    let _ = writeln!(o, "Memory:         {} GB", or_unknown(inv.ram_gb));
    for s in &inv.storage {
        let _ = writeln!(
            o,
            "Storage:        {} {} {} GB, scheduler {}",
            s.device,
            s.kind,
            s.size_gb,
            or_unknown(s.scheduler.as_deref())
        );
    }
    for w in &inv.wifi {
        let _ = writeln!(
            o,
            "Wi-Fi:          driver {}",
            or_unknown(w.driver.as_deref())
        );
    }
    let _ = writeln!(o);

    let _ = writeln!(o, "== Service ==");
    match status {
        Some(st) => {
            let s = |k: &str| st[k].as_str().unwrap_or("").to_string();
            let _ = writeln!(o, "Running:        yes, version {}", s("version"));
            let _ = writeln!(o, "Mode:           {}", s("profile"));
            let _ = writeln!(
                o,
                "Auto mode:      {}{}",
                if st["auto"]["on"] == true {
                    "on"
                } else {
                    "off"
                },
                st["auto"]["reason"]
                    .as_str()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default()
            );
            let _ = writeln!(
                o,
                "Learning:       {}: {}",
                if st["learning"]["on"] == true {
                    "on"
                } else {
                    "off"
                },
                st["learning"]["status"].as_str().unwrap_or("")
            );
            if let Some(p) = st["learning"]["progress"]["pct"].as_f64() {
                let _ = writeln!(o, "Learned:        {p:.0}% of the tests this laptop needs");
            }
            let _ = writeln!(
                o,
                "Safe mode:      {}",
                st["safe_mode"].as_str().unwrap_or("off")
            );
            let _ = writeln!(
                o,
                "Dry run:        {}",
                if st["dry_run"] == true { "on" } else { "off" }
            );
            if let Some(k) = st["kept"]["settings"].as_array().filter(|k| !k.is_empty()) {
                let list: Vec<String> = k
                    .iter()
                    .map(|x| {
                        format!(
                            "{} = {}",
                            x["id"].as_str().unwrap_or(""),
                            x["value"].as_str().unwrap_or("")
                        )
                    })
                    .collect();
                let _ = writeln!(o, "Kept by hand:   {}", list.join(", "));
            }
            if st["kept"]["charge_limit_ignored"] == true {
                let _ = writeln!(o, "Charge limit:   the firmware ignores it");
            }
            if st["full_charge"]["active"] == true {
                let _ = writeln!(o, "Full charge:    charging fully once");
            }
            let u = &st["unclean"];
            if let Some(last) = u["last"].as_object() {
                let _ = writeln!(
                    o,
                    "Last unclean:   kernel log shows {}{}; changed in the 10 min before: {}",
                    last.get("kernel").and_then(Value::as_str).unwrap_or("?"),
                    last.get("detail")
                        .and_then(Value::as_str)
                        .map(|d| format!(" ({d})"))
                        .unwrap_or_default(),
                    last.get("recent")
                        .and_then(Value::as_array)
                        .map(|r| r
                            .iter()
                            .filter_map(|x| x["id"].as_str())
                            .collect::<Vec<_>>()
                            .join(", "))
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| "nothing".into())
                );
            }
            if let Some(n) = u["in_safe_mode"].as_u64().filter(|n| *n > 0) {
                let _ = writeln!(o, "Unclean in safe mode: {n}");
            }
        }
        None => {
            let _ = writeln!(
                o,
                "Running:        not reachable (stopped, or not installed)"
            );
        }
    }
    let _ = writeln!(o);

    let _ = writeln!(o, "== Battery ==");
    let ps = power::read(root);
    for c in &ps.chargers {
        let _ = writeln!(
            o,
            "{}: {}",
            c.name,
            if c.online {
                "plugged in"
            } else {
                "not plugged in"
            }
        );
    }
    for b in &ps.batteries {
        let _ = writeln!(
            o,
            "{}: {}, {}%, {} W",
            b.name,
            or_unknown(b.status.as_deref()),
            or_unknown(b.capacity_pct),
            b.power_w.map_or("unknown".into(), |w| format!("{w:.2}"))
        );
        let _ = writeln!(
            o,
            "  energy {} / {} Wh (design {} Wh), health {}, cycles {}",
            b.energy_now_wh.map_or("?".into(), |v| format!("{v:.1}")),
            b.energy_full_wh.map_or("?".into(), |v| format!("{v:.1}")),
            b.energy_full_design_wh
                .map_or("?".into(), |v| format!("{v:.1}")),
            b.health_pct.map_or("?".into(), |v| format!("{v:.0}%")),
            or_unknown(b.cycle_count)
        );
        let _ = writeln!(
            o,
            "  charge limit {}, run from battery while plugged in: {}",
            b.charge_limit_pct
                .map_or("not supported".into(), |v| format!("{v}%")),
            if b.charge_behaviour_supported {
                "supported"
            } else {
                "not supported"
            }
        );
    }
    if ps.batteries.is_empty() {
        let _ = writeln!(o, "No battery.");
    }
    if let Some(ac) = power::source_override() {
        let _ = writeln!(
            o,
            "Power source:   always treated as {} (set by the person; the charger's report is ignored)",
            if ac { "plugged in" } else { "on battery" }
        );
    }
    let _ = writeln!(o);

    let (baseline, blocked) = state
        .map(|d| (baseline_at(d), blocked_at(d)))
        .unwrap_or_default();
    let _ = writeln!(o, "== Settings ==");
    let _ = writeln!(
        o,
        "{:<24} {:<22} {:<16} notes",
        "setting", "now", "original"
    );
    for s in settings::all() {
        let id = s.id();
        let now = s.read(root).unwrap_or_else(|_| "-".into());
        let original = baseline.get(id).map_or("", String::as_str);
        let mut notes: Vec<String> = Vec::new();
        match s.probe(root) {
            Probe::Supported { allowed } => {
                notes.push(format!("allowed: {}", allowed_text(&allowed)))
            }
            Probe::Unsupported { reason } => notes.push(format!("not here: {reason}")),
        }
        if let Some(why) = blocked.get(id) {
            notes.push(format!("BLOCKED: {why}"));
        }
        if s.risk().needs_opt_in() {
            notes.push("needs a yes".into());
        }
        let _ = writeln!(o, "{id:<24} {now:<22} {original:<16} {}", notes.join("; "));
    }
    for id in ["usb.autosuspend", "pci.runtime_pm"] {
        if let Some(why) = blocked.get(id) {
            let _ = writeln!(o, "{id:<24} BLOCKED: {why}");
        }
    }
    let _ = writeln!(o);

    let _ = writeln!(o, "== Devices (power/control) ==");
    for d in devices::scan(root) {
        let _ = writeln!(
            o,
            "{:<22} {:<5} {:<15} {}{}",
            d.id,
            d.control,
            d.what,
            d.name.as_deref().unwrap_or(""),
            d.excluded
                .map(|x| format!(" [kept awake: {x}]"))
                .unwrap_or_default()
        );
    }
    let _ = writeln!(o);

    let _ = writeln!(o, "== Other power tools ==");
    let running = tools::running(root);
    let at_boot = tools::enabled_at_boot(root);
    let _ = writeln!(
        o,
        "Running:        {}",
        if running.is_empty() {
            "none found".into()
        } else {
            running.join(", ")
        }
    );
    let _ = writeln!(
        o,
        "Start at boot:  {}",
        if at_boot.is_empty() {
            "none found".into()
        } else {
            at_boot.join(", ")
        }
    );
    let _ = writeln!(o);

    let _ = writeln!(o, "== Last changes ==");
    match state {
        Some(dir) if dir.join("journal.jsonl").exists() => {
            let j = journal_at(dir, 20);
            if j.is_empty() {
                let _ = writeln!(o, "None.");
            }
            for e in j {
                let _ = writeln!(
                    o,
                    "{} {:<14} {:<22} {} -> {} {}",
                    e.time,
                    e.event,
                    e.id,
                    e.from.as_deref().unwrap_or(""),
                    e.to.as_deref().unwrap_or(""),
                    e.reason
                );
            }
        }
        Some(_) => {
            let _ = writeln!(o, "No journal yet, or it cannot be read (run with sudo).");
        }
        _ => {
            let _ = writeln!(o, "The service's state cannot be read (run with sudo).");
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_fixture_without_a_service() {
        let root = SysRoot::at(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../tests/fixtures/sysfs/synthetic-laptop"),
        );
        let r = report(&root, None, None, "test");
        for part in [
            "== System ==",
            "Running:        not reachable",
            "BAT0:",
            "cpu.epp",
            "vm.swappiness",
            "== Other power tools ==",
            "cannot be read",
        ] {
            assert!(r.contains(part), "missing {part}:\n{r}");
        }
    }
}
