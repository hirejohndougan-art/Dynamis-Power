//! Other programs that manage power settings: which are running, which are
//! set to start at boot, and which one most likely changed a setting.
//! Read-only; used to explain outside changes and in the diagnostic report.

use crate::fs::SysRoot;
use crate::settings::Setting;

/// Processes that set power settings: (name in `comm` or the program file
/// name, name for people).
pub const TOOLS: &[(&str, &str)] = &[
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
pub const UNITS: &[(&str, &str)] = &[
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

/// The program, rule or service that most likely changed a setting:
/// a running tool, then a udev rule writing the same file, then a tool's
/// service set to start at boot.
pub fn likely(root: &SysRoot, s: &dyn Setting, value: &str) -> Option<String> {
    running(root)
        .into_iter()
        .next()
        .or_else(|| udev_rule(root, s, value))
        .or_else(|| enabled_at_boot(root).into_iter().next())
}

/// Known tools running now (by name for people), without repeats.
pub fn running(root: &SysRoot) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for pid in root.list_dir("/proc") {
        if let Some(t) = tool_of(root, &pid) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

/// Known tools whose service is set to start at boot.
pub fn enabled_at_boot(root: &SysRoot) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for dir in UNIT_DIRS {
        for wants in root.list_dir(dir) {
            if !wants.ends_with(".wants") {
                continue;
            }
            let present = root.list_dir(&format!("{dir}/{wants}"));
            for (unit, nice) in UNITS {
                if present.iter().any(|p| p == unit) && !out.iter().any(|o| o == nice) {
                    out.push(nice.to_string());
                }
            }
        }
    }
    out
}

/// The known tool a process is, by its `comm` or program file name (a
/// Python tool shows as `python3`, so the script name counts too).
fn tool_of(root: &SysRoot, pid: &str) -> Option<String> {
    if !pid.bytes().all(|b| b.is_ascii_digit()) || pid == std::process::id().to_string() {
        return None;
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
    TOOLS.iter().find_map(|(name, nice)| {
        let matches = |n: &str| n == *name || n.strip_suffix(".py") == Some(name);
        (matches(comm.trim()) || args.iter().any(|a| matches(a))).then(|| nice.to_string())
    })
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
