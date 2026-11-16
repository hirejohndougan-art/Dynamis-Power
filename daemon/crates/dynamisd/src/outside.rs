//! Settings changed behind Dynamis Power's back.
//!
//! Another tool (PowerTOP's auto-tune at boot, a vendor service, a udev
//! rule, a script) can set the same files. Each would undo the other and
//! nobody could tell why. On each reading the service compares what it last
//! set (worked out from its journal) with what the kernel reports. A
//! difference seen on two readings in a row is journaled as `outside`, with
//! the likely tool if one can be found. After the second outside change to a
//! setting in a day, the engine blocks it: Dynamis Power stops managing it
//! and leaves it to the other tool (it never fights).
//!
//! Only settings whose last journal event is Dynamis Power's own change are
//! watched; after a restore, revert or block the kernel's value is not ours.
//! Nothing is checked while a change is pending, nor for 30 seconds after
//! waking (firmware and drivers settle then, and the service re-applies).

use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

use dynamis_core::engine::{journal_at, Entry};
use dynamis_core::settings::{self, Setting};
use dynamis_core::SysRoot;

use crate::core::open_engine;

/// After waking, readings are not compared for this long.
pub const AFTER_WAKE_S: f64 = 30.0;

/// Processes that set power settings: (name in `comm` or the program file
/// name, name for people).
const TOOLS: &[(&str, &str)] = &[
    ("tlp", "TLP"),
    ("auto-cpufreq", "auto-cpufreq"),
    ("asusd", "Asus Linux (asusd)"),
    ("asusctl", "Asus Linux (asusctl)"),
    ("supergfxd", "supergfxctl"),
    ("system76-power", "System76 Power"),
    ("tuned", "TuneD"),
    ("tuned-ppd", "tuned-ppd"),
    ("power-profiles-daemon", "power-profiles-daemon"),
    ("laptop_mode", "Laptop Mode Tools"),
    ("powertop", "PowerTOP"),
    ("slimbookbattery", "Slimbook Battery"),
    ("tccd", "TUXEDO Control Center"),
    ("thermald", "thermald"),
    ("cpupower-gui", "cpupower-gui"),
    ("cpufreqd", "cpufreqd"),
    ("powerd", "powerd"),
];

/// systemd units that run such tools at boot without staying around.
const UNITS: &[(&str, &str)] = &[
    ("powertop.service", "PowerTOP auto-tune"),
    ("tlp.service", "TLP"),
    ("laptop-mode.service", "Laptop Mode Tools"),
    ("auto-cpufreq.service", "auto-cpufreq"),
    ("system76-power.service", "System76 Power"),
    ("tuned.service", "TuneD"),
    ("asusd.service", "Asus Linux (asusd)"),
    ("slimbookbattery.service", "Slimbook Battery"),
    ("tccd.service", "TUXEDO Control Center"),
];

const UDEV_DIRS: &[&str] = &[
    "/etc/udev/rules.d",
    "/usr/lib/udev/rules.d",
    "/lib/udev/rules.d",
];
const UNIT_DIRS: &[&str] = &["/etc/systemd/system"];

#[derive(Default)]
pub struct OutsideState {
    /// What Dynamis Power last set, per setting it currently manages.
    expected: BTreeMap<String, String>,
    /// Size and time of the journal when `expected` was worked out.
    sig: Option<(u64, Option<SystemTime>)>,
    /// A difference seen once, waiting for the next reading.
    seen: BTreeMap<String, String>,
    /// Unix time before which nothing is compared.
    quiet_until: f64,
    /// Side effects of our own changes (for example, choosing a governor
    /// resets the speed preference): the value taken as ours, and how many
    /// journal entries that setting had then (a newer entry ends it).
    adopted: BTreeMap<String, (String, usize)>,
    /// Our own setting changes in the journal at the last check.
    own_changes: usize,
}

const OWN: &[&str] = &["done", "keep", "revert", "restore", "recover", "quirk"];

/// What Dynamis Power last set, from its journal (oldest first).
pub fn expected_from(journal: &[Entry]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for e in journal {
        match e.event.as_str() {
            "done" | "keep" | "outside" => {
                if let Some(to) = &e.to {
                    out.insert(e.id.clone(), to.clone());
                }
            }
            "revert" | "recover" | "restore" | "quirk" | "block" => {
                out.remove(&e.id);
            }
            _ => {}
        }
    }
    out
}

