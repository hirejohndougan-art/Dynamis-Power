//! What a learning result was measured on (everyday problem §6): the kernel,
//! the firmware (BIOS) version and date, and any start-up limit on how
//! deeply the processor may rest (`intel_idle.max_cstate=1` and the like:
//! with it, idle power is much higher, so what saves power changes). A
//! change to any of these re-checks the results, as does age.

use serde::{Deserialize, Serialize};

use crate::fs::SysRoot;

/// Results older than this are re-checked anyway (seasons, dust, wear).
pub const RECHECK_AFTER_S: u64 = 120 * 86_400;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemId {
    /// `/proc/sys/kernel/osrelease`, e.g. `6.11.0-21-generic`.
    pub kernel: String,
    /// BIOS version and date, e.g. `E410MA.316 05/12/2023`.
    pub firmware: String,
    /// The start-up limits on processor rest, as on the kernel command line
    /// (`intel_idle.max_cstate=1`), or `none`. `None`: not recorded (a
    /// result from before this was kept), which never counts as a change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_limit: Option<String>,
}

/// The kernel command line's limits on processor rest, or `none`.
pub fn idle_limit(root: &SysRoot) -> String {
    let caps: Vec<String> = root
        .read_string("/proc/cmdline")
        .unwrap_or_default()
        .split_whitespace()
        .filter(|p| {
            p.starts_with("intel_idle.max_cstate=")
                || p.starts_with("processor.max_cstate=")
                || p.starts_with("idle=")
        })
        .map(str::to_string)
        .collect();
    if caps.is_empty() {
        "none".into()
    } else {
        caps.join(" ")
    }
}

impl SystemId {
    pub fn read(root: &SysRoot) -> SystemId {
        let read = |p: &str| {
            root.read_string(p)
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        let fw = [
            read("/sys/class/dmi/id/bios_version"),
            read("/sys/class/dmi/id/bios_date"),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
        SystemId {
            kernel: read("/proc/sys/kernel/osrelease"),
            firmware: fw,
            idle_limit: Some(idle_limit(root)),
        }
    }

    /// What changed from `old` to `self`, in plain words, or `None`.
    pub fn change_from(&self, old: &SystemId) -> Option<String> {
        let mut parts = Vec::new();
        if old.kernel != self.kernel {
            parts.push(format!(
                "kernel {} → {}",
                or_unknown(&old.kernel),
                or_unknown(&self.kernel)
            ));
        }
        if old.firmware != self.firmware {
            parts.push(format!(
                "firmware {} → {}",
                or_unknown(&old.firmware),
                or_unknown(&self.firmware)
            ));
        }
        if let (Some(o), Some(n)) = (&old.idle_limit, &self.idle_limit) {
            if o != n {
                parts.push(format!("processor rest limit {o} → {n}"));
            }
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

fn or_unknown(s: &str) -> &str {
    if s.is_empty() {
        "unknown"
    } else {
        s
    }
}

/// The leading numbers of a kernel version: `6.11.0-21-generic` → [6, 11, 0].
pub fn kernel_numbers(release: &str) -> Vec<u32> {
    release
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .next()
        .unwrap_or("")
        .split('.')
        .map_while(|p| p.parse().ok())
        .collect()
}

/// Whether kernel `a` is older than `b` (by their leading numbers).
pub fn kernel_older(a: &str, b: &str) -> bool {
    kernel_numbers(a) < kernel_numbers(b)
}

/// A firmware date (`MM/DD/YYYY`, as the DMI table gives it) as
/// `YYYY-MM-DD`, for comparing.
pub fn firmware_date(firmware: &str) -> Option<String> {
    let d = firmware.split_whitespace().last()?;
    let mut it = d.split('/');
    let (m, day, y) = (it.next()?, it.next()?, it.next()?);
    (y.len() == 4 && m.len() <= 2 && day.len() <= 2).then(|| format!("{y}-{m:0>2}-{day:0>2}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_in_the_rest_limit_counts_but_an_unrecorded_one_does_not() {
        let capped = SystemId {
            kernel: "7.0.0".into(),
            firmware: "E410MA.303".into(),
            idle_limit: Some("intel_idle.max_cstate=1".into()),
        };
        let raised = SystemId {
            idle_limit: Some("intel_idle.max_cstate=3".into()),
            ..capped.clone()
        };
        assert_eq!(
            raised.change_from(&capped).unwrap(),
            "processor rest limit intel_idle.max_cstate=1 → intel_idle.max_cstate=3"
        );
        let old = SystemId {
            idle_limit: None,
            ..capped.clone()
        };
        assert_eq!(
            capped.change_from(&old),
            None,
            "older results did not record it"
        );
        let dir = std::env::temp_dir().join(format!("dynamis-sysid-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("proc")).unwrap();
        std::fs::write(
            dir.join("proc/cmdline"),
            "ro quiet intel_idle.max_cstate=1 preempt=full\n",
        )
        .unwrap();
        assert_eq!(idle_limit(&SysRoot::at(&dir)), "intel_idle.max_cstate=1");
        std::fs::write(dir.join("proc/cmdline"), "ro quiet\n").unwrap();
        assert_eq!(idle_limit(&SysRoot::at(&dir)), "none");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn versions_and_dates() {
        assert_eq!(kernel_numbers("6.11.0-21-generic"), [6, 11, 0]);
        assert_eq!(kernel_numbers("6.8.12-arch1-1"), [6, 8, 12]);
        assert!(kernel_older("6.8.0-45-generic", "6.11"));
        assert!(!kernel_older("6.11.2", "6.11"));
        assert!(kernel_older("5.15.0", "6.1"));
        assert_eq!(
            firmware_date("E410MA.316 05/12/2023").as_deref(),
            Some("2023-05-12")
        );
        assert_eq!(firmware_date("1.2.3").as_deref(), None);
        let a = SystemId {
            kernel: "6.8.0".into(),
            firmware: "A 01/01/2024".into(),
            idle_limit: None,
        };
        let b = SystemId {
            kernel: "6.11.0".into(),
            ..a.clone()
        };
        assert_eq!(b.change_from(&a).unwrap(), "kernel 6.8.0 → 6.11.0");
        assert!(a.change_from(&a).is_none());
    }
}
