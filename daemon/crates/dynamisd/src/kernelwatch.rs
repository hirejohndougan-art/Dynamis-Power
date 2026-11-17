//! Watching the kernel log all the time, not only during tests (everyday
//! problem §1).
//!
//! On each reading, new kernel log records are checked for known signs of
//! trouble from a part of the laptop a setting could affect: Wi-Fi firmware
//! crashes, USB resets, GPU hangs, sound chip timeouts, disk errors, lockups
//! ([`trouble::from_kernel`]). When one appears **within ten minutes of a
//! change** to a setting that could cause it, that change is put back and
//! blocked on this laptop. Trouble with no such recent change is only noted
//! (at most once an hour per kind), so an unrelated fault never blocks a good
//! setting.
//!
//! Not while a test is running (the test's own health monitor judges it),
//! during the sleep test, or in the first two minutes after waking from
//! sleep, when resets and link messages are normal.
//!
//! [`trouble::from_kernel`]: dynamis_core::trouble::from_kernel

use std::collections::BTreeMap;
use std::path::Path;

use dynamis_core::health::{self, Monitor};
use dynamis_core::{trouble, SysRoot};

use crate::core::open_engine;
use crate::trouble::{put_back_and_block, what};

/// A change this recent can be blamed.
pub const RECENT_S: u64 = 600;
/// Quiet after waking from sleep.
pub const AFTER_WAKE_S: u64 = 120;
/// At most one note per kind in this time when nothing is blamed.
const NOTE_EVERY_S: u64 = 3600;

pub struct KernelWatch {
    log: Monitor,
    quiet_until: u64,
    noted: BTreeMap<&'static str, u64>,
}

/// What a check did, for the caller to signal the app.
#[derive(Debug, Default, PartialEq)]
pub struct Acted {
    pub blocked: Vec<String>,
    pub noted: usize,
}

