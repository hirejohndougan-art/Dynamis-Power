//! Switching radios off by rule (TLP's `DEVICES_TO_DISABLE_ON_BAT`,
//! `_ON_LAN_CONNECT` and `_ON_STARTUP`). All off until the person picks a
//! rule.
//!
//! - **On battery:** the chosen kinds (Bluetooth, mobile broadband, Wi-Fi)
//!   are switched off when the charger is unplugged and back to how they
//!   were when it is plugged in again.
//! - **Cable in:** Wi-Fi is switched off while a network cable is plugged
//!   in and back on when it is unplugged.
//! - **At startup:** the chosen kinds are switched off once when the
//!   service starts.
//!
//! Rules act only when their situation changes, never in between: a radio
//! the person switches back on stays on until the next change (no fight).
//! Every switch goes through the engine (journaled, put back by Restore).

use std::collections::BTreeSet;
use std::path::Path;

use dynamis_core::engine::EngineError;
use dynamis_core::radios;
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{open_engine, write_atomic};

const FILE: &str = "radios-policy.json";
const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RadioPolicy {
    /// Kinds switched off on battery (`bluetooth`, `wlan`, `wwan`).
    #[serde(default)]
    pub off_on_battery: Vec<String>,
    /// Wi-Fi off while a network cable is plugged in.
    #[serde(default)]
    pub wifi_off_when_wired: bool,
    /// Kinds switched off once when the service starts.
    #[serde(default)]
    pub off_at_startup: Vec<String>,
}

impl RadioPolicy {
    fn check(&mut self) -> Result<(), String> {
        for k in self.off_on_battery.iter().chain(&self.off_at_startup) {
            if !radios::KINDS.contains(&k.as_str()) {
                return Err(format!("{k} is not bluetooth, wlan or wwan"));
            }
        }
        self.off_on_battery.sort();
        self.off_on_battery.dedup();
        self.off_at_startup.sort();
        self.off_at_startup.dedup();
        Ok(())
    }

    pub fn enabled(&self) -> bool {
        !self.off_on_battery.is_empty() || self.wifi_off_when_wired
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    v: u32,
    #[serde(flatten)]
    policy: RadioPolicy,
}

#[derive(Default)]
pub struct RadiosState {
    pub policy: RadioPolicy,
    /// The kinds the rules wanted off at the last look; `None` before the
    /// first look.
    last: Option<BTreeSet<String>>,
}

impl RadiosState {
    pub fn load(dir: &Path) -> RadiosState {
        let policy = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
            .map(|s| s.policy)
            .unwrap_or_default();
        RadiosState { policy, last: None }
    }

    pub fn needs_ticks(&self) -> bool {
        self.policy.enabled()
    }

    /// At service start: the startup rule, once.
    pub fn at_startup(&self, root: &SysRoot, dir: &Path) {
        if self.policy.off_at_startup.is_empty() {
            return;
        }
        let kinds: BTreeSet<String> = self.policy.off_at_startup.iter().cloned().collect();
        switch(
            root,
            dir,
            &kinds,
            true,
            "switched off when the computer starts",
        );
    }

    pub fn set_policy(
        &mut self,
        mut p: RadioPolicy,
        root: &SysRoot,
        dir: &Path,
        on_ac: bool,
    ) -> Result<(), String> {
        p.check()?;
        write_atomic(
            &dir.join(FILE),
            &serde_json::to_string_pretty(&Saved {
                v: VERSION,
                policy: p.clone(),
            })
            .map_err(|e| e.to_string())?,
        )?;
        self.policy = p;
        // Apply the new rules to the situation now.
        self.last = Some(BTreeSet::new());
        self.tick(root, dir, on_ac);
        Ok(())
    }

    /// Forget the rules (Restore); the engine puts the radios back.
    pub fn clear(&mut self, dir: &Path) {
        self.policy = RadioPolicy::default();
        self.last = None;
        let _ = std::fs::remove_file(dir.join(FILE));
    }

    /// One reading: act only if what the rules want changed.
    pub fn tick(&mut self, root: &SysRoot, dir: &Path, on_ac: bool) {
        let mut want: BTreeSet<String> = BTreeSet::new();
        if !on_ac {
            want.extend(self.policy.off_on_battery.iter().cloned());
        }
        if self.policy.wifi_off_when_wired && radios::wired_up(root) {
            want.insert("wlan".into());
        }
        let Some(last) = self.last.replace(want.clone()) else {
            // First look: take the situation as it is, and apply it.
            switch(root, dir, &want, true, &reason(&want, on_ac));
            return;
        };
        if last == want {
            return;
        }
        let newly: BTreeSet<String> = want.difference(&last).cloned().collect();
        let ended: BTreeSet<String> = last.difference(&want).cloned().collect();
        if !newly.is_empty() {
            switch(root, dir, &newly, true, &reason(&newly, on_ac));
        }
        if !ended.is_empty() {
            switch(root, dir, &ended, false, "the rule no longer applies");
        }
    }

