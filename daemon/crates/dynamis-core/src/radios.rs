//! Radios (Bluetooth, Wi-Fi, mobile broadband) through the kernel's rfkill
//! switches (`/sys/class/rfkill/rfkillN`). Read-only here; the engine
//! switches them (`Engine::set_radio`), and `dynamisd/src/radios.rs`
//! decides when (TLP's `DEVICES_TO_DISABLE_ON_*`).

use serde::Serialize;

use crate::fs::SysRoot;

pub const RFKILL: &str = "/sys/class/rfkill";

/// The kinds of radio a person can switch off by rule.
pub const KINDS: &[&str] = &["bluetooth", "wlan", "wwan"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Radio {
    /// `rfkill0`.
    pub id: String,
    /// `bluetooth`, `wlan` or `wwan`.
    pub kind: String,
    /// The driver's name for it (`hci0`, `phy0`).
    pub name: String,
    /// Switched off in software.
    pub soft_off: bool,
    /// Switched off by a hardware switch or key (cannot be changed here).
    pub hard_off: bool,
}

/// An rfkill switch name the kernel could have made.
pub fn valid_id(id: &str) -> bool {
    id.strip_prefix("rfkill")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

pub fn scan(root: &SysRoot) -> Vec<Radio> {
    let mut out: Vec<Radio> = root
        .list_dir(RFKILL)
        .into_iter()
        .filter(|id| valid_id(id))
        .filter_map(|id| {
            let base = format!("{RFKILL}/{id}");
            let kind = root
                .read_string(&format!("{base}/type"))
                .ok()?
                .trim()
                .to_string();
            if !KINDS.contains(&kind.as_str()) {
                return None;
            }
            Some(Radio {
                name: root
                    .read_string(&format!("{base}/name"))
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                soft_off: root.read_string(&format!("{base}/soft")).ok()?.trim() == "1",
                hard_off: root
                    .read_string(&format!("{base}/hard"))
                    .is_ok_and(|h| h.trim() == "1"),
                kind,
                id,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Whether a network cable is plugged in and the link is up on a wired
/// card (not Wi-Fi, not virtual).
pub fn wired_up(root: &SysRoot) -> bool {
    root.list_dir("/sys/class/net").into_iter().any(|n| {
        let base = format!("/sys/class/net/{n}");
        n != "lo"
            && root.exists(&format!("{base}/device"))
            && !root.exists(&format!("{base}/wireless"))
            && !root.exists(&format!("{base}/phy80211"))
            && root
                .read_string(&format!("{base}/type"))
                .is_ok_and(|t| t.trim() == "1")
            && root
                .read_string(&format!("{base}/carrier"))
                .is_ok_and(|c| c.trim() == "1")
    })
}

/// Whether the laptop is in a dock (TLP's dock detection, simplified): an
/// ACPI dock station reporting `docked`, or a Thunderbolt device that calls
/// itself a dock.
/// Bluetooth connections open now: each one is a `hciN:handle` entry in
/// `/sys/class/bluetooth`, so no Bluetooth daemon needs asking.
pub fn bluetooth_connections(root: &SysRoot) -> usize {
    root.list_dir("/sys/class/bluetooth")
        .into_iter()
        .filter(|n| n.starts_with("hci") && n.contains(':'))
        .count()
}

pub fn docked(root: &SysRoot) -> bool {
    let acpi = root
        .list_dir("/sys/devices/platform")
        .into_iter()
        .filter(|d| d.starts_with("dock."))
        .any(|d| {
            root.read_string(&format!("/sys/devices/platform/{d}/docked"))
                .is_ok_and(|v| v.trim() == "1")
        });
    acpi || root
        .list_dir("/sys/bus/thunderbolt/devices")
        .into_iter()
        .filter(|d| !d.contains('/') && d.contains('-'))
        .any(|d| {
            root.read_string(&format!("/sys/bus/thunderbolt/devices/{d}/device_name"))
                .is_ok_and(|n| n.to_ascii_lowercase().contains("dock"))
        })
}

/// For tests: make an rfkill switch in a fixture.
/// Keyboard backlights (`/sys/class/leds/*::kbd_backlight`): their
/// directories, sorted.
pub fn keyboard_lights(root: &SysRoot) -> Vec<String> {
    let mut v: Vec<String> = root
        .list_dir("/sys/class/leds")
        .into_iter()
        .filter(|n| valid_kbd_light(n))
        .map(|n| format!("/sys/class/leds/{n}"))
        .collect();
    v.sort();
    v
}

/// A keyboard backlight's name (`tpacpi::kbd_backlight`, `asus::kbd_backlight`).
pub fn valid_kbd_light(name: &str) -> bool {
    name.ends_with("::kbd_backlight")
        && !name.contains('/')
        && !name.contains("..")
        && name.len() < 64
}

pub mod fixture {
    use std::path::Path;

    /// A keyboard backlight at `level` of `max`.
    pub fn kbd_light(root: &Path, name: &str, level: u32, max: u32) {
        let d = root.join("sys/class/leds").join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("brightness"), format!("{level}\n")).unwrap();
        std::fs::write(d.join("max_brightness"), format!("{max}\n")).unwrap();
    }

    pub fn add(root: &Path, id: &str, kind: &str, name: &str, soft_off: bool) {
        let d = root.join("sys/class/rfkill").join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("type"), format!("{kind}\n")).unwrap();
        std::fs::write(d.join("name"), format!("{name}\n")).unwrap();
        std::fs::write(d.join("soft"), if soft_off { "1\n" } else { "0\n" }).unwrap();
        std::fs::write(d.join("hard"), "0\n").unwrap();
    }

    /// An ACPI dock station, docked or not.
    /// A Bluetooth connection (`connected`) or none.
    pub fn bt_connection(root: &Path, connected: bool) {
        let d = root.join("sys/class/bluetooth/hci0:3585");
        if connected {
            std::fs::create_dir_all(&d).unwrap();
        } else {
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    pub fn dock(root: &Path, docked: bool) {
        let d = root.join("sys/devices/platform/dock.0");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("docked"), if docked { "1\n" } else { "0\n" }).unwrap();
    }

    /// A wired card, with the cable in or out.
    pub fn wired(root: &Path, ifname: &str, cable_in: bool) {
        let d = root.join("sys/class/net").join(ifname);
        std::fs::create_dir_all(d.join("device")).unwrap();
        std::fs::write(d.join("type"), "1\n").unwrap();
        std::fs::write(d.join("carrier"), if cable_in { "1\n" } else { "0\n" }).unwrap();
    }
}
