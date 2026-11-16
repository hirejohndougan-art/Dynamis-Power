//! Settings a person chose by hand, kept across restarts and sleep.
//!
//! Every change Dynamis Power makes is runtime-only (safety rule 3): a
//! reboot puts the kernel's own values back. That is right for automatic
//! changes, but a person who sets a charge limit expects it to stay. So the
//! service keeps a short list, in its own state directory (never in system
//! configuration), of settings set by hand: the battery's charge limit and
//! start always, and any Advanced setting with "Keep after restart" ticked.
//! It applies them again, through the engine as usual, when it starts and
//! after the laptop wakes from sleep (some firmware resets them then).
//!
//! A kept setting is **on probation** until it has been in force for an
//! hour. After an unclean shutdown (a freeze, crash or power cut), settings
//! still on probation are not applied again, so a setting that freezes the
//! laptop cannot come back by itself at every boot. Safe mode applies
//! nothing (the engine refuses).

use std::collections::BTreeMap;
use std::path::Path;

use dynamis_core::engine::{Outcome, Request};
use dynamis_core::sensors::power;
use dynamis_core::settings;
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{open_engine, write_atomic};

const FILE: &str = "kept.json";
const VERSION: u32 = 1;
/// In force this long, and it is trusted after an unclean shutdown.
const PROBATION_S: f64 = 3600.0;
/// Settings kept whenever they are set by hand.
pub const ALWAYS_KEPT: &[&str] = &["battery.charge_limit", "battery.charge_start"];
/// Readings above the limit while charging before we say it is ignored.
const IGNORED_AFTER: u32 = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub value: String,
    /// Unix time it was set.
    pub set_at: f64,
    /// In force for an hour: applied again even after an unclean shutdown.
    #[serde(default)]
    pub trusted: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    v: u32,
    #[serde(default)]
    settings: BTreeMap<String, Kept>,
    #[serde(default)]
    charge_limit_ignored: bool,
}

#[derive(Default)]
pub struct KeptState {
    pub settings: BTreeMap<String, Kept>,
    /// The firmware let the battery charge past the limit.
    pub charge_limit_ignored: bool,
    /// Unix time this run of the service started (probation counts from
    /// the later of this and `set_at`).
    started: f64,
    over_limit: u32,
    /// What the last application left out, for the app.
    pub held_back: Vec<String>,
}

impl KeptState {
    pub fn load(dir: &Path, now: f64) -> KeptState {
        let s: Saved = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        KeptState {
            settings: s.settings,
            charge_limit_ignored: s.charge_limit_ignored,
            started: now,
            ..KeptState::default()
        }
    }

    fn save(&self, dir: &Path) -> Result<(), String> {
        let s = Saved {
            v: VERSION,
            settings: self.settings.clone(),
            charge_limit_ignored: self.charge_limit_ignored,
        };
        write_atomic(
            &dir.join(FILE),
            &serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?,
        )
    }

    pub fn keep(&mut self, id: &str, value: &str, now: f64, dir: &Path) -> Result<(), String> {
        self.settings.insert(
            id.to_string(),
            Kept {
                value: value.to_string(),
                set_at: now,
                trusted: false,
            },
        );
        if id == "battery.charge_limit" {
            self.charge_limit_ignored = false;
            self.over_limit = 0;
        }
        self.save(dir)
    }

    /// Stop keeping a setting. Returns whether it was kept.
    pub fn drop_setting(&mut self, id: &str, dir: &Path) -> Result<bool, String> {
        let had = self.settings.remove(id).is_some();
        if had {
            self.save(dir)?;
        }
        Ok(had)
    }

    pub fn clear(&mut self, dir: &Path) -> Result<(), String> {
        self.settings.clear();
        self.charge_limit_ignored = false;
        self.save(dir)
    }

    /// Apply every kept setting. After an unclean shutdown, settings still
    /// on probation are held back. Returns what was applied.
    pub fn apply_all(
        &mut self,
        root: &SysRoot,
        dir: &Path,
        after_unclean: bool,
        why: &str,
    ) -> usize {
        self.held_back.clear();
        if self.settings.is_empty() {
            return 0;
        }
        let mut engine = match open_engine(root, dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("dynamisd: kept settings: {e}");
                return 0;
            }
        };
        if engine.safe_mode().is_some() || engine.dry_run() {
            return 0;
        }
        let mut applied = 0;
        for (id, k) in &self.settings {
            if after_unclean && !k.trusted {
                eprintln!(
                    "dynamisd: not applying {id} = {} again: it was set shortly before an unclean shutdown",
                    k.value
                );
                self.held_back.push(id.clone());
                continue;
            }
            match engine.apply(&Request {
                id,
                value: &k.value,
                allow_risky: true,
                trial_secs: None,
                reason: &format!("kept setting, applied again {why}"),
            }) {
                Ok(Outcome::Changed { .. }) => applied += 1,
                Ok(_) => {}
                Err(e) => eprintln!("dynamisd: kept setting {id}: {e}"),
            }
        }
        applied
    }

    /// One reading: probation, and whether the firmware ignores the charge
    /// limit (the battery keeps charging past it).
    pub fn tick(&mut self, root: &SysRoot, dir: &Path, now: f64) {
        let mut changed = false;
        for k in self.settings.values_mut() {
            if !k.trusted && now - k.set_at.max(self.started) >= PROBATION_S {
                k.trusted = true;
                changed = true;
            }
        }
        if !self.charge_limit_ignored {
            let limit = settings::by_id("battery.charge_limit")
                .and_then(|s| s.read(root).ok())
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|l| *l < 100.0);
            let ps = power::read(root);
            let bat = ps.batteries.first();
            let charging = bat.and_then(|b| b.status.as_deref()) == Some("Charging");
            let pct = bat.and_then(|b| b.capacity_pct).map(f64::from);
            match (limit, pct) {
                (Some(l), Some(p)) if charging && p > l + 2.0 => self.over_limit += 1,
                _ => self.over_limit = 0,
            }
            if self.over_limit >= IGNORED_AFTER {
                eprintln!(
                    "dynamisd: the battery charges past its charge limit: the firmware ignores it"
                );
                self.charge_limit_ignored = true;
                changed = true;
            }
        }
        if changed {
            if let Err(e) = self.save(dir) {
                eprintln!("dynamisd: {e}");
            }
        }
    }

    pub fn json(&self) -> Value {
        json!({
            "settings": self.settings.iter().map(|(id, k)| json!({
                "id": id,
                "label": settings::by_id(id).map(|s| s.label()).unwrap_or(""),
                "value": k.value,
                "trusted": k.trusted,
            })).collect::<Vec<_>>(),
            "held_back": self.held_back,
            "charge_limit_ignored": self.charge_limit_ignored,
        })
    }
}
