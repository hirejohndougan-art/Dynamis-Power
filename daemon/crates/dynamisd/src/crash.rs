//! What was happening when the laptop last stopped without shutting down.
//!
//! After an unclean shutdown (a freeze, crash or power cut), the service
//! reads the end of the previous boot's kernel log, read-only, and its own
//! journal, and keeps a short plain account for the app: was the laptop
//! going to sleep or waking, did a driver report trouble just before, and
//! was Dynamis Power changing anything then or in the ten minutes before.
//! Settings changed in those ten minutes become safe-mode suspects
//! (`dynamis_core::boots`).
//!
//! The kernel log comes from `journalctl -b -1 -k`, which needs a persistent
//! system journal; without one the account says there is no log. Tests use
//! a file in the fixture tree instead (never the real system's log).

use std::path::Path;
use std::process::Command;

use dynamis_core::boots::BootCheck;
use dynamis_core::engine::Entry;
use dynamis_core::{health, settings, SysRoot};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::write_atomic;

const FILE: &str = "last-unclean.json";
const VERSION: u32 = 1;
/// Changes this long before the end count as "just before".
const RECENT_S: f64 = 600.0;
/// Trouble this long before the end counts.
const TROUBLE_S: f64 = 300.0;
/// Waking this long before the end counts as "shortly after waking".
const WAKE_S: f64 = 120.0;
/// Where tests put the previous boot's kernel log, inside a fixture root.
pub const FIXTURE_LOG: &str = "/var/log/dynamis-test/previous-boot-kernel.log";

/// What the kernel log shows at the end of the previous boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kernel {
    /// Going to sleep, asleep, or waking (no "exit" after the last "entry").
    Sleeping,
    /// Within two minutes of waking.
    Waking,
    /// A driver reported trouble in the last five minutes.
    Driver,
    /// The log ends with nothing unusual.
    Nothing,
    /// No log from that boot.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnosis {
    pub v: u32,
    /// When this was worked out (Unix time).
    pub found_at: f64,
    /// The last kernel log line's time, if there was a log.
    pub ended_at: Option<f64>,
    pub kernel: Kernel,
    /// The trouble line, for Details.
    #[serde(default)]
    pub detail: Option<String>,
    /// A change in progress, put back at startup (setting id).
    #[serde(default)]
    pub in_progress: Option<String>,
    /// Settings changed in the ten minutes before.
    #[serde(default)]
    pub recent: Vec<String>,
    /// Safe mode was on, so Dynamis Power was changing nothing.
    pub in_safe_mode: bool,
}

