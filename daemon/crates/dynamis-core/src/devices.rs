//! USB and PCI devices that can be let sleep when idle (runtime power
//! management), and the ones that must not be.
//!
//! The kernel's `power/control` file per device is `on` (always powered) or
//! `auto` (the driver may power the device down when it is idle). Many
//! devices default to `on`. Setting `auto` saves power, but some devices
//! misbehave when they sleep: input devices miss the first key press or
//! lag, sound pops, printers and phones lose their connection, Bluetooth
//! and mobile broadband drop. Those are never touched, by class and by
//! driver. Graphics cards are left to their own drivers.
//!
//! This module only reads; the engine writes (`Engine::set_device_power`).

use serde::Serialize;

use crate::fs::SysRoot;

pub const USB_DIR: &str = "/sys/bus/usb/devices";
pub const PCI_DIR: &str = "/sys/bus/pci/devices";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Usb,
    Pci,
}

impl Kind {
    /// The setting id that a quirk or a block can use to stop this kind
    /// being managed on a laptop.
    pub fn setting_id(self) -> &'static str {
        match self {
            Kind::Usb => "usb.autosuspend",
            Kind::Pci => "pci.runtime_pm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Device {
    pub kind: Kind,
    /// Kernel path of the device directory, e.g. `/sys/bus/usb/devices/1-2`.
    pub path: String,
    /// Short id for the journal, e.g. `usb:1-2`.
    pub id: String,
    /// The device's own name, if it gives one (USB devices usually do).
    pub name: Option<String>,
    /// What kind of device, as a key the app translates: `usb_device` for
    /// USB, or the PCI class (storage, network, usb_controller, …).
    pub what: &'static str,
    /// Its address on the bus, e.g. `1-2` or `0000:00:14.0`.
    pub address: String,
    /// Which model of device it is (`usb:046d:c52b`, `pci:8086:a0ed`), the
    /// same whichever port it is in: what a person's "keep awake" list holds.
    pub model: Option<String>,
    /// `on` or `auto`.
    pub control: String,
    /// Why it is never touched, if so, as a key the app translates: input,
    /// sound, printer, phone, modem, bluetooth, graphics, management.
    pub excluded: Option<&'static str>,
}

/// USB interface classes never let sleep: HID (keyboards, mice, touchpads),
/// audio, printers, still image (phones, cameras), communications (modems,
/// mobile broadband), wireless controllers (Bluetooth).
const USB_EXCLUDED_CLASSES: &[(&str, &str)] = &[
    ("03", "input"),
    ("01", "sound"),
    ("07", "printer"),
    ("06", "phone"),
    ("02", "modem"),
    ("0a", "modem"),
    ("e0", "bluetooth"),
];

/// USB drivers never let sleep, for devices whose class does not say.
const USB_EXCLUDED_DRIVERS: &[(&str, &str)] = &[
    ("usbhid", "input"),
    ("btusb", "bluetooth"),
    ("snd-usb-audio", "sound"),
    ("usblp", "printer"),
    ("qmi_wwan", "modem"),
    ("cdc_mbim", "modem"),
    ("cdc_ncm", "modem"),
    ("cdc_acm", "modem"),
    ("cdc_wdm", "modem"),
    ("option", "modem"),
    ("sierra", "modem"),
    ("huawei_cdc_ncm", "modem"),
];

/// PCI drivers left alone: graphics drivers manage their own power, and the
/// Intel management engine is known to misbehave.
const PCI_EXCLUDED_DRIVERS: &[(&str, &str)] = &[
    ("nouveau", "graphics"),
    ("nvidia", "graphics"),
    ("radeon", "graphics"),
    ("amdgpu", "graphics"),
    ("i915", "graphics"),
    ("xe", "graphics"),
    ("mei_me", "management"),
];

fn driver(root: &SysRoot, dir: &str) -> Option<String> {
    root.link_name(&format!("{dir}/driver")).ok()
}

fn lower(root: &SysRoot, path: &str) -> Option<String> {
    root.read_string(path)
        .ok()
        .map(|s| s.trim().to_ascii_lowercase())
}

fn usb_excluded(root: &SysRoot, name: &str) -> Option<&'static str> {
    let dir = format!("{USB_DIR}/{name}");
    let class = lower(root, &format!("{dir}/bDeviceClass"));
    let mut classes: Vec<String> = class.into_iter().collect();
    let mut drivers = Vec::new();
    // Interfaces are listed next to the device as `1-2:1.0`.
    let prefix = format!("{name}:");
    for iface in root.list_dir(USB_DIR) {
        if !iface.starts_with(&prefix) {
            continue;
        }
        let idir = format!("{USB_DIR}/{iface}");
        classes.extend(lower(root, &format!("{idir}/bInterfaceClass")));
        drivers.extend(driver(root, &idir));
    }
    for c in &classes {
        let c = c.trim_start_matches("0x");
        let c = if c.len() == 1 {
            format!("0{c}")
        } else {
            c.to_string()
        };
        if let Some((_, why)) = USB_EXCLUDED_CLASSES.iter().find(|(k, _)| *k == c) {
            return Some(why);
        }
    }
    drivers
        .iter()
        .find_map(|d| USB_EXCLUDED_DRIVERS.iter().find(|(k, _)| k == d))
        .map(|(_, why)| *why)
}

