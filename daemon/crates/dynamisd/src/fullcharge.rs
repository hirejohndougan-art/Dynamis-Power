//! "Charge fully once": lift the charge limit until the battery is full,
//! then put it back (TLP's `fullcharge`). For a trip, or before checking the
//! battery's capacity.
//!
//! It ends when the battery is full, when the charger is unplugged after
//! being plugged in, after a day, or when the person stops it, changes the
//! limit by hand, or presses Restore. While it runs, charging that follows
//! habits waits, and the limit is set to 100 again if something (waking from
//! sleep, kept settings) put it back early. The limit to return to is kept
//! in the state directory, so a restart of the service carries on.

use std::path::Path;

use dynamis_core::engine::Request;
use dynamis_core::sensors::power;
use dynamis_core::settings::{self, Probe};
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{open_engine, write_atomic};

const FILE: &str = "full-charge.json";
const VERSION: u32 = 1;
const LIMIT: &str = "battery.charge_limit";
/// It gives up after this long.
const MAX_S: f64 = 86_400.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Active {
    /// The limit to put back.
    pub restore_to: String,
    /// Unix time it started.
    pub started: f64,
    /// The charger has been plugged in since it started.
    #[serde(default)]
    pub seen_ac: bool,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    v: u32,
    active: Active,
}

#[derive(Default)]
pub struct FullCharge {
    pub active: Option<Active>,
    /// Why it last ended, for the app.
    pub ended: Option<String>,
}

fn read_limit(root: &SysRoot) -> Option<String> {
    settings::by_id(LIMIT).and_then(|s| s.read(root).ok())
}

fn set_limit(root: &SysRoot, dir: &Path, value: &str, reason: &str) -> Result<(), String> {
    let mut e = open_engine(root, dir)?;
    e.apply(&Request {
        id: LIMIT,
        value,
        allow_risky: false,
        trial_secs: None,
        reason,
    })
    .map(|_| ())
    .map_err(|e| e.to_string())
}

impl FullCharge {
    pub fn load(dir: &Path) -> FullCharge {
        FullCharge {
            active: std::fs::read_to_string(dir.join(FILE))
                .ok()
                .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
                .map(|s| s.active),
            ended: None,
        }
    }

    fn save(&self, dir: &Path) -> Result<(), String> {
        match &self.active {
            Some(a) => write_atomic(
                &dir.join(FILE),
                &serde_json::to_string_pretty(&Saved {
                    v: VERSION,
                    active: a.clone(),
                })
                .map_err(|e| e.to_string())?,
            ),
            None => {
                let _ = std::fs::remove_file(dir.join(FILE));
                Ok(())
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Whether this battery has a limit that can be lifted.
    pub fn supported(root: &SysRoot) -> bool {
        settings::by_id(LIMIT).is_some_and(|s| matches!(s.probe(root), Probe::Supported { .. }))
    }

    pub fn start(
        &mut self,
        root: &SysRoot,
        dir: &Path,
        now: f64,
        on_ac: bool,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Ok(());
        }
        if !Self::supported(root) {
            return Err("this battery has no charge limit".into());
        }
        let limit = read_limit(root).ok_or("the charge limit cannot be read")?;
        if limit == "100" {
            return Err("the battery already charges fully".into());
        }
        set_limit(root, dir, "100", "charge fully once")?;
        self.active = Some(Active {
            restore_to: limit,
            started: now,
            seen_ac: on_ac,
        });
        self.ended = None;
        self.save(dir)
    }

    /// Stop and put the limit back.
    pub fn stop(&mut self, root: &SysRoot, dir: &Path, why: &str) -> Result<(), String> {
        let Some(a) = self.active.take() else {
            return Ok(());
        };
        self.ended = Some(why.to_string());
        self.save(dir)?;
        set_limit(
            root,
            dir,
            &a.restore_to,
            &format!("charge fully once ended: {why}"),
        )
    }

    /// Forget it without touching the limit (the person set the limit by
    /// hand, or Restore puts everything back itself).
    pub fn drop_it(&mut self, dir: &Path, why: &str) {
        if self.active.take().is_some() {
            self.ended = Some(why.to_string());
            if let Err(e) = self.save(dir) {
                eprintln!("dynamisd: {e}");
            }
        }
    }

    /// One reading.
    pub fn tick(&mut self, root: &SysRoot, dir: &Path, now: f64, on_ac: bool) {
        let Some(a) = self.active.as_mut() else {
            return;
        };
        if on_ac && !a.seen_ac {
            a.seen_ac = true;
            let _ = self.save(dir);
        }
        let Some(a) = self.active.as_ref() else {
            return;
        };
        let ps = power::read(root);
        let bat = ps.batteries.first();
        let pct = bat.and_then(|b| b.capacity_pct).unwrap_or(0);
        let status = bat.and_then(|b| b.status.clone()).unwrap_or_default();
        let end = if status == "Full"
            || (on_ac && (pct >= 99 || (status == "Not charging" && pct >= 95)))
        {
            Some("the battery is full")
        } else if !on_ac && a.seen_ac {
            Some("the charger was unplugged")
        } else if now - a.started >= MAX_S {
            Some("a day passed")
        } else {
            None
        };
        if let Some(why) = end {
            if let Err(e) = self.stop(root, dir, why) {
                eprintln!("dynamisd: charge fully once: {e}");
            }
        } else if read_limit(root).as_deref() != Some("100") {
            // Put back early (waking, kept settings, firmware): lift it again.
            if let Err(e) = set_limit(root, dir, "100", "charge fully once") {
                eprintln!("dynamisd: charge fully once: {e}");
            }
        }
    }

    pub fn json(&self, root: &SysRoot) -> Value {
        json!({
            "supported": Self::supported(root),
            "active": self.active.is_some(),
            "restore_to": self.active.as_ref().map(|a| a.restore_to.clone()),
            "ended": self.ended,
        })
    }
}
