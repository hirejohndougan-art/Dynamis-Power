//! "Something stopped working" (everyday problem §1), in two steps.
//!
//! 1. **Undo the last hour.** Every change Dynamis Power made in the last
//!    hour is put back (newest first, journaled) and held: not changed again
//!    until the person answers, or for twelve hours. Learning waits too.
//! 2. **What stopped working?** The person picks Wi-Fi, sound, the screen…
//!    The settings that could cause that kind of trouble ([`trouble`]), if
//!    Dynamis Power changed them (in the last hour or earlier), are put back
//!    to their original values and blocked on this laptop. If none of them
//!    were changed, nothing is blocked and the app says the trouble is
//!    probably not from Dynamis Power. "Something else" blocks what step 1
//!    undid. The answer is journaled, so *Report a problem* includes it.
//!
//! [`trouble`]: dynamis_core::trouble

use std::path::Path;

use dynamis_core::devices::{PCI_DIR, USB_DIR};
use dynamis_core::engine::Undone;
use dynamis_core::{settings, trouble, SysRoot};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{open_engine, write_atomic};

const FILE: &str = "trouble.json";
const VERSION: u32 = 1;
/// What is undone.
const WINDOW_S: u64 = 3600;
/// How long what was undone is held without an answer.
const HOLD_S: u64 = 12 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Open {
    v: u32,
    at: u64,
    until: u64,
    undone: Vec<Undone>,
}

#[derive(Default)]
pub struct Trouble {
    open: Option<Open>,
}

/// The ids to hold or block for what was undone (groups for devices and
/// radios).
fn group_ids(undone: &[Undone]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for u in undone {
        let add: Vec<&str> = match u.id.as_str() {
            "devices" => vec!["usb.autosuspend", "pci.runtime_pm"],
            id if dynamis_core::radios::valid_id(id) => vec!["radios"],
            id => vec![id],
        };
        for a in add {
            if !ids.iter().any(|i| i == a) {
                ids.push(a.to_string());
            }
        }
    }
    ids
}

/// The words used in the journal for each kind.
fn what(kind: &str) -> &'static str {
    match kind {
        "wifi" => "Wi-Fi",
        "bluetooth" => "Bluetooth",
        "sound" => "sound",
        "input" => "the mouse, keyboard or touchpad",
        "screen" => "the screen",
        "storage" => "a disk or USB drive",
        "sleep" => "sleep or waking up",
        "freeze" => "the laptop (it froze)",
        _ => "something",
    }
}

