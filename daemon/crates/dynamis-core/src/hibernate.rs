//! Whether this laptop could hibernate, and what is missing (decision
//! 0025). Read-only: Dynamis Power never sets hibernation up (that touches
//! the bootloader and persistent configuration, safety rules 3 and 5). It
//! only says, in plain words, whether "sleep, then hibernate when the
//! battery runs low" could be switched on, and what to do first.

use serde::Serialize;

use crate::fs::SysRoot;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Readiness {
    /// Everything needed is there.
    pub ready: bool,
    /// The kernel offers hibernation (`disk` in `/sys/power/state`).
    pub kernel_offers: bool,
    pub memory_mb: Option<u64>,
    /// Swap that can hold an image of memory (partitions and files).
    pub swap_mb: Option<u64>,
    /// The kernel knows where to resume from (`/sys/power/resume` set).
    pub resume_set: bool,
    /// Kernel lockdown (often with Secure Boot) blocks hibernation.
    pub locked_down: bool,
    /// What is missing: `kernel`, `lockdown`, `swap`, `resume` (the app
    /// words them; [`describe`] gives English for reports).
    pub missing: Vec<&'static str>,
}

/// One missing piece in plain English (for `dynamis stat`).
pub fn describe(key: &str) -> &'static str {
    match key {
        "kernel" => "the kernel does not offer hibernation here",
        "lockdown" => "kernel lockdown (often Secure Boot) blocks it",
        "swap" => "not enough swap space (at least as much as the memory)",
        "resume" => "the system is not told where to resume from (the resume setting)",
        _ => "something else is missing",
    }
}

fn meminfo_kb(root: &SysRoot, key: &str) -> Option<u64> {
    root.read_string("/proc/meminfo")
        .ok()?
        .lines()
        .find_map(|l| {
            let rest = l.strip_prefix(key)?.strip_prefix(':')?;
            rest.split_whitespace().next()?.parse().ok()
        })
}

pub fn check(root: &SysRoot) -> Readiness {
    let kernel_offers = root
        .read_string("/sys/power/state")
        .is_ok_and(|s| s.split_whitespace().any(|w| w == "disk"));
    let memory_mb = meminfo_kb(root, "MemTotal").map(|k| k / 1024);
    let swap_mb = meminfo_kb(root, "SwapTotal").map(|k| k / 1024);
    let resume_set = root
        .read_string("/sys/power/resume")
        .is_ok_and(|s| !matches!(s.trim(), "" | "0:0"));
    let locked_down = root
        .read_string("/sys/kernel/security/lockdown")
        .is_ok_and(|s| s.contains("[integrity]") || s.contains("[confidentiality]"));
    let mut missing = Vec::new();
    if !kernel_offers {
        missing.push("kernel");
    }
    if locked_down {
        missing.push("lockdown");
    }
    // The image is compressed, but space for all of memory is the safe rule.
    let enough_swap = matches!((swap_mb, memory_mb), (Some(s), Some(m)) if s >= m);
    if !enough_swap {
        missing.push("swap");
    }
    if !resume_set {
        missing.push("resume");
    }
    Readiness {
        ready: missing.is_empty(),
        kernel_offers,
        memory_mb,
        swap_mb,
        resume_set,
        locked_down,
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str, files: &[(&str, &str)]) -> (std::path::PathBuf, SysRoot) {
        let dir = std::env::temp_dir().join(format!("dynamis-hib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (p, t) in files {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, t).unwrap();
        }
        (dir.clone(), SysRoot::at(dir))
    }

    #[test]
    fn ready_only_with_everything() {
        let (d, r) = root(
            "ok",
            &[
                ("sys/power/state", "freeze mem disk\n"),
                ("sys/power/resume", "259:2\n"),
                (
                    "proc/meminfo",
                    "MemTotal: 4000000 kB\nSwapTotal: 4194300 kB\n",
                ),
            ],
        );
        let c = check(&r);
        assert!(c.ready, "{c:?}");
        std::fs::write(
            d.join("proc/meminfo"),
            "MemTotal: 4000000 kB\nSwapTotal: 0 kB\n",
        )
        .unwrap();
        std::fs::write(d.join("sys/power/resume"), "0:0\n").unwrap();
        let c = check(&r);
        assert!(!c.ready);
        assert_eq!(c.missing.len(), 2, "{c:?}");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn lockdown_is_said() {
        let (d, r) = root(
            "lock",
            &[
                ("sys/power/state", "freeze mem\n"),
                (
                    "sys/kernel/security/lockdown",
                    "none [integrity] confidentiality\n",
                ),
            ],
        );
        let c = check(&r);
        assert!(!c.kernel_offers && c.locked_down);
        assert!(c.missing.contains(&"lockdown"));
        let _ = std::fs::remove_dir_all(d);
    }
}
