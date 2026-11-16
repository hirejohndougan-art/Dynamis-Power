//! Battery recalibration on request (TLP's `recalibrate`; decision 0019).
//!
//! A battery's charge gauge drifts: after months of partial charges it may
//! say 20% when the battery is nearly empty, or the other way round. Running
//! it down and then charging it fully lets the gauge learn again what the
//! battery really holds.
//!
//! 1. **Running down:** with the charger plugged in, the battery is told to
//!    run the laptop anyway (`charge_behaviour` = `force-discharge`, only
//!    where the kernel offers it in the standard file) until it reaches
//!    15%. Not lower: an empty battery is bad for it, and a laptop that
//!    switches off mid-way would leave the setting on.
//! 2. **Charging fully:** back to `auto`; a charge limit is lifted to 100%
//!    for the charge ("Charge fully once"), and put back when full.
//!
//! Only after an explicit yes. It stops, and switches back to `auto` at
//! once, when the charger is unplugged, the laptop gets warm, safe mode
//! comes on, the laptop sleeps, the person stops it, Restore is pressed,
//! or a day passes; and at the next start of the service if it was running
//! (a restart in the middle never leaves the battery running down).

use std::path::Path;

use dynamis_core::sensors::power;
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{open_engine, write_atomic};

const FILE: &str = "recalibrate.json";
const VERSION: u32 = 1;
/// Where running down stops.
pub const FLOOR_PCT: u8 = 15;
/// It needs a little more than the floor to be worth starting.
const MIN_START_PCT: u8 = 25;
const MAX_TEMP_C: f64 = 80.0;
const MAX_S: f64 = 86_400.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    RunningDown,
    Charging,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Active {
    phase: Phase,
    started: f64,
    /// What the gauge said the battery holds when it started (Wh).
    full_before_wh: Option<f64>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    v: u32,
    active: Active,
}

#[derive(Default)]
pub struct Recal {
    active: Option<Active>,
    /// How it last ended, for the app.
    ended: Option<String>,
    /// What the gauge said before and after, once it finished.
    result: Option<(Option<f64>, Option<f64>)>,
}

/// Whether this laptop can do it: a battery with the standard control
/// listing `force-discharge`.
pub fn supported(root: &SysRoot) -> bool {
    power::read(root).batteries.iter().any(|b| {
        b.charge_behaviour_supported
            && root
                .read_string(&format!(
                    "/sys/class/power_supply/{}/charge_behaviour",
                    b.name
                ))
                .is_ok_and(|t| {
                    t.split_whitespace()
                        .any(|w| w.trim_matches(['[', ']']) == "force-discharge")
                })
    })
}

fn battery(root: &SysRoot) -> (Option<u8>, Option<String>, Option<f64>) {
    let ps = power::read(root);
    let b = ps.batteries.first();
    (
        b.and_then(|b| b.capacity_pct),
        b.and_then(|b| b.status.clone()),
        b.and_then(|b| b.energy_full_wh),
    )
}