/// Read the kernel log's last lines from the previous boot, as
/// `(unix time, message)`.
fn previous_boot_log(root: &SysRoot) -> Option<Vec<(f64, String)>> {
    let text = if root.root() == Path::new("/") {
        let out = Command::new("journalctl")
            .args([
                "-b",
                "-1",
                "-k",
                "-o",
                "short-unix",
                "--no-pager",
                "-q",
                "-n",
                "400",
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        root.read_string(FIXTURE_LOG).ok()?
    };
    let lines: Vec<(f64, String)> = text
        .lines()
        .filter_map(|l| {
            let (ts, rest) = l.split_once(' ')?;
            let ts: f64 = ts.parse().ok()?;
            let msg = rest.split_once("kernel: ").map_or(rest, |(_, m)| m);
            Some((ts, msg.trim().to_string()))
        })
        .collect();
    (!lines.is_empty()).then_some(lines)
}

/// Read the kernel log's end: what it shows, when it ended, and a trouble
/// line if any.
pub fn read_kernel(lines: Option<&[(f64, String)]>) -> (Kernel, Option<f64>, Option<String>) {
    let Some(lines) = lines.filter(|l| !l.is_empty()) else {
        return (Kernel::Unknown, None, None);
    };
    let end = lines.last().map(|l| l.0).unwrap_or_default();
    let trouble = lines
        .iter()
        .rev()
        .take_while(|(t, _)| end - t <= TROUBLE_S)
        .find(|(_, m)| health::is_trouble(m))
        .map(|(_, m)| m.chars().take(200).collect::<String>());
    let last_pm = lines.iter().rev().find_map(|(t, m)| {
        let m = m.to_ascii_lowercase();
        if m.contains("pm: suspend entry") || m.contains("pm: hibernation entry") {
            Some((*t, true))
        } else if m.contains("pm: suspend exit") || m.contains("pm: hibernation exit") {
            Some((*t, false))
        } else {
            None
        }
    });
    let kernel = match last_pm {
        Some((_, true)) => Kernel::Sleeping,
        Some((t, false)) if end - t <= WAKE_S => Kernel::Waking,
        _ if trouble.is_some() => Kernel::Driver,
        _ => Kernel::Nothing,
    };
    (kernel, Some(end), trouble)
}

/// Settings changed in the ten minutes before `end`, or after it: the kernel
/// log often stops before the laptop does, and the journal at this point
/// holds only the previous boot's changes (this one has applied nothing yet).
pub fn recent_changes(journal: &[Entry], end: f64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in journal {
        let t = e.time as f64;
        if matches!(e.event.as_str(), "start" | "done")
            && end - t <= RECENT_S
            && settings::by_id(&e.id).is_some()
            && !out.contains(&e.id)
        {
            out.push(e.id.clone());
        }
    }
    out
}

/// Work out and keep the account after an unclean shutdown. `in_progress`
/// is the change the engine put back at startup, if any.
pub fn diagnose(
    root: &SysRoot,
    state_dir: &Path,
    boot: &BootCheck,
    in_progress: Option<String>,
    journal: &[Entry],
    now: f64,
) -> Option<Diagnosis> {
    if !boot.unclean {
        return None;
    }
    let lines = previous_boot_log(root);
    let (kernel, ended_at, detail) = read_kernel(lines.as_deref());
    let recent = match (ended_at, boot.unclean_in_safe_mode) {
        (Some(end), false) => recent_changes(journal, end),
        _ => Vec::new(),
    };
    let d = Diagnosis {
        v: VERSION,
        found_at: now,
        ended_at,
        kernel,
        detail,
        in_progress,
        recent,
        in_safe_mode: boot.unclean_in_safe_mode,
    };
    let mut suspects = d.recent.clone();
    suspects.extend(d.in_progress.clone());
    if !d.in_safe_mode && !suspects.is_empty() {
        if let Err(e) = dynamis_core::boots::add_suspects(state_dir, &suspects) {
            eprintln!("dynamisd: {e}");
        }
    }
    match serde_json::to_string_pretty(&d) {
        Ok(t) => {
            if let Err(e) = write_atomic(&state_dir.join(FILE), &t) {
                eprintln!("dynamisd: {e}");
            }
        }
        Err(e) => eprintln!("dynamisd: {e}"),
    }
    Some(d)
}

pub fn load(state_dir: &Path) -> Option<Diagnosis> {
    std::fs::read_to_string(state_dir.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

/// For `Status`: the last account, the unclean shutdowns while safe mode was
/// on, and the suspects (with labels).
pub fn json(state_dir: &Path) -> Value {
    let label = |id: &str| settings::by_id(id).map(|s| s.label()).unwrap_or("");
    let (suspects, in_safe_mode) = dynamis_core::boots::suspects(state_dir);
    json!({
        "last": load(state_dir).map(|d| json!({
            "found_at": d.found_at,
            "ended_at": d.ended_at,
            "kernel": d.kernel,
            "detail": d.detail,
            "in_progress": d.in_progress.as_deref().map(|id| json!({"id": id, "label": label(id)})),
            "recent": d.recent.iter().map(|id| json!({"id": id, "label": label(id)})).collect::<Vec<_>>(),
            "in_safe_mode": d.in_safe_mode,
        })),
        "in_safe_mode": in_safe_mode,
        "suspects": suspects.iter().map(|id| json!({"id": id, "label": label(id)})).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(lines: &[(f64, &str)]) -> Vec<(f64, String)> {
        lines.iter().map(|(t, m)| (*t, m.to_string())).collect()
    }

    #[test]
    fn reads_the_end_of_the_kernel_log() {
        assert_eq!(read_kernel(None).0, Kernel::Unknown);
        let asleep = log(&[
            (100.0, "wlan0: associated"),
            (200.0, "PM: suspend entry (s2idle)"),
        ]);
        assert_eq!(
            read_kernel(Some(&asleep)),
            (Kernel::Sleeping, Some(200.0), None)
        );
        let woke = log(&[
            (100.0, "PM: suspend entry (deep)"),
            (900.0, "PM: suspend exit"),
            (
                950.0,
                "usb 1-2: reset high-speed USB device number 3 using xhci_hcd",
            ),
        ]);
        let (k, _, detail) = read_kernel(Some(&woke));
        assert_eq!(k, Kernel::Waking);
        assert!(detail.unwrap().contains("reset high-speed"));
        let gpu = log(&[
            (100.0, "PM: suspend exit"),
            (
                5000.0,
                "i915 0000:00:02.0: [drm] GPU HANG: ecode 9:1:85dffffb",
            ),
            (5010.0, "audit: something"),
        ]);
        assert_eq!(read_kernel(Some(&gpu)).0, Kernel::Driver);
        let old_trouble = log(&[(100.0, "GPU HANG"), (5000.0, "audit: something")]);
        assert_eq!(
            read_kernel(Some(&old_trouble)),
            (Kernel::Nothing, Some(5000.0), None)
        );
    }

    #[test]
    fn finds_changes_in_the_ten_minutes_before() {
        let e = |time: u64, event: &str, id: &str| Entry {
            v: 1,
            time,
            event: event.into(),
            id: id.into(),
            from: None,
            to: None,
            reason: String::new(),
        };
        let j = vec![
            e(1000, "done", "cpu.turbo"),
            e(9500, "start", "cpu.epp"),
            e(9500, "done", "cpu.epp"),
            e(9700, "app-weight", "app:firefox"),
            e(9800, "done", "not.a.setting"),
            e(9900, "restore", "vm.swappiness"),
        ];
        assert_eq!(recent_changes(&j, 10_000.0), vec!["cpu.epp".to_string()]);
    }
}
