//! Summaries and actions for front ends (the app), as JSON. The D-Bus
//! methods in `bus.rs` are thin wrappers around these, so they can be tested
//! without a bus.

use dynamis_core::calibrate::Verdict;
use dynamis_core::sensors::power;
use dynamis_core::settings;
use serde_json::{json, Value};

use crate::core::{open_engine, Core};

fn label(id: &str) -> &'static str {
    settings::by_id(id).map(|s| s.label()).unwrap_or("")
}

impl Core {
    /// Everything the app's Home and Battery screens show.
    pub fn status_json(&self) -> Value {
        let ps = power::read(&self.root);
        let bat = ps.batteries.first();
        let on_ac = ps.on_ac() || ps.batteries.is_empty();
        let watts = ps.total_battery_power_w();
        let charging = bat.and_then(|b| b.status.as_deref()) == Some("Charging");
        let hours = |wh: Option<f64>| match (wh, watts) {
            (Some(wh), Some(w)) if w > 0.2 => Some((wh / w * 10.0).round() / 10.0),
            _ => None,
        };
        let now_wh = bat.and_then(|b| b.energy_now_wh);
        let full_wh = bat.and_then(|b| b.energy_full_wh);

        let kept: Vec<Value> = self
            .learn
            .learned()
            .iter()
            .map(|r| {
                json!({
                    "setting": r.setting, "label": label(&r.setting), "value": r.value,
                    "baseline": r.baseline, "saves_w": -r.effect.mean, "reason": r.reason,
                })
            })
            .collect();
        let saves: f64 = self.learn.learned().iter().map(|r| -r.effect.mean).sum();
        let engine = open_engine(&self.root, &self.state_dir).ok();
        let mut tested: Vec<&str> = self
            .learn
            .records
            .iter()
            .filter(|r| r.verdict != Verdict::Inconclusive)
            .map(|r| r.setting.as_str())
            .collect();
        tested.sort();
        tested.dedup();
        let tested_settings = tested.len();
        let (workload, reason) = self.auto_status();

        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "profile": self.active().as_str(),
            "profiles": self.available().iter().map(|p| p.as_str()).collect::<Vec<_>>(),
            "holds": self.holds.values().map(|h| json!({"profile": h.profile.as_str(), "app": h.application_id, "reason": h.reason})).collect::<Vec<_>>(),
            "auto": { "on": self.auto.enabled, "workload": workload, "reason": reason },
            "learning": {
                "on": self.learn.enabled,
                "status": self.learn.status(),
                "tests": self.learn.records.len(),
                "tested_settings": tested_settings,
                "kept": kept,
                "saves_w": (saves * 100.0).round() / 100.0,
            },
            "power": {
                "on_ac": on_ac,
                "has_battery": bat.is_some(),
                "battery_pct": bat.and_then(|b| b.capacity_pct),
                "charging": charging,
                "watts": watts,
                "hours_left": if on_ac { None } else { hours(now_wh) },
                "hours_to_full": if charging { hours(full_wh.zip(now_wh).map(|(f, n)| (f - n).max(0.0))) } else { None },
                "health_pct": bat.and_then(|b| b.health_pct),
                "cycles": bat.and_then(|b| b.cycle_count),
                "design_wh": bat.and_then(|b| b.energy_full_design_wh),
                "full_wh": full_wh,
                "charge_limit": bat.and_then(|b| b.charge_limit_pct),
            },
            "safe_mode": engine.as_ref().and_then(|e| e.safe_mode()),
            "pending": engine.as_ref().and_then(|e| e.pending()).map(|p| json!({
                "id": p.id, "label": label(&p.id), "to": p.to, "trial": p.trial_deadline.is_some(),
            })),
            "display": self.learn.display.fit(),
        })
    }

    /// The change journal, newest first, with plain names added.
    pub fn journal_json(&self, n: usize) -> Result<Value, String> {
        let e = open_engine(&self.root, &self.state_dir)?;
        let mut entries: Vec<Value> = e
            .journal(n)
            .into_iter()
            .map(|j| {
                let mut v = serde_json::to_value(&j).unwrap_or_default();
                v["label"] = json!(label(&j.id));
                v
            })
            .collect();
        entries.reverse();
        Ok(Value::Array(entries))
    }

    pub fn settings_json(&self) -> Result<Value, String> {
        let e = open_engine(&self.root, &self.state_dir)?;
        let rows = dynamis_core::overview::settings(&self.root, &e);
        serde_json::to_value(rows).map_err(|e| e.to_string())
    }

    pub fn inventory_json(&self) -> Value {
        serde_json::to_value(dynamis_core::inventory::read(&self.root)).unwrap_or_default()
    }

    /// Undo one setting: forget it if it was learned, otherwise put its
    /// original value back.
    pub fn undo(&mut self, id: &str) -> Result<String, String> {
        if self.learn.learned().iter().any(|r| r.setting == id) {
            self.forget(id)?;
            return Ok(format!("{} is back to its original value", label(id)));
        }
        let mut e = open_engine(&self.root, &self.state_dir)?;
        match e.restore_one(id).map_err(|e| e.to_string())? {
            Some(v) => Ok(format!("{} is back to {v}", label(id))),
            None => Ok(format!("{} was never changed", label(id))),
        }
    }

    /// "Restore original settings": always works, also in safe mode. Turns
    /// auto mode and learning off first, so nothing re-applies a change.
    pub fn restore_all(&mut self, now: f64) -> Result<Value, String> {
        if self.learn.enabled {
            self.set_learning(false)?;
        }
        if self.auto.enabled {
            self.set_auto(false, now)?;
        }
        if self.apps.policy.changes_shares() {
            let dir = self.state_dir.clone();
            self.apps.stop_changing_shares(&dir)?;
        }
        let mut e = open_engine(&self.root, &self.state_dir)?;
        let done = e.restore().map_err(|e| e.to_string())?;
        Ok(json!(done
            .into_iter()
            .map(|(id, v, ok)| json!({"id": id, "label": label(&id), "value": v, "ok": ok}))
            .collect::<Vec<_>>()))
    }

    /// Change one setting by hand (Advanced). Risky settings need
    /// `allow_risky`, which the app only sends after an explicit yes.
    pub fn set_setting(&self, id: &str, value: &str, allow_risky: bool) -> Result<(), String> {
        let mut e = open_engine(&self.root, &self.state_dir)?;
        e.apply(&dynamis_core::engine::Request {
            id,
            value,
            allow_risky,
            trial_secs: None,
            reason: "set in the app",
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    pub fn set_safe_mode(&self, on: bool) -> Result<Option<String>, String> {
        let mut e = open_engine(&self.root, &self.state_dir)?;
        e.set_safe_mode(on.then_some("switched on in the app"))
            .map_err(|e| e.to_string())?;
        Ok(e.safe_mode())
    }
}