impl OutsideState {
    pub fn quiet_until(&mut self, unix: f64) {
        self.quiet_until = self.quiet_until.max(unix);
        self.seen.clear();
    }

    /// One reading. Returns the settings blocked now.
    pub fn check(&mut self, root: &SysRoot, dir: &Path, unix: f64) -> Vec<String> {
        if unix < self.quiet_until || dir.join("pending.json").exists() {
            self.seen.clear();
            return Vec::new();
        }
        let sig = std::fs::metadata(dir.join("journal.jsonl"))
            .ok()
            .map(|m| (m.len(), m.modified().ok()));
        if sig != self.sig {
            self.sig = sig;
            let journal = journal_at(dir, usize::MAX);
            let entries = |id: &str| journal.iter().filter(|e| e.id == id).count();
            self.expected = expected_from(&journal);
            self.adopted
                .retain(|id, (_, n)| self.expected.contains_key(id) && entries(id) == *n);
            for (id, (v, _)) in &self.adopted {
                self.expected.insert(id.clone(), v.clone());
            }
            self.seen.retain(|id, _| self.expected.contains_key(id));
            // A difference that first shows right after one of our own
            // changes is taken as its side effect, not another tool's.
            let own = journal
                .iter()
                .filter(|e| OWN.contains(&e.event.as_str()))
                .count();
            if own != self.own_changes {
                self.own_changes = own;
                let mut adopt = Vec::new();
                for (id, ours) in &self.expected {
                    let now = settings::by_id(id).and_then(|s| s.read(root).ok());
                    if let Some(now) = now.filter(|n| n != ours) {
                        if self.seen.get(id) != Some(&now) {
                            adopt.push((id.clone(), now));
                        }
                    }
                }
                for (id, now) in adopt {
                    self.adopted.insert(id.clone(), (now.clone(), entries(&id)));
                    self.expected.insert(id, now);
                }
            }
        }
        let mut confirmed = Vec::new();
        for (id, ours) in &self.expected {
            let Some(s) = settings::by_id(id) else {
                continue;
            };
            let Ok(now) = s.read(root) else {
                self.seen.remove(id);
                continue;
            };
            if now == *ours {
                self.seen.remove(id);
            } else if self.seen.get(id) == Some(&now) {
                confirmed.push((s, ours.clone(), now));
            } else {
                self.seen.insert(id.clone(), now);
            }
        }
        if confirmed.is_empty() {
            return Vec::new();
        }
        let mut engine = match open_engine(root, dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("dynamisd: outside change: {e}");
                return Vec::new();
            }
        };
        let mut blocked = Vec::new();
        for (s, ours, now) in confirmed {
            let id = s.id();
            self.seen.remove(id);
            let tool = likely_tool(root, s, &now);
            eprintln!(
                "dynamisd: {id} was changed outside Dynamis Power ({ours} -> {now}){}",
                tool.as_deref()
                    .map(|t| format!(", likely by {t}"))
                    .unwrap_or_default()
            );
            match engine.note_outside(id, &ours, &now, tool.as_deref()) {
                Ok(true) => blocked.push(id.to_string()),
                Ok(false) => {}
                Err(e) => eprintln!("dynamisd: {e}"),
            }
        }
        blocked
    }
}

/// The program, rule or service that most likely changed a setting:
/// a running tool, then a udev rule writing the same file, then a tool's
/// service set to start at boot.
pub fn likely_tool(root: &SysRoot, s: &dyn Setting, value: &str) -> Option<String> {
    running_tool(root)
        .or_else(|| udev_rule(root, s, value))
        .or_else(|| boot_unit(root))
}

fn running_tool(root: &SysRoot) -> Option<String> {
    let me = std::process::id().to_string();
    for pid in root.list_dir("/proc") {
        if !pid.bytes().all(|b| b.is_ascii_digit()) || pid == me {
            continue;
        }
        let comm = root
            .read_string(&format!("/proc/{pid}/comm"))
            .unwrap_or_default();
        let cmdline = std::fs::read(root.path(&format!("/proc/{pid}/cmdline"))).unwrap_or_default();
        let args: Vec<String> = cmdline
            .split(|b| *b == 0)
            .take(2)
            .map(|a| {
                let a = String::from_utf8_lossy(a);
                a.rsplit('/').next().unwrap_or_default().to_string()
            })
            .collect();
        for (name, nice) in TOOLS {
            let matches = |n: &str| n == *name || n.strip_suffix(".py") == Some(name);
            if matches(comm.trim()) || args.iter().any(|a| matches(a)) {
                return Some(nice.to_string());
            }
        }
    }
    None
}