/// The ids changed since `since`, newest first: settings, and the device
/// groups (`usb.autosuspend`, `pci.runtime_pm`) for devices let sleep.
fn recent_changes(engine: &dynamis_core::engine::Engine, since: u64) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for e in engine.journal(400).into_iter().filter(|e| e.time >= since) {
        let id = match e.event.as_str() {
            "done" | "keep" if e.from != e.to => e.id.clone(),
            "device-power" if e.to.as_deref() == Some("auto") => {
                if e.id.starts_with("usb:") {
                    "usb.autosuspend".into()
                } else {
                    "pci.runtime_pm".into()
                }
            }
            _ => continue,
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

fn short(message: &str) -> String {
    let m: String = message.chars().take(160).collect();
    if m.len() < message.len() {
        format!("{m}…")
    } else {
        m
    }
}

impl KernelWatch {
    /// Start at the end of the kernel log: older records never count.
    pub fn start(root: &SysRoot) -> KernelWatch {
        KernelWatch {
            log: Monitor::start(root, health::Limits::default()),
            quiet_until: 0,
            noted: BTreeMap::new(),
        }
    }

    pub fn watching(&self) -> bool {
        self.log.watching_kernel_log()
    }

    /// The laptop woke: resets and link messages are normal for a while.
    pub fn after_wake(&mut self, now: u64) {
        self.quiet_until = now + AFTER_WAKE_S;
    }

    /// Read what is new and act on it. `busy` is true while a test or the
    /// sleep test is running (their own monitors judge the log then).
    pub fn check(&mut self, root: &SysRoot, dir: &Path, now: u64, busy: bool) -> Acted {
        let records = self.log.kernel_records();
        if records.is_empty() || busy || now < self.quiet_until {
            return Acted::default();
        }
        let mut acted = Acted::default();
        // One answer per kind per reading.
        let mut seen: Vec<&'static str> = Vec::new();
        for (prio, message) in records {
            let Some(kind) = trouble::from_kernel(prio, &message) else {
                continue;
            };
            if seen.contains(&kind) {
                continue;
            }
            seen.push(kind);
            let mut engine = match open_engine(root, dir) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("dynamisd: kernel watch: {e}");
                    return acted;
                }
            };
            let suspects = trouble::suspects(kind);
            let blamed: Vec<String> = recent_changes(&engine, now.saturating_sub(RECENT_S))
                .into_iter()
                .filter(|id| suspects.contains(&id.as_str()))
                .collect();
            let msg = short(&message);
            if blamed.is_empty() {
                if self.noted.get(kind).is_some_and(|t| now < t + NOTE_EVERY_S) {
                    continue;
                }
                self.noted.insert(kind, now);
                let _ = engine.note(
                    "kernel",
                    kind,
                    &format!(
                        "{msg}; nothing changed in the last 10 minutes could cause trouble with {}, so nothing was blocked",
                        what(kind)
                    ),
                );
                acted.noted += 1;
                continue;
            }
            let reason = format!(
                "the kernel reported trouble with {} within 10 minutes of this change: {msg}",
                what(kind)
            );
            for id in &blamed {
                if let Err(e) = put_back_and_block(&mut engine, id, &reason) {
                    eprintln!("dynamisd: kernel watch: {id}: {e}");
                }
            }
            let _ = engine.note(
                "kernel",
                kind,
                &format!("{msg}; undone and blocked: {}", blamed.join(", ")),
            );
            acted.blocked.extend(blamed);
        }
        acted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sandbox(name: &str) -> (std::path::PathBuf, SysRoot, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("dynamis-kwatch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("root");
        std::fs::create_dir_all(root.join("dev")).unwrap();
        std::fs::write(
            root.join("dev/kmsg"),
            "3,1,1,-;old trouble: GPU HANG before we started\n",
        )
        .unwrap();
        let snd = root.join("sys/module/snd_hda_intel/parameters");
        std::fs::create_dir_all(&snd).unwrap();
        std::fs::write(snd.join("power_save"), "0\n").unwrap();
        std::fs::write(snd.join("power_save_controller"), "N\n").unwrap();
        let state = dir.join("state");
        std::fs::create_dir_all(&state).unwrap();
        (dir.clone(), SysRoot::at(root), state)
    }

    fn kernel_says(dir: &Path, line: &str) {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join("root/dev/kmsg"))
            .unwrap();
        writeln!(f, "{line}").unwrap();
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn trouble_right_after_a_matching_change_undoes_and_blocks_it() {
        let (dir, root, state) = sandbox("match");
        let mut w = KernelWatch::start(&root);
        // A change to the sound chip, then unrelated Wi-Fi trouble: noted only.
        let mut e = open_engine(&root, &state).unwrap();
        e.apply(&dynamis_core::engine::Request {
            id: "audio.power_save",
            value: "1",
            reason: "test",
            trial_secs: None,
            allow_risky: false,
        })
        .unwrap();
        drop(e);
        kernel_says(
            &dir,
            "3,2,2,-;iwlwifi 0000:00:14.3: Microcode SW error detected",
        );
        let a = w.check(&root, &state, now(), false);
        assert_eq!(
            a,
            Acted {
                blocked: vec![],
                noted: 1
            }
        );
        // Sound trouble: the sound change is undone and blocked.
        kernel_says(&dir, "3,3,3,-;snd_hda_intel 0000:00:1f.3: azx_get_response timeout, switching to polling mode");
        let a = w.check(&root, &state, now(), false);
        assert_eq!(a.blocked, ["audio.power_save"]);
        let e = open_engine(&root, &state).unwrap();
        assert!(e.blocked()["audio.power_save"].contains("kernel reported trouble with sound"));
        assert_eq!(
            std::fs::read_to_string(
                dir.join("root/sys/module/snd_hda_intel/parameters/power_save")
            )
            .unwrap()
            .trim(),
            "0"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quiet_after_waking_and_while_testing() {
        let (dir, root, state) = sandbox("quiet");
        let mut w = KernelWatch::start(&root);
        w.after_wake(now());
        kernel_says(
            &dir,
            "6,2,2,-;usb 1-2: reset high-speed USB device number 3 using xhci_hcd",
        );
        assert_eq!(w.check(&root, &state, now(), false), Acted::default());
        kernel_says(
            &dir,
            "6,3,3,-;usb 1-2: reset high-speed USB device number 3 using xhci_hcd",
        );
        assert_eq!(w.check(&root, &state, now() + 3600, true), Acted::default());
        // The old record from before the start never counted.
        assert!(open_engine(&root, &state).unwrap().journal(10).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
