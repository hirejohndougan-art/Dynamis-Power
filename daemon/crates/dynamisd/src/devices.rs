//! Letting idle USB devices and internal hardware sleep, on battery.
//!
//! Two switches, both off by default (TLP turns these on by default; Dynamis
//! Power asks first). When one is on, the laptop is on battery and the mode
//! is not Faster, every device of that kind that is not excluded
//! (`dynamis_core::devices`) is set to `auto`; plugged in, in Faster, or
//! with the switch off, each goes back to its own value. Devices plugged in
//! later are handled at the next reading (the device directories are listed
//! on each reading, which is cheap). Every write goes through the engine,
//! so it is journaled, refused in safe mode or when blocked for this laptop,
//! and put back by Restore.

use std::path::Path;

use dynamis_core::devices::{self, Kind};
use dynamis_core::engine::EngineError;
use dynamis_core::linksleep;
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{open_engine, write_atomic};

const FILE: &str = "devices-policy.json";
const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DevicePolicy {
    /// Let idle USB devices sleep (not input, sound, printers, phones,
    /// Bluetooth or mobile broadband).
    #[serde(default)]
    pub usb: bool,
    /// Let idle internal (PCI) hardware sleep (not graphics).
    #[serde(default)]
    pub pci: bool,
    /// Device models the person keeps awake (`usb:046d:c52b`), for devices
    /// that misbehave asleep but are not caught by class or driver.
    #[serde(default)]
    pub never: Vec<String>,
    /// Deeper link sleep (PCIe L1 substates) on battery. Experimental; the
    /// app asks for an explicit yes before turning it on.
    #[serde(default)]
    pub deeper_link_sleep: bool,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    v: u32,
    #[serde(flatten)]
    policy: DevicePolicy,
}

#[derive(Default)]
pub struct DevicesState {
    pub policy: DevicePolicy,
    /// Device directories at the last application.
    listing: Option<(Vec<String>, Vec<String>)>,
    /// Whether saving was wanted at the last application.
    last_want: Option<bool>,
    dirty: bool,
}

impl DevicesState {
    pub fn load(dir: &Path) -> DevicesState {
        let policy = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
            .map(|s| s.policy)
            .unwrap_or_default();
        DevicesState {
            policy,
            dirty: true,
            ..DevicesState::default()
        }
    }

    pub fn enabled(&self) -> bool {
        self.policy.usb || self.policy.pci || self.policy.deeper_link_sleep
    }

    /// Readings are needed while a switch is on, to follow the charger and
    /// devices being plugged in.
    pub fn needs_ticks(&self) -> bool {
        self.enabled()
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn set_policy(
        &mut self,
        p: DevicePolicy,
        root: &SysRoot,
        dir: &Path,
        want_saving: bool,
    ) -> Result<(), String> {
        self.policy = p;
        self.policy.never.sort();
        self.policy.never.dedup();
        write_atomic(
            &dir.join(FILE),
            &serde_json::to_string_pretty(&Saved {
                v: VERSION,
                policy: self.policy.clone(),
            })
            .map_err(|e| e.to_string())?,
        )?;
        self.dirty = true;
        self.tick(root, dir, want_saving);
        Ok(())
    }

    /// Switch both off (Restore); the engine puts the devices back. The
    /// devices the person keeps awake stay on their list.
    pub fn clear(&mut self, dir: &Path) {
        self.policy = DevicePolicy {
            never: std::mem::take(&mut self.policy.never),
            ..DevicePolicy::default()
        };
        if self.policy.never.is_empty() {
            let _ = std::fs::remove_file(dir.join(FILE));
        } else if let Ok(t) = serde_json::to_string_pretty(&Saved {
            v: VERSION,
            policy: self.policy.clone(),
        }) {
            let _ = write_atomic(&dir.join(FILE), &t);
        }
        self.dirty = true;
        self.listing = None;
    }

    /// One reading: apply again if devices came or went, the charger or
    /// mode changed, or something asked for it.
    pub fn tick(&mut self, root: &SysRoot, dir: &Path, want_saving: bool) {
        let listing = devices::listing(root);
        if !self.dirty
            && self.listing.as_ref() == Some(&listing)
            && self.last_want == Some(want_saving)
        {
            return;
        }
        self.dirty = false;
        self.listing = Some(listing);
        self.last_want = Some(want_saving);
        self.apply(root, dir, want_saving);
    }

    fn apply(&self, root: &SysRoot, dir: &Path, want_saving: bool) {
        let mut engine = match open_engine(root, dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("dynamisd: devices: {e}");
                return;
            }
        };
        let touched = engine.touched_devices();
        let links = engine.touched_links();
        for d in devices::scan(root) {
            if d.kind == Kind::Pci {
                let kept_awake = d
                    .model
                    .as_ref()
                    .is_some_and(|m| self.policy.never.contains(m));
                let has = !linksleep::files(root, &d.path).is_empty();
                let result = if has
                    && self.policy.deeper_link_sleep
                    && want_saving
                    && linksleep::excluded(&d).is_none()
                    && !kept_awake
                {
                    engine.set_link_sleep(&d.path, &d.id, "on battery: deeper link sleep")
                } else if links.contains(&d.path) {
                    let why = if !self.policy.deeper_link_sleep {
                        "switched off"
                    } else if kept_awake {
                        "you keep it awake"
                    } else {
                        "plugged in or Faster mode"
                    };
                    engine.reset_link_sleep(&d.path, &d.id, why)
                } else {
                    Ok(false)
                };
                match result {
                    Ok(_) | Err(EngineError::Blocked { .. }) => {}
                    Err(EngineError::SafeMode(_)) => return,
                    Err(e) => eprintln!("dynamisd: {} link sleep: {e}", d.id),
                }
            }
            let on = match d.kind {
                Kind::Usb => self.policy.usb,
                Kind::Pci => self.policy.pci,
            };
            let kept_awake = d
                .model
                .as_ref()
                .is_some_and(|m| self.policy.never.contains(m));
            let result = if on && want_saving && d.excluded.is_none() && !kept_awake {
                engine.set_device_power(
                    d.kind,
                    &d.path,
                    &d.id,
                    "auto",
                    "on battery: let it sleep when idle",
                )
            } else if touched.contains_key(&d.path) {
                let why = if !on {
                    "switched off"
                } else if kept_awake {
                    "you keep it awake"
                } else {
                    "plugged in or Faster mode"
                };
                engine.reset_device_power(&d.path, &d.id, why)
            } else {
                Ok(false)
            };
            match result {
                Ok(_) | Err(EngineError::Blocked { .. }) => {}
                Err(EngineError::SafeMode(_)) => return,
                Err(e) => eprintln!("dynamisd: {}: {e}", d.id),
            }
        }
    }