fn set_behaviour(root: &SysRoot, dir: &Path, value: &str, reason: &str) -> Result<(), String> {
    open_engine(root, dir)?
        .set_charge_behaviour(value, reason)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

impl Recal {
    /// Load the state. If it was running when the service stopped, the
    /// battery is switched back to normal now and the run ends.
    pub fn load(root: &SysRoot, dir: &Path) -> Recal {
        let mut r = Recal::default();
        let saved = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok());
        if saved.is_some() {
            if let Err(e) = set_behaviour(
                root,
                dir,
                "auto",
                "recalibration ended: the service restarted",
            ) {
                eprintln!("dynamisd: recalibration: {e}");
            }
            let _ = std::fs::remove_file(dir.join(FILE));
            r.ended = Some("the service restarted".into());
        }
        r
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Whether the charge limit should be lifted now (the charging part).
    pub fn wants_full_charge(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|a| a.phase == Phase::Charging)
    }

    fn save(&self, dir: &Path) -> Result<(), String> {
        match &self.active {
            Some(a) => write_atomic(
                &dir.join(FILE),
                &serde_json::to_string(&Saved {
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

    /// Start: plugged in, above 25%, where supported, not in safe mode or
    /// dry run.
    pub fn start(
        &mut self,
        root: &SysRoot,
        dir: &Path,
        now: f64,
        on_ac: bool,
    ) -> Result<(), String> {
        if self.active.is_some() {
            return Err("recalibration is already running".into());
        }
        if !supported(root) {
            return Err("This laptop's battery cannot run it while plugged in, so it cannot be recalibrated here.".into());
        }
        if !on_ac {
            return Err("Plug the charger in first: the battery runs down while plugged in, then charges fully.".into());
        }
        let (pct, _, full) = battery(root);
        if pct.is_none_or(|p| p < MIN_START_PCT) {
            return Err(format!("Charge the battery above {MIN_START_PCT}% first."));
        }
        let engine = open_engine(root, dir)?;
        if engine.dry_run() {
            return Err("Dry-run mode is on, so nothing is changed.".into());
        }
        drop(engine);
        set_behaviour(
            root,
            dir,
            "force-discharge",
            "recalibration: running the battery down",
        )?;
        self.active = Some(Active {
            phase: Phase::RunningDown,
            started: now,
            full_before_wh: full,
        });
        self.ended = None;
        self.result = None;
        self.save(dir)
    }

    /// Stop now: the battery goes back to normal. The charge limit is put
    /// back by "Charge fully once" if it was lifted (the caller stops it).
    pub fn stop(&mut self, root: &SysRoot, dir: &Path, why: &str) {
        if self.active.take().is_none() {
            return;
        }
        if let Err(e) = set_behaviour(root, dir, "auto", &format!("recalibration ended: {why}")) {
            eprintln!("dynamisd: recalibration: {e}");
        }
        self.ended = Some(why.to_string());
        let _ = self.save(dir);
    }

    /// One reading. Returns whether "Charge fully once" should start now
    /// (the running-down part just finished).
    pub fn tick(
        &mut self,
        root: &SysRoot,
        dir: &Path,
        now: f64,
        on_ac: bool,
        temp_c: Option<f64>,
        safe_mode: bool,
    ) -> bool {
        let Some(a) = self.active.clone() else {
            return false;
        };
        let (pct, status, full) = battery(root);
        let stop = if a.phase == Phase::RunningDown && !on_ac {
            Some("the charger was unplugged")
        } else if temp_c.is_some_and(|t| t >= MAX_TEMP_C) {
            Some("the laptop got warm")
        } else if safe_mode {
            Some("safe mode is on")
        } else if now - a.started >= MAX_S {
            Some("a day passed")
        } else {
            None
        };
        if let Some(why) = stop {
            self.stop(root, dir, why);
            return false;
        }
        match a.phase {
            Phase::RunningDown if pct.is_some_and(|p| p <= FLOOR_PCT) => {
                if let Err(e) = set_behaviour(root, dir, "auto", "recalibration: charging fully") {
                    eprintln!("dynamisd: recalibration: {e}");
                }
                if let Some(x) = self.active.as_mut() {
                    x.phase = Phase::Charging;
                }
                let _ = self.save(dir);
                true
            }
            Phase::RunningDown => false,
            Phase::Charging => {
                let full_now =
                    status.as_deref() == Some("Full") || (on_ac && pct.is_some_and(|p| p >= 100));
                if full_now {
                    self.active = None;
                    self.result = Some((a.full_before_wh, full));
                    self.ended = Some("done".into());
                    let _ = self.save(dir);
                }
                false
            }
        }
    }

    pub fn json(&self, root: &SysRoot) -> Value {
        let (pct, _, _) = battery(root);
        json!({
            "supported": supported(root),
            "phase": self.active.as_ref().map(|a| a.phase),
            "battery_pct": pct,
            "floor_pct": FLOOR_PCT,
            "min_start_pct": MIN_START_PCT,
            "ended": self.ended,
            "full_before_wh": self.result.and_then(|r| r.0),
            "full_after_wh": self.result.and_then(|r| r.1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> (std::path::PathBuf, SysRoot, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dynamis-recal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bat = dir.join("root/sys/class/power_supply/BAT0");
        std::fs::create_dir_all(&bat).unwrap();
        for (f, v) in [
            ("type", "Battery"),
            ("status", "Charging"),
            ("capacity", "60"),
            ("energy_full", "38000000"),
            ("energy_now", "22800000"),
            ("charge_behaviour", "[auto] inhibit-charge force-discharge"),
        ] {
            std::fs::write(bat.join(f), v).unwrap();
        }
        let state = dir.join("state");
        std::fs::create_dir_all(&state).unwrap();
        (dir.clone(), SysRoot::at(dir.join("root")), state)
    }

    fn behaviour(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("root/sys/class/power_supply/BAT0/charge_behaviour"))
            .unwrap()
            .trim()
            .to_string()
    }

    fn set(dir: &Path, f: &str, v: &str) {
        std::fs::write(dir.join("root/sys/class/power_supply/BAT0").join(f), v).unwrap();
    }

    #[test]
    fn runs_down_then_charges_fully() {
        let (dir, root, state) = sandbox("full");
        let mut r = Recal::default();
        assert!(r
            .start(&root, &state, 0.0, false)
            .unwrap_err()
            .contains("Plug the charger"));
        r.start(&root, &state, 0.0, true).unwrap();
        assert_eq!(behaviour(&dir), "force-discharge");
        assert!(!r.tick(&root, &state, 10.0, true, None, false));
        set(&dir, "capacity", "15");
        assert!(
            r.tick(&root, &state, 20.0, true, None, false),
            "charging starts"
        );
        assert_eq!(behaviour(&dir), "auto");
        assert!(r.wants_full_charge());
        set(&dir, "status", "Full");
        set(&dir, "energy_full", "36500000");
        r.tick(&root, &state, 30.0, true, None, false);
        assert!(!r.is_active());
        let j = r.json(&root);
        assert_eq!(j["ended"], "done");
        assert_eq!(j["full_before_wh"], 38.0);
        assert_eq!(j["full_after_wh"], 36.5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unplugging_or_a_restart_puts_the_battery_back() {
        let (dir, root, state) = sandbox("stop");
        let mut r = Recal::default();
        r.start(&root, &state, 0.0, true).unwrap();
        r.tick(&root, &state, 5.0, false, None, false);
        assert!(!r.is_active());
        assert_eq!(behaviour(&dir), "auto");
        assert_eq!(r.json(&root)["ended"], "the charger was unplugged");

        // A restart in the middle. (A test copy holds one bare word after
        // a write; the kernel's file keeps listing the choices.)
        set(
            &dir,
            "charge_behaviour",
            "[auto] inhibit-charge force-discharge",
        );
        r.start(&root, &state, 10.0, true).unwrap();
        assert_eq!(behaviour(&dir), "force-discharge");
        let again = Recal::load(&root, &state);
        assert!(!again.is_active());
        assert_eq!(behaviour(&dir), "auto");
        assert_eq!(again.json(&root)["ended"], "the service restarted");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