    pub fn json(&self, root: &SysRoot, dir: &Path) -> Value {
        let touched = open_engine(root, dir)
            .map(|e| e.touched_radios())
            .unwrap_or_default();
        let list: Vec<Value> = radios::scan(root)
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "kind": r.kind,
                    "name": r.name,
                    "off": r.soft_off || r.hard_off,
                    "hard_off": r.hard_off,
                    "managed": touched.contains_key(&r.id),
                })
            })
            .collect();
        json!({ "policy": self.policy, "radios": list, "wired": radios::wired_up(root) })
    }
}

fn reason(kinds: &BTreeSet<String>, on_ac: bool) -> String {
    if kinds.contains("wlan") && !on_ac {
        "on battery or a network cable is plugged in".into()
    } else if kinds.contains("wlan") {
        "a network cable is plugged in".into()
    } else {
        "on battery".into()
    }
}

/// Switch every radio of these kinds off, or back to how it was.
fn switch(root: &SysRoot, dir: &Path, kinds: &BTreeSet<String>, off: bool, why: &str) {
    if kinds.is_empty() {
        return;
    }
    let mut engine = match open_engine(root, dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("dynamisd: radios: {e}");
            return;
        }
    };
    for r in radios::scan(root)
        .into_iter()
        .filter(|r| kinds.contains(&r.kind))
    {
        let result = if off {
            engine.set_radio(&r.id, true, why)
        } else {
            engine.reset_radio(&r.id, why)
        };
        match result {
            Ok(_) | Err(EngineError::Blocked { .. }) => {}
            Err(EngineError::SafeMode(_)) => return,
            Err(e) => eprintln!("dynamisd: radio {}: {e}", r.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dynamis_core::radios::fixture;

    fn sandbox(name: &str) -> (std::path::PathBuf, SysRoot, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("dynamis-radios-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("root");
        fixture::add(&root, "rfkill0", "bluetooth", "hci0", false);
        fixture::add(&root, "rfkill1", "wlan", "phy0", false);
        fixture::wired(&root, "enp2s0", false);
        let state = dir.join("state");
        std::fs::create_dir_all(&state).unwrap();
        (dir.clone(), SysRoot::at(root), state)
    }

    fn soft(dir: &Path, id: &str) -> String {
        std::fs::read_to_string(dir.join(format!("root/sys/class/rfkill/{id}/soft")))
            .unwrap()
            .trim()
            .to_string()
    }

    #[test]
    fn rules_act_on_changes_and_never_fight() {
        let (dir, root, state) = sandbox("rules");
        let mut r = RadiosState::load(&state);
        r.set_policy(
            RadioPolicy {
                off_on_battery: vec!["bluetooth".into()],
                wifi_off_when_wired: true,
                off_at_startup: vec![],
            },
            &root,
            &state,
            true,
        )
        .unwrap();
        assert_eq!(soft(&dir, "rfkill0"), "0", "plugged in: Bluetooth stays on");
        // Unplugged: Bluetooth off.
        r.tick(&root, &state, false);
        assert_eq!(soft(&dir, "rfkill0"), "1");
        // The person switches it back on: left alone until the next change.
        std::fs::write(dir.join("root/sys/class/rfkill/rfkill0/soft"), "0").unwrap();
        r.tick(&root, &state, false);
        assert_eq!(soft(&dir, "rfkill0"), "0");
        // Cable in: Wi-Fi off; cable out: back on.
        fixture::wired(&dir.join("root"), "enp2s0", true);
        r.tick(&root, &state, false);
        assert_eq!(soft(&dir, "rfkill1"), "1");
        fixture::wired(&dir.join("root"), "enp2s0", false);
        r.tick(&root, &state, false);
        assert_eq!(soft(&dir, "rfkill1"), "0");
        // Plugged in again: Bluetooth back as it was before the rule.
        std::fs::write(dir.join("root/sys/class/rfkill/rfkill0/soft"), "1").unwrap();
        r.tick(&root, &state, true);
        assert_eq!(soft(&dir, "rfkill0"), "0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn startup_rule_and_restore() {
        let (dir, root, state) = sandbox("startup");
        let mut r = RadiosState::load(&state);
        r.set_policy(
            RadioPolicy {
                off_on_battery: vec![],
                wifi_off_when_wired: false,
                off_at_startup: vec!["bluetooth".into()],
            },
            &root,
            &state,
            true,
        )
        .unwrap();
        let again = RadiosState::load(&state);
        again.at_startup(&root, &state);
        assert_eq!(soft(&dir, "rfkill0"), "1");
        open_engine(&root, &state).unwrap().restore().unwrap();
        assert_eq!(soft(&dir, "rfkill0"), "0", "Restore puts it back");
        assert!(RadioPolicy {
            off_on_battery: vec!["nfc".into()],
            ..RadioPolicy::default()
        }
        .check()
        .is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
