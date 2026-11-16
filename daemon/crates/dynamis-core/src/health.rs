//! Health monitor: watches the machine while a setting is on trial, so the
//! change is undone the moment something goes wrong rather than when the trial
//! times out.
//!
//! It looks for:
//!
//! - **Kernel trouble.** New kernel log messages at error level or worse, or
//!   matching known signs of trouble (GPU hangs, lockups, disk and storage
//!   timeouts, device resets, firmware crashes). Only messages that appear
//!   *after* the trial starts count.
//! - **Thrashing.** Memory or disk stalls (PSI "full", averaged over 10 s)
//!   above a limit and clearly above where they were when the trial began.
//! - **Overheating.** Any temperature sensor above 95 °C.
//!
//! It only reads. Reading the kernel log needs root; without it, the other
//! checks still run and [`Monitor::watching_kernel_log`] says so.

use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;

use serde::Serialize;

use crate::fs::SysRoot;
use crate::sensors::{pressure, thermal};

/// Limits beyond which a trial is considered unhealthy.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Memory "full" stall, % of time (all tasks waiting for memory).
    pub memory_full_pct: f64,
    /// Disk "full" stall, % of time.
    pub io_full_pct: f64,
    /// How far above the pre-trial level a stall must be, in % points, so a
    /// machine that was already busy does not fail every trial.
    pub stall_margin_pct: f64,
    pub max_temp_c: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            memory_full_pct: 10.0,
            io_full_pct: 30.0,
            stall_margin_pct: 5.0,
            max_temp_c: 95.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Problem {
    Kernel { message: String },
    MemoryThrashing { pct: f64 },
    DiskStalled { pct: f64 },
    Overheating { celsius: f64 },
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::Kernel { message } => write!(f, "kernel reported: {message}"),
            Problem::MemoryThrashing { pct } => {
                write!(f, "memory thrashing ({pct:.0}% of time stalled)")
            }
            Problem::DiskStalled { pct } => write!(f, "disk stalled ({pct:.0}% of time)"),
            Problem::Overheating { celsius } => write!(f, "overheating ({celsius:.0} °C)"),
        }
    }
}

/// Lower-case fragments of kernel messages that mean trouble even when the
/// driver logs them below error level.
const TROUBLE: &[&str] = &[
    "gpu hang",
    "hung task",
    "blocked for more than",
    "soft lockup",
    "hard lockup",
    "detected stall",
    "i/o error",
    "timed out",
    "timeout",
    "card stuck",
    "firmware crash",
    "microcode sw error",
    "aer:",
    "pcie bus error",
    "link is down",
    "reset high-speed usb",
    "reset full-speed usb",
    "reset superspeed usb",
];

/// Whether a kernel message is one of the trouble patterns.
pub fn is_trouble(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    TROUBLE.iter().any(|t| m.contains(t))
}

pub struct Monitor {
    root: SysRoot,
    limits: Limits,
    kmsg: Option<File>,
    pending: String,
    start_mem_full: f64,
    start_io_full: f64,
}

impl Monitor {
    /// Start watching. Everything already in the kernel log is skipped.
    pub fn start(root: &SysRoot, limits: Limits) -> Monitor {
        let kmsg = std::fs::OpenOptions::new()
            .read(true)
            // Non-blocking, so reading stops when there is nothing new instead
            // of waiting for the next kernel message.
            .custom_flags(libc::O_NONBLOCK)
            .open(root.path("/dev/kmsg"))
            .ok()
            .and_then(|mut f| f.seek(SeekFrom::End(0)).ok().map(|_| f));
        let (m, i) = stalls(root);
        Monitor {
            root: root.clone(),
            limits,
            kmsg,
            pending: String::new(),
            start_mem_full: m,
            start_io_full: i,
        }
    }

    pub fn watching_kernel_log(&self) -> bool {
        self.kmsg.is_some()
    }

    /// Everything wrong since the last check. Empty means healthy.
    pub fn check(&mut self) -> Vec<Problem> {
        let mut problems: Vec<Problem> = self
            .new_kernel_lines()
            .into_iter()
            .filter_map(|l| troubling(&l).map(|message| Problem::Kernel { message }))
            .collect();

        let l = self.limits;
        let (mem, io) = stalls(&self.root);
        if mem > l.memory_full_pct && mem > self.start_mem_full + l.stall_margin_pct {
            problems.push(Problem::MemoryThrashing { pct: mem });
        }
        if io > l.io_full_pct && io > self.start_io_full + l.stall_margin_pct {
            problems.push(Problem::DiskStalled { pct: io });
        }
        if let Some(t) = thermal::max_c(&thermal::read(&self.root)) {
            if t > l.max_temp_c {
                problems.push(Problem::Overheating { celsius: t });
            }
        }
        problems
    }

    fn new_kernel_lines(&mut self) -> Vec<String> {
        let Some(f) = self.kmsg.as_mut() else {
            return Vec::new();
        };
        // /dev/kmsg returns one record per read; a plain file (in tests)
        // returns many lines at once. Handle both by splitting on newlines.
        let mut buf = [0u8; 8192];
        loop {
            match f.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.pending.push_str(&String::from_utf8_lossy(&buf[..n])),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                // The kernel overwrote records we had not read yet; carry on.
                Err(e) if e.raw_os_error() == Some(libc::EPIPE) => continue,
                Err(_) => break,
            }
        }
        let Some(cut) = self.pending.rfind('\n') else {
            return Vec::new();
        };
        let complete: String = self.pending.drain(..=cut).collect();
        complete
            .lines()
            // Lines starting with a space are key=value continuations.
            .filter(|l| !l.starts_with(' ') && !l.is_empty())
            .map(String::from)
            .collect()
    }
}

/// `Some(message)` if a kernel log record (`prio,seq,time,flags;message`)
/// signals trouble.
fn troubling(record: &str) -> Option<String> {
    let (meta, message) = record.split_once(';')?;
    let prio: u32 = meta.split(',').next()?.parse().ok()?;
    let level = prio & 7; // the low three bits; the rest is the facility
    let lower = message.to_ascii_lowercase();
    (level <= 3 || TROUBLE.iter().any(|t| lower.contains(t))).then(|| message.trim().to_string())
}

/// Memory and disk "full" stall, % over the last 10 seconds.
fn stalls(root: &SysRoot) -> (f64, f64) {
    let p = pressure::read(root);
    let full = |x: Option<pressure::Psi>| x.and_then(|p| p.full).map(|l| l.avg10).unwrap_or(0.0);
    (full(p.memory), full(p.io))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_kernel_records() {
        assert!(troubling("3,100,5,-;i915 0000:00:02.0: [drm] GPU HANG: ecode 9:1").is_some());
        assert!(troubling("4,101,6,-;mmc0: Timeout waiting for hardware interrupt.").is_some());
        assert!(troubling("6,102,7,-;usb 1-1: reset high-speed USB device number 2").is_some());
        assert!(troubling("6,103,8,-;wlan0: associated").is_none());
        // Facility 3 (daemon), level 6: not trouble.
        assert!(troubling("30,104,9,-;systemd[1]: Started something.").is_none());
        // Facility 0, level 2 (critical): trouble whatever it says.
        assert!(troubling("2,105,10,-;anything").is_some());
        assert!(troubling("garbage").is_none());
    }
}
