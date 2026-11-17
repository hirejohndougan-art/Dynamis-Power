//! What a learning result was measured on (everyday problem §6): the kernel
//! and the firmware (BIOS) version and date. An update to either can change
//! what is best, so results measured on another kernel or firmware are
//! re-checked, as are results more than a few months old.

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
        };
        let b = SystemId {
            kernel: "6.11.0".into(),
            ..a.clone()
        };
        assert_eq!(b.change_from(&a).unwrap(), "kernel 6.8.0 → 6.11.0");
        assert!(a.change_from(&a).is_none());
    }
}