/// A udev rule that writes the same attribute (for example
/// `ATTR{power/control}="on"`).
fn udev_rule(root: &SysRoot, s: &dyn Setting, value: &str) -> Option<String> {
    let writes = s.plan(root, value).ok()?;
    let attrs: Vec<String> = writes
        .iter()
        .flat_map(|w| {
            let parts: Vec<&str> = w.path.rsplit('/').take(2).collect();
            let mut v = Vec::new();
            if let Some(last) = parts.first() {
                v.push(format!("{{{last}}}="));
            }
            if let [last, parent] = parts[..] {
                v.push(format!("{{{parent}/{last}}}="));
            }
            v
        })
        .collect();
    for dir in UDEV_DIRS {
        for f in root.list_dir(dir) {
            if !f.ends_with(".rules") {
                continue;
            }
            let Ok(text) = root.read_string(&format!("{dir}/{f}")) else {
                continue;
            };
            let writes_it = text
                .lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .any(|l| attrs.iter().any(|a| l.contains(a.as_str())));
            if writes_it {
                return Some(format!("a device rule ({f})"));
            }
        }
    }
    None
}

fn boot_unit(root: &SysRoot) -> Option<String> {
    for dir in UNIT_DIRS {
        for wants in root.list_dir(dir) {
            if !wants.ends_with(".wants") {
                continue;
            }
            let present = root.list_dir(&format!("{dir}/{wants}"));
            for (unit, nice) in UNITS {
                if present.iter().any(|p| p == unit) {
                    return Some(nice.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn e(event: &str, id: &str, to: Option<&str>) -> Entry {
        Entry {
            v: 1,
            time: 0,
            event: event.into(),
            id: id.into(),
            from: None,
            to: to.map(String::from),
            reason: String::new(),
        }
    }

    #[test]
    fn works_out_what_it_manages() {
        let j = [
            e("done", "cpu.epp", Some("power")),
            e("done", "vm.swappiness", Some("10")),
            e("start", "cpu.turbo", Some("off")),
            e("would", "cpu.turbo", Some("off")),
            e("restore", "vm.swappiness", Some("60")),
            e("outside", "cpu.epp", Some("performance")),
            e("app-weight", "app:x", Some("50")),
        ];
        let x = expected_from(&j);
        assert_eq!(x.len(), 1);
        assert_eq!(x["cpu.epp"], "performance");
    }

    #[test]
    fn names_the_likely_tool() {
        let dir = std::env::temp_dir().join(format!("dynamis-outside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let root = SysRoot::at(&dir);
        let s = settings::by_id("vm.swappiness").unwrap();
        fs::create_dir_all(dir.join("proc/sys/vm")).unwrap();
        fs::write(dir.join("proc/sys/vm/swappiness"), "60").unwrap();
        assert_eq!(likely_tool(&root, s, "60"), None);

        fs::create_dir_all(dir.join("etc/systemd/system/multi-user.target.wants")).unwrap();
        fs::write(
            dir.join("etc/systemd/system/multi-user.target.wants/powertop.service"),
            "",
        )
        .unwrap();
        assert_eq!(
            likely_tool(&root, s, "60").as_deref(),
            Some("PowerTOP auto-tune")
        );

        let rules = dir.join("etc/udev/rules.d");
        fs::create_dir_all(&rules).unwrap();
        fs::write(rules.join("50-x.rules"), "# ATTR{swappiness}=\"1\"\n").unwrap();
        assert_eq!(
            likely_tool(&root, s, "60").as_deref(),
            Some("PowerTOP auto-tune")
        );
        fs::write(
            rules.join("50-x.rules"),
            "ACTION==\"add\", ATTR{vm/swappiness}=\"1\"\n",
        )
        .unwrap();
        assert_eq!(
            likely_tool(&root, s, "60").as_deref(),
            Some("a device rule (50-x.rules)")
        );

        // A Python program shows as python3; its script name gives it away.
        let p = dir.join("proc/4242");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("comm"), "python3\n").unwrap();
        fs::write(
            p.join("cmdline"),
            "/usr/bin/python3\0/usr/bin/auto-cpufreq\0--daemon\0",
        )
        .unwrap();
        assert_eq!(likely_tool(&root, s, "60").as_deref(), Some("auto-cpufreq"));
        fs::remove_dir_all(&dir).ok();
    }
}