/// The device's own name (manufacturer and product), if it gives one.
fn usb_name(root: &SysRoot, name: &str) -> Option<String> {
    let read = |f: &str| {
        root.read_string(&format!("{USB_DIR}/{name}/{f}"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    match (read("manufacturer"), read("product")) {
        (Some(m), Some(p)) if !p.contains(&m) && !name.starts_with("usb") => {
            Some(format!("{m} {p}"))
        }
        (_, p) => p,
    }
}

/// What kind of PCI function it is, as a key the app translates, and
/// whether its class alone keeps it awake.
fn pci_class(root: &SysRoot, name: &str) -> (&'static str, Option<&'static str>) {
    let class = root
        .read_string(&format!("{PCI_DIR}/{name}/class"))
        .ok()
        .and_then(|c| u32::from_str_radix(c.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0);
    let (base, sub) = (class >> 16, (class >> 8) & 0xff);
    let what = match (base, sub) {
        (0x01, _) => "storage",
        (0x02, _) => "network",
        (0x03, _) => "graphics",
        (0x04, _) => "multimedia",
        (0x06, _) => "bridge",
        (0x07, _) => "communication",
        (0x08, _) => "system",
        (0x0c, 0x03) => "usb_controller",
        (0x0c, _) => "serial_bus",
        (0x0d, _) => "wireless",
        (0x11, _) => "signal",
        _ => "other",
    };
    (what, (base == 0x03).then_some("graphics"))
}

fn model_key(root: &SysRoot, bus: &str, dir: &str, vendor: &str, product: &str) -> Option<String> {
    let read = |f: &str| {
        root.read_string(&format!("{dir}/{f}"))
            .ok()
            .map(|v| v.trim().trim_start_matches("0x").to_ascii_lowercase())
            .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    Some(format!("{bus}:{}:{}", read(vendor)?, read(product)?))
}

/// Every USB device and PCI function with a `power/control` file.
pub fn scan(root: &SysRoot) -> Vec<Device> {
    let mut out = Vec::new();
    let mut usb = root.list_dir(USB_DIR);
    usb.sort();
    for name in usb {
        if name.contains(':') {
            continue; // an interface, not a device
        }
        let path = format!("{USB_DIR}/{name}");
        let Ok(control) = root.read_string(&format!("{path}/power/control")) else {
            continue;
        };
        out.push(Device {
            kind: Kind::Usb,
            id: format!("usb:{name}"),
            name: usb_name(root, &name),
            what: "usb_device",
            address: name.clone(),
            model: model_key(root, "usb", &path, "idVendor", "idProduct"),
            control: control.trim().to_string(),
            excluded: usb_excluded(root, &name),
            path,
        });
    }
    let mut pci = root.list_dir(PCI_DIR);
    pci.sort();
    for name in pci {
        let path = format!("{PCI_DIR}/{name}");
        let Ok(control) = root.read_string(&format!("{path}/power/control")) else {
            continue;
        };
        let (what, by_class) = pci_class(root, &name);
        let by_driver = driver(root, &path).and_then(|d| {
            PCI_EXCLUDED_DRIVERS
                .iter()
                .find(|(k, _)| *k == d)
                .map(|(_, why)| *why)
        });
        out.push(Device {
            kind: Kind::Pci,
            id: format!("pci:{name}"),
            name: None,
            what,
            address: name.clone(),
            model: model_key(root, "pci", &path, "vendor", "device"),
            control: control.trim().to_string(),
            excluded: by_class.or(by_driver),
            path,
        });
    }
    out
}

/// The device directories present now, to notice plugging and unplugging
/// cheaply (two directory listings).
pub fn listing(root: &SysRoot) -> (Vec<String>, Vec<String>) {
    let mut u = root.list_dir(USB_DIR);
    let mut p = root.list_dir(PCI_DIR);
    u.sort();
    p.sort();
    (u, p)
}

/// Whether `path` is a device directory this module manages (used by the
/// engine to refuse anything else).
pub fn is_device_path(path: &str) -> bool {
    if path.contains("..") {
        return false;
    }
    [USB_DIR, PCI_DIR].iter().any(|d| {
        path.strip_prefix(d)
            .and_then(|r| r.strip_prefix('/'))
            .is_some_and(|name| !name.is_empty() && !name.contains('/'))
    })
}

/// Test helpers: build devices in a fixture tree.
#[doc(hidden)]
pub mod fixture {
    use std::fs;
    use std::path::Path;

    /// A USB device with optional interfaces (class, driver).
    pub fn usb(root: &Path, name: &str, product: &str, control: &str, ifaces: &[(&str, &str)]) {
        let dir = root.join(format!("sys/bus/usb/devices/{name}"));
        fs::create_dir_all(dir.join("power")).unwrap();
        fs::write(dir.join("power/control"), format!("{control}\n")).unwrap();
        fs::write(dir.join("product"), format!("{product}\n")).unwrap();
        fs::write(dir.join("bDeviceClass"), "00\n").unwrap();
        fs::write(dir.join("idVendor"), "1234\n").unwrap();
        fs::write(
            dir.join("idProduct"),
            format!(
                "{:04x}\n",
                name.len() * 7 + name.as_bytes()[name.len() - 1] as usize
            ),
        )
        .unwrap();
        for (i, (class, drv)) in ifaces.iter().enumerate() {
            let idir = root.join(format!("sys/bus/usb/devices/{name}:1.{i}"));
            fs::create_dir_all(&idir).unwrap();
            fs::write(idir.join("bInterfaceClass"), format!("{class}\n")).unwrap();
            if !drv.is_empty() {
                let target = root.join(format!("sys/bus/usb/drivers/{drv}"));
                fs::create_dir_all(&target).unwrap();
                let _ = std::os::unix::fs::symlink(&target, idir.join("driver"));
            }
        }
    }

    /// A PCI function with a class code and optional driver.
    pub fn pci(root: &Path, name: &str, class: &str, control: &str, drv: &str) {
        let dir = root.join(format!("sys/bus/pci/devices/{name}"));
        fs::create_dir_all(dir.join("power")).unwrap();
        fs::write(dir.join("power/control"), format!("{control}\n")).unwrap();
        fs::write(dir.join("class"), format!("{class}\n")).unwrap();
        if !drv.is_empty() {
            let target = root.join(format!("sys/bus/pci/drivers/{drv}"));
            fs::create_dir_all(&target).unwrap();
            let _ = std::os::unix::fs::symlink(&target, dir.join("driver"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_devices_and_keeps_sensitive_ones_awake() {
        let dir = std::env::temp_dir().join(format!("dynamis-devices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        fixture::usb(&dir, "1-1", "Webcam", "on", &[("0e", "uvcvideo")]);
        fixture::usb(&dir, "1-2", "Mouse", "on", &[("03", "usbhid")]);
        fixture::usb(&dir, "1-3", "Bluetooth", "on", &[("e0", "btusb")]);
        fixture::usb(&dir, "1-4", "Modem", "on", &[("ff", "qmi_wwan")]);
        fixture::usb(
            &dir,
            "usb1",
            "xHCI Host Controller",
            "auto",
            &[("09", "hub")],
        );
        fixture::pci(&dir, "0000:00:02.0", "0x030000", "on", "i915");
        fixture::pci(&dir, "0000:00:14.0", "0x0c0330", "on", "xhci_hcd");
        fixture::pci(&dir, "0000:00:16.0", "0x078000", "on", "mei_me");
        fixture::pci(&dir, "0000:01:00.0", "0x028000", "on", "rtw_8821ce");
        let d = scan(&SysRoot::at(&dir));
        let get = |id: &str| d.iter().find(|x| x.id == id).unwrap();
        assert_eq!(d.len(), 9);
        assert_eq!(get("usb:1-1").excluded, None);
        assert_eq!(get("usb:1-1").name.as_deref(), Some("Webcam"));
        assert!(get("usb:1-1")
            .model
            .as_deref()
            .unwrap()
            .starts_with("usb:1234:"));
        assert_eq!(
            get("pci:0000:00:14.0").model,
            None,
            "no ids in this fixture"
        );
        assert_eq!(
            get("usb:usb1").name.as_deref(),
            Some("xHCI Host Controller")
        );
        assert_eq!(get("usb:1-2").excluded, Some("input"));
        assert_eq!(get("usb:1-3").excluded, Some("bluetooth"));
        assert_eq!(get("usb:1-4").excluded, Some("modem"));
        assert_eq!(get("usb:usb1").control, "auto");
        assert_eq!(get("pci:0000:00:02.0").excluded, Some("graphics"));
        assert_eq!(get("pci:0000:00:14.0").what, "usb_controller");
        assert_eq!(get("pci:0000:00:14.0").excluded, None);
        assert!(get("pci:0000:00:16.0").excluded.is_some());
        assert_eq!(get("pci:0000:01:00.0").what, "network");
        assert_eq!(get("pci:0000:01:00.0").address, "0000:01:00.0");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn only_device_directories_are_paths() {
        assert!(is_device_path("/sys/bus/usb/devices/1-2"));
        assert!(is_device_path("/sys/bus/pci/devices/0000:00:14.0"));
        assert!(!is_device_path("/sys/bus/usb/devices/"));
        assert!(!is_device_path("/sys/bus/usb/devices/1-2/power"));
        assert!(!is_device_path("/sys/bus/usb/devices/../../power"));
        assert!(!is_device_path("/sys/class/net/wlan0"));
    }
}
