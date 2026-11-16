//! Prediction (phase 7) inside the daemon: learning habits, "Last until",
//! and charging that follows when the laptop is usually unplugged.
//!
//! The decisions are in `dynamis_core::predict`; this file keeps the state,
//! saves it, and makes changes through the engine like everything else:
//!
//! - "Last until" caps the power mode (and at its strongest level slows
//!   background apps). It ends by itself when the charger is plugged in or
//!   the time is reached.
//! - Charging that follows habits changes only the battery's charge limit,
//!   where the battery offers one, between 80% and 100%. Off by default.

use std::path::Path;

use dynamis_core::engine::Request;
use dynamis_core::predict::{self, ChargePlan, Habits, Level, Target, TargetStatus};
use dynamis_core::sensors::power;
use dynamis_core::settings::{self, Probe};
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{open_engine, write_atomic};

const FILE_VERSION: u32 = 1;
const FILE: &str = "predict.json";
const HABITS_FILE: &str = "habits.json";
/// Habits are saved this often (and when the charger is plugged or unplugged).
const SAVE_EVERY_S: f64 = 1800.0;
const CHARGE_LIMIT: &str = "battery.charge_limit";

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    v: u32,
    #[serde(default)]
    target: Option<Target>,
    #[serde(default)]
    adaptive_charging: bool,
}

#[derive(Default)]
pub struct PredictState {
    pub habits: Habits,
    pub target: Option<Target>,
    pub adaptive_charging: bool,
    pub last: Option<TargetStatus>,
    pub charge: Option<ChargePlan>,
    /// The last reading's forecast of when the battery runs out.
    pub forecast: Option<f64>,
    /// Why the last target ended, for the app ("reached 17:00", …).
    pub ended: Option<String>,
    last_save: Option<f64>,
}

impl PredictState {
    pub fn load(state_dir: &Path) -> PredictState {
        let read = |n: &str| std::fs::read_to_string(state_dir.join(n)).ok();
        let saved: Saved = read(FILE)
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        PredictState {
            habits: read(HABITS_FILE)
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default(),
            target: saved.target,
            adaptive_charging: saved.adaptive_charging,
            ..PredictState::default()
        }
    }

    pub fn needs_ticks(&self) -> bool {
        self.target.is_some() || self.adaptive_charging
    }

    /// The fastest power mode "Last until" allows right now.
    pub fn level(&self) -> Level {
        self.target.as_ref().map_or(Level::Normal, |t| t.level)
    }

    fn save(&self, state_dir: &Path) -> Result<(), String> {
        let s = Saved {
            v: FILE_VERSION,
            target: self.target.clone(),
            adaptive_charging: self.adaptive_charging,
        };
        write_atomic(
            &state_dir.join(FILE),
            &serde_json::to_string(&s).map_err(|e| e.to_string())?,
        )
    }

    fn save_habits(&mut self, state_dir: &Path, now: f64) {
        self.last_save = Some(now);
        match serde_json::to_string(&self.habits) {
            Ok(t) => {
                if let Err(e) = write_atomic(&state_dir.join(HABITS_FILE), &t) {
                    eprintln!("dynamisd: {e}");
                }
            }
            Err(e) => eprintln!("dynamisd: habits: {e}"),
        }
    }

    /// Start or end "Last until". `until` is Unix time; `None` ends it.
    pub fn set_target(
        &mut self,
        until: Option<f64>,
        now: f64,
        state_dir: &Path,
    ) -> Result<(), String> {
        match until {
            Some(u) if u <= now + 600.0 => {
                return Err("pick a time at least ten minutes from now".into())
            }
            Some(u) if u > now + 7.0 * 86400.0 => {
                return Err("pick a time within the next week".into())
            }
            Some(u) => {
                self.target = Some(Target::new(u));
                self.ended = None;
            }
            None => {
                self.target = None;
                self.last = None;
            }
        }
        self.save(state_dir)
    }