    /// For the app: the switches, and every device with whether it is
    /// managed, kept awake (and why), and its current state.
    pub fn json(&self, root: &SysRoot, dir: &Path) -> Value {
        let (touched, blocked, links) = open_engine(root, dir)
            .map(|e| (e.touched_devices(), e.blocked(), e.touched_links()))
            .unwrap_or_default();
        let mut found = devices::scan(root);
        devices::add_friendly_names(root, &mut found);
        let found_links = found
            .iter()
            .filter(|d| {
                linksleep::excluded(d).is_none() && !linksleep::files(root, &d.path).is_empty()
            })
            .count();
        let list: Vec<Value> = found
            .into_iter()
            .map(|d| {
                let files = linksleep::files(root, &d.path);
                json!({
                    "id": d.id,
                    "kind": d.kind,
                    "name": d.name,
                    "what": d.what,
                    "address": d.address,
                    "model": d.model,
                    "kept_awake": d.model.as_ref().is_some_and(|m| self.policy.never.contains(m)),
                    "sleeps": d.control == "auto",
                    "managed": touched.contains_key(&d.path),
                    "excluded": d.excluded,
                    // Deeper link sleep: null where the link has none.
                    "link_sleep": (!files.is_empty()).then(|| json!({
                        "on": linksleep::all_on(&files),
                        "managed": links.contains(&d.path),
                        "excluded": linksleep::excluded(&d),
                    })),
                })
            })
            .collect();
        json!({
            "policy": self.policy,
            "blocked": {
                "usb": blocked.get(Kind::Usb.setting_id()),
                "pci": blocked.get(Kind::Pci.setting_id()),
                "links": blocked.get(linksleep::ID).or(blocked.get("pcie.aspm")),
            },
            "link_sleep_capable": found_links,
            "devices": list,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dynamis_core::deepidle::fixture::link;
    use dynamis_core::devices::fixture::pci;

    #[test]
    fn deeper_link_sleep_on_battery_only_and_never_for_graphics() {
        let dir = std::env::temp_dir().join(format!("dynamis-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let rootdir = dir.join("root");
        pci(&rootdir, "0000:01:00.0", "0x010802", "on", "nvme");
        link(
            &rootdir,
            "0000:01:00.0",
            &[("l1_1_aspm", "0"), ("l1_2_aspm", "0")],
        );
        pci(&rootdir, "0000:02:00.0", "0x030000", "on", "amdgpu");
        link(&rootdir, "0000:02:00.0", &[("l1_2_aspm", "0")]);
        let state = dir.join("state");
        std::fs::create_dir_all(&state).unwrap();
        let root = SysRoot::at(&rootdir);
        let read = |dev: &str, f: &str| {
            std::fs::read_to_string(rootdir.join(format!("sys/bus/pci/devices/{dev}/link/{f}")))
                .unwrap()
                .trim()
                .to_string()
        };
        let mut d = DevicesState::load(&state);
        d.set_policy(
            DevicePolicy {
                deeper_link_sleep: true,
                ..DevicePolicy::default()
            },
            &root,
            &state,
            false,
        )
        .unwrap();
        assert_eq!(
            read("0000:01:00.0", "l1_2_aspm"),
            "0",
            "plugged in: left alone"
        );
        d.tick(&root, &state, true);
        assert_eq!(read("0000:01:00.0", "l1_1_aspm"), "1");
        assert_eq!(read("0000:01:00.0", "l1_2_aspm"), "1");
        assert_eq!(
            read("0000:02:00.0", "l1_2_aspm"),
            "0",
            "graphics left alone"
        );
        let j = d.json(&root, &state);
        assert_eq!(j["link_sleep_capable"], 1);
        d.tick(&root, &state, false);
        assert_eq!(read("0000:01:00.0", "l1_1_aspm"), "0", "plugged in: back");
        assert_eq!(read("0000:01:00.0", "l1_2_aspm"), "0");
        // Restore puts it back too.
        d.tick(&root, &state, true);
        open_engine(&root, &state).unwrap().restore().unwrap();
        assert_eq!(read("0000:01:00.0", "l1_2_aspm"), "0");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