impl Trouble {
    pub fn load(dir: &Path, now: u64) -> Trouble {
        let open = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Open>(&t).ok())
            .filter(|o| now < o.until);
        Trouble { open }
    }

    /// Waiting for the person to say what stopped working.
    pub fn waiting(&self, now: u64) -> bool {
        self.open.as_ref().is_some_and(|o| now < o.until)
    }

    /// Step 1: undo the last hour and hold it.
    pub fn start(&mut self, root: &SysRoot, dir: &Path, now: u64) -> Result<Value, String> {
        let mut engine = open_engine(root, dir)?;
        let undone = engine
            .undo_since(now.saturating_sub(WINDOW_S), "something stopped working")
            .map_err(|e| e.to_string())?;
        let until = now + HOLD_S;
        engine
            .hold(
                &group_ids(&undone),
                until,
                "held until you say what stopped working",
            )
            .map_err(|e| e.to_string())?;
        engine
            .note(
                "problem",
                "start",
                &format!(
                    "something stopped working: {} change(s) from the last hour undone",
                    undone.len()
                ),
            )
            .map_err(|e| e.to_string())?;
        let open = Open {
            v: VERSION,
            at: now,
            until,
            undone,
        };
        write_atomic(
            &dir.join(FILE),
            &serde_json::to_string(&open).map_err(|e| e.to_string())?,
        )?;
        self.open = Some(open);
        Ok(self.json(now))
    }

    /// Step 2: what stopped working. `none` ends it without blocking
    /// anything (clicked by mistake, or it works again by itself).
    pub fn answer(&mut self, root: &SysRoot, dir: &Path, kind: &str) -> Result<Value, String> {
        if kind != "none" && !trouble::KINDS.contains(&kind) {
            return Err(format!(
                "{kind} is not one of {}",
                trouble::KINDS.join(", ")
            ));
        }
        let undone = self
            .open
            .as_ref()
            .map(|o| group_ids(&o.undone))
            .unwrap_or_default();
        let mut engine = open_engine(root, dir)?;
        let mut blocked: Vec<String> = Vec::new();
        if kind != "none" {
            let suspects: Vec<String> = if kind == "other" {
                undone.clone()
            } else {
                trouble::suspects(kind)
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            };
            let baseline = engine.baseline();
            let devices = engine.touched_devices();
            let radios = engine.touched_radios();
            let reason = format!(
                "you said {} stopped working after it was changed",
                what(kind)
            );
            for id in suspects {
                let changed_now = match id.as_str() {
                    "usb.autosuspend" => devices.keys().any(|d| d.starts_with(USB_DIR)),
                    "pci.runtime_pm" => devices.keys().any(|d| d.starts_with(PCI_DIR)),
                    "radios" => !radios.is_empty(),
                    _ => baseline.get(&id).is_some_and(|orig| {
                        settings::by_id(&id)
                            .and_then(|s| s.read(root).ok())
                            .is_some_and(|cur| &cur != orig)
                    }),
                };
                if !changed_now && !undone.contains(&id) {
                    continue;
                }
                // Back to the laptop's own value first, then blocked.
                let put_back = match id.as_str() {
                    "usb.autosuspend" | "pci.runtime_pm" => {
                        engine.restore_device_power().map(|_| ())
                    }
                    "radios" => engine.restore_radios().map(|_| ()),
                    _ => engine.restore_one(&id).map(|_| ()),
                };
                if let Err(e) = put_back {
                    eprintln!("dynamisd: something stopped working: {id}: {e}");
                }
                if !engine.blocked().contains_key(&id) {
                    engine
                        .block_by_person(&id, &reason)
                        .map_err(|e| e.to_string())?;
                }
                blocked.push(id);
            }
        }
        engine.end_hold().map_err(|e| e.to_string())?;
        let note = match (kind, blocked.is_empty()) {
            ("none", _) => "nothing stopped working after all".to_string(),
            (_, true) => format!(
                "{} stopped working; nothing Dynamis Power changed could cause it",
                what(kind)
            ),
            (_, false) => format!(
                "{} stopped working; blocked: {}",
                what(kind),
                blocked.join(", ")
            ),
        };
        engine
            .note("problem", kind, &note)
            .map_err(|e| e.to_string())?;
        self.open = None;
        let _ = std::fs::remove_file(dir.join(FILE));
        Ok(json!({
            "kind": kind,
            "blocked": blocked
                .iter()
                .map(|id| json!({"id": id, "label": trouble::label(id)}))
                .collect::<Vec<_>>(),
        }))
    }

    /// Forget an open question (Restore puts everything back anyway).
    pub fn clear(&mut self, root: &SysRoot, dir: &Path) {
        if self.open.take().is_some() {
            if let Ok(mut e) = open_engine(root, dir) {
                let _ = e.end_hold();
            }
        }
        let _ = std::fs::remove_file(dir.join(FILE));
    }

    /// For `Status`: the open question, or null.
    pub fn json(&self, now: u64) -> Value {
        match &self.open {
            Some(o) if now < o.until => json!({
                "at": o.at,
                "until": o.until,
                "undone": o.undone.iter().map(|u| json!({
                    "id": u.id,
                    "label": trouble::label(&u.id),
                    "to": u.to,
                    "ok": u.ok,
                })).collect::<Vec<_>>(),
                "kinds": trouble::KINDS,
            }),
            _ => Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_for_devices_and_radios() {
        let u = |id: &str| Undone {
            id: id.into(),
            to: "x".into(),
            ok: true,
        };
        assert_eq!(
            group_ids(&[u("pcie.aspm"), u("devices"), u("rfkill0"), u("rfkill1")]),
            ["pcie.aspm", "usb.autosuspend", "pci.runtime_pm", "radios"]
        );
    }
}