    /// Switch charging that follows habits on or off. Off puts the charge
    /// limit back to its original value.
    pub fn set_adaptive_charging(
        &mut self,
        on: bool,
        root: &SysRoot,
        state_dir: &Path,
    ) -> Result<(), String> {
        if on && !charge_limit_supported(root) {
            return Err("this battery has no charge limit control".into());
        }
        self.adaptive_charging = on;
        self.save(state_dir)?;
        if !on {
            self.charge = None;
            let mut e = open_engine(root, state_dir)?;
            e.restore_one(CHARGE_LIMIT).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// One reading. Returns true if the "Last until" level changed (the
    /// power mode may need to change).
    pub fn tick(
        &mut self,
        root: &SysRoot,
        state_dir: &Path,
        now: f64,
        on_ac: bool,
        power_w: Option<f64>,
        energy_wh: Option<f64>,
    ) -> bool {
        let event = self.habits.add(now, on_ac, power_w);
        if event.is_some() || self.last_save.is_none_or(|t| now - t >= SAVE_EVERY_S) {
            self.save_habits(state_dir, now);
        }
        self.forecast = if on_ac {
            None
        } else {
            energy_wh.and_then(|e| self.habits.forecast_empty(now, e, power_w))
        };

        let before = self.level();
        if let Some(t) = self.target.as_mut() {
            let end = if on_ac {
                Some("the charger was plugged in")
            } else if now >= t.until {
                Some("the time was reached")
            } else {
                None
            };
            if let Some(why) = end {
                self.ended = Some(why.to_string());
                self.target = None;
                self.last = None;
                if let Err(e) = self.save(state_dir) {
                    eprintln!("dynamisd: {e}");
                }
            } else {
                let full = power::read(root)
                    .batteries
                    .iter()
                    .filter_map(|b| b.energy_full_wh)
                    .reduce(|a, b| a + b);
                self.last = Some(t.step(now, energy_wh, power_w, full));
            }
        }

        if self.adaptive_charging {
            self.follow_habits(root, state_dir, now);
        }
        self.level() != before
    }

    fn follow_habits(&mut self, root: &SysRoot, state_dir: &Path, now: f64) {
        let plan = predict::charge_plan(&self.habits, now);
        let current = settings::by_id(CHARGE_LIMIT).and_then(|s| s.read(root).ok());
        if current.as_deref() != Some(plan.limit_pct.to_string().as_str()) {
            let result = open_engine(root, state_dir).and_then(|mut e| {
                e.apply(&Request {
                    id: CHARGE_LIMIT,
                    value: &plan.limit_pct.to_string(),
                    allow_risky: false,
                    trial_secs: None,
                    reason: &format!("charging follows your habits: {}", plan.reason),
                })
                .map(|_| ())
                .map_err(|e| e.to_string())
            });
            if let Err(e) = result {
                // Once per plan, not every reading.
                if self.charge.as_ref() != Some(&plan) {
                    eprintln!("dynamisd: charge limit: {e}");
                }
            }
        }
        self.charge = Some(plan);
    }

    /// Everything the Battery screen shows about predictions (JSON).
    pub fn json(&self, root: &SysRoot, now: f64) -> Value {
        json!({
            "forecast_end": self.forecast,
            "typical_w": self.habits.typical_w(now).map(|w| (w * 100.0).round() / 100.0),
            "usual_unplug": self.habits.usual_unplug(now),
            "unplugs_seen": self.habits.unplugs_seen(),
            "target": self.last.as_ref().map(|s| serde_json::to_value(s).unwrap_or_default())
                .or_else(|| self.target.as_ref().map(|t| json!({"until": t.until, "level": t.level}))),
            "target_ended": self.ended,
            "adaptive_charging": self.adaptive_charging,
            "charge": self.charge,
            "charge_supported": charge_limit_supported(root),
        })
    }
}

fn charge_limit_supported(root: &SysRoot) -> bool {
    settings::by_id(CHARGE_LIMIT).is_some_and(|s| matches!(s.probe(root), Probe::Supported { .. }))
}
