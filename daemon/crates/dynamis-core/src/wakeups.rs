//! What wakes the laptop from sleep, and how much battery sleep costs.
//!
//! The kernel counts, per wake-up source (`/sys/class/wakeup/wakeupN`),
//! how often it woke the system (`wakeup_count`) and how often it fired
//! (`event_count`). A reading before sleep and one after waking tell which
//! source did it: the lid, the power button, a key, the charger, a USB
//! device, the network, or the clock. Read-only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::fs::SysRoot;

const WAKEUP: &str = "/sys/class/wakeup";

/// Counts per source name: (wakeup_count, event_count).
pub type Counts = BTreeMap<String, (u64, u64)>;

pub fn read(root: &SysRoot) -> Counts {
    root.list_dir(WAKEUP)
        .into_iter()
        .filter(|d| d.starts_with("wakeup"))
        .filter_map(|d| {
            let base = format!("{WAKEUP}/{d}");
            let name = root
                .read_string(&format!("{base}/name"))
                .ok()?
                .trim()
                .to_string();
            let w = root.read_u64(&format!("{base}/wakeup_count")).unwrap_or(0);
            let e = root.read_u64(&format!("{base}/event_count")).unwrap_or(0);
            (!name.is_empty()).then_some((name, (w, e)))
        })
        .collect()
}

/// The sources that woke the system between two readings: those whose
/// wake-up count rose; failing that, those that fired (some drivers only
/// count events).
pub fn woke_by(before: &Counts, after: &Counts) -> Vec<String> {
    let rose = |pick: fn(&(u64, u64)) -> u64| -> Vec<String> {
        after
            .iter()
            .filter(|(n, a)| before.get(*n).is_none_or(|b| pick(a) > pick(b)))
            .filter(|(_, a)| pick(a) > 0)
            .map(|(n, _)| n.clone())
            .collect()
    };
    let w = rose(|c| c.0);
    if w.is_empty() {
        rose(|c| c.1)
    } else {
        w
    }
}

/// A plain kind for a source name, for people: `lid`, `power_button`,
/// `charger`, `battery`, `clock`, `usb`, `network`, `keyboard`, or `other`.
pub fn kind(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.starts_with("pnp0c0d") {
        "lid"
    } else if n.starts_with("pnp0c0c")
        || n.starts_with("lnxpwrbn")
        || n.contains("power") && n.contains("button")
    {
        "power_button"
    } else if n.starts_with("acpi0003") || n == "ac" || n.starts_with("adp") {
        "charger"
    } else if n.starts_with("pnp0c0a") || n.starts_with("bat") {
        "battery"
    } else if n.starts_with("rtc") || n.contains("alarmtimer") {
        "clock"
    } else if n.contains("xhci")
        || n.contains("ehci")
        || n.starts_with("usb")
        || n.contains("-")
            && n.chars().next().is_some_and(|c| c.is_ascii_digit())
            && !n.contains(':')
    {
        "usb"
    } else if n.contains("wlan")
        || n.contains("wlp")
        || n.contains("eth")
        || n.contains("enp")
        || n.contains("iwlwifi")
        || n.contains("rtw")
    {
        "network"
    } else if n.contains("i8042")
        || n.contains("kbd")
        || n.contains("keyboard")
        || n.contains("serio")
    {
        "keyboard"
    } else {
        "other"
    }
}

/// One sleep, as remembered for the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sleep {
    /// Unix seconds it started and ended.
    pub from: f64,
    pub to: f64,
    /// Source names that woke it (empty if the kernel did not say).
    pub woke_by: Vec<String>,
    pub battery_before: Option<f64>,
    pub battery_after: Option<f64>,
    /// Plugged in at either end (the drain then means nothing).
    pub on_ac: bool,
}

impl Sleep {
    /// Battery percent used per hour asleep, where it means something.
    pub fn pct_per_hour(&self) -> Option<f64> {
        let h = (self.to - self.from) / 3600.0;
        match (self.battery_before, self.battery_after) {
            (Some(b), Some(a)) if !self.on_ac && h >= 0.25 && b >= a => Some((b - a) / h),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_that_rose_woke_it() {
        let before: Counts = [("PNP0C0D:00".into(), (3, 3)), ("rtc0".into(), (1, 1))].into();
        let after: Counts = [("PNP0C0D:00".into(), (4, 4)), ("rtc0".into(), (1, 1))].into();
        assert_eq!(woke_by(&before, &after), ["PNP0C0D:00"]);
        assert_eq!(kind("PNP0C0D:00"), "lid");
        assert_eq!(kind("LNXPWRBN:00"), "power_button");
        assert_eq!(kind("ACPI0003:00"), "charger");
        assert_eq!(kind("rtc0"), "clock");
        assert_eq!(kind("0000:00:14.0"), "other");
        // Only events counted: still found.
        let after: Counts = [("PNP0C0D:00".into(), (3, 5)), ("rtc0".into(), (1, 1))].into();
        assert_eq!(woke_by(&before, &after), ["PNP0C0D:00"]);
    }

    #[test]
    fn drain_per_hour() {
        let s = Sleep {
            from: 0.0,
            to: 8.0 * 3600.0,
            woke_by: vec![],
            battery_before: Some(90.0),
            battery_after: Some(82.0),
            on_ac: false,
        };
        assert_eq!(s.pct_per_hour(), Some(1.0));
        assert_eq!(Sleep { on_ac: true, ..s }.pct_per_hour(), None);
    }
}
