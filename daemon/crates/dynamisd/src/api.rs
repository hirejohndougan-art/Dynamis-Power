//! Summaries and actions for front ends (the app), as JSON. The D-Bus
//! methods in `bus.rs` are thin wrappers around these, so they can be tested
//! without a bus.

use dynamis_core::calibrate::Verdict;
use dynamis_core::profiles;
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
        // Before the engine below is opened: it takes the engine's lock.
        let progress = self.learning_progress();
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
                "progress": progress,
                "waiting": self.learn.main_wait().map(|(k, pct)| json!({"reason": k, "share_pct": pct})),
                "power_update_s": self.learn.power_update_s(),
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
                // The hottest temperature sensor, for Home.
                "temp_c": dynamis_core::sensors::thermal::max_c(&dynamis_core::sensors::thermal::read(&self.root)),
            },
            "safe_mode": engine.as_ref().and_then(|e| e.safe_mode()),
            "dry_run": engine.as_ref().is_some_and(|e| e.dry_run()),
            "kept": self.kept.json(),
            "unclean": crate::crash::json(&self.state_dir),
            "full_charge": self.fullcharge.json(&self.root),
            "missing_modes": profiles::missing(&self.root).into_iter().map(|(p, why)| json!({"profile": p.as_str(), "why": why})).collect::<Vec<_>>(),
            "pending": engine.as_ref().and_then(|e| e.pending()).map(|p| json!({
                "id": p.id, "label": label(&p.id), "to": p.to, "trial": p.trial_deadline.is_some(),
            })),
            "display": self.learn.display.fit(),
            "last_until": self.predict.target.as_ref().map(|t| json!({
                "until": t.until,
                "level": t.level,
                "reason": self.predict.last.as_ref().map(|s| s.reason.clone()),
                "on_track": self.predict.last.as_ref().is_none_or(|s| s.on_track),
            })),
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
        let dir = self.state_dir.clone();
        self.kept.drop_setting(id, &dir)?;
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
        if self.predict.target.is_some() {
            let dir = self.state_dir.clone();
            self.predict.set_target(None, 0.0, &dir)?;
        }
        if self.predict.adaptive_charging {
            let (root, dir) = (self.root.clone(), self.state_dir.clone());
            self.predict.set_adaptive_charging(false, &root, &dir)?;
        }
        self.apps.forced_slow = false;
        let dir = self.state_dir.clone();
        self.kept.clear(&dir)?;
        self.devices.clear(&dir);
        self.brightness.clear(&dir);
        self.stop_recalibration("everything was restored");
        self.fullcharge.drop_it(&dir, "everything was restored");
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
    pub fn set_setting(&mut self, id: &str, value: &str, allow_risky: bool) -> Result<(), String> {
        let mut e = open_engine(&self.root, &self.state_dir)?;
        e.apply(&dynamis_core::engine::Request {
            id,
            value,
            allow_risky,
            trial_secs: None,
            reason: "set in the app",
        })
        .map_err(|e| e.to_string())?;
        drop(e);
        if id == "battery.charge_limit" {
            let dir = self.state_dir.clone();
            self.fullcharge.drop_it(&dir, "you set the limit by hand");
        }
        // The battery's limits stay after a restart; others when asked to.
        if crate::kept::ALWAYS_KEPT.contains(&id) || self.kept.settings.contains_key(id) {
            let dir = self.state_dir.clone();
            self.kept.keep(id, value, crate::core::unix_now(), &dir)?;
        }
        Ok(())
    }

    /// Keep a setting's current value after restarts and sleep, or stop.
    pub fn set_kept(&mut self, id: &str, keep: bool) -> Result<(), String> {
        let dir = self.state_dir.clone();
        if !keep {
            return self.kept.drop_setting(id, &dir).map(|_| ());
        }
        let s = settings::by_id(id).ok_or_else(|| format!("unknown setting '{id}'"))?;
        let value = s.read(&self.root).map_err(|e| e.to_string())?;
        if value == "mixed" {
            return Err("this setting differs between devices; set one value first".into());
        }
        self.kept.keep(id, &value, crate::core::unix_now(), &dir)
    }

    pub fn set_safe_mode(&self, on: bool) -> Result<Option<String>, String> {
        let mut e = open_engine(&self.root, &self.state_dir)?;
        e.set_safe_mode(on.then_some("switched on in the app"))
            .map_err(|e| e.to_string())?;
        Ok(e.safe_mode())
    }

    // --- Advanced: Profiles, Sensors, System, Developer ---------------------

    /// What each profile sets here: the default, the person's own choice,
    /// and the values offered.
    pub fn profiles_json(&self) -> Value {
        use dynamis_core::profiles::{editable, plan, plan_with};
        let rows: Vec<Value> = self
            .available()
            .iter()
            .map(|p| {
                let now = plan_with(&self.root, *p, &self.profile_overrides);
                let default = plan(&self.root, *p);
                let settings: Vec<Value> = editable(&self.root, *p)
                    .into_iter()
                    .map(|(id, choices)| {
                        let pick = |steps: &[dynamis_core::profiles::Step]| {
                            steps.iter().find(|s| s.id == id).map(|s| s.value.clone())
                        };
                        json!({
                            "id": id,
                            "label": label(id),
                            "choices": choices,
                            "value": pick(&now),
                            "default": pick(&default),
                        })
                    })
                    .collect();
                json!({"profile": p.as_str(), "settings": settings})
            })
            .collect();
        json!({"profiles": rows, "overrides": self.profile_overrides})
    }

    /// Replace the person's profile choices (checked against this machine)
    /// and apply the active profile again.
    pub fn set_profile_overrides(&mut self, mut ov: profiles::Overrides) -> Result<(), String> {
        ov.v = profiles::Overrides::VERSION;
        ov.profiles.retain(|_, m| !m.is_empty());
        ov.check(&self.root)?;
        crate::core::write_atomic(
            &self.state_dir.join("profiles.json"),
            &serde_json::to_string_pretty(&ov).map_err(|e| e.to_string())?,
        )?;
        self.profile_overrides = ov;
        self.apply_active("profile settings changed in Advanced")
    }

    /// Live readings for the Sensors tab. Counters (energy, idle time) are
    /// totals; the app turns two readings into rates.
    pub fn sensors_json(&self) -> Value {
        use dynamis_core::sensors::{self, idle};
        let snap = sensors::snapshot(&self.root);
        json!({
            "time": snap.time,
            "on_ac": snap.power.on_ac(),
            "battery_w": snap.power.total_battery_power_w(),
            "battery_pct": snap.power.batteries.first().and_then(|b| b.capacity_pct),
            "rapl": snap.rapl,
            "pressure": {
                "cpu": snap.pressure.cpu.as_ref().map(|p| p.some.avg10),
                "memory": snap.pressure.memory.as_ref().map(|p| p.some.avg10),
                "io": snap.pressure.io.as_ref().map(|p| p.some.avg10),
            },
            "thermal": snap.thermal,
            "cpu_mhz": snap.cpu.avg_mhz(),
            "idle": idle::read(&self.root),
            "brightness_pct": snap.backlight.map(|b| b.percent),
        })
    }

    /// About this machine and the service, for the System tab.
    pub fn system_json(&self, uptime_s: f64) -> Value {
        let me = dynamis_core::selfstat::SelfUsage::read();
        let os = self.root.read_string("/etc/os-release").ok().and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_string())
        });
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "kernel": self.root.read_string("/proc/sys/kernel/osrelease").ok(),
            "os": os,
            "uptime_s": uptime_s,
            "cpu_s": me.as_ref().map(|m| m.cpu_s),
            "cpu_pct": me.as_ref().map(|m| if uptime_s > 0.0 { m.cpu_s / uptime_s * 100.0 } else { 0.0 }),
            "memory_mb": me.as_ref().and_then(|m| m.rss_mb),
            "state_dir": self.state_dir.display().to_string(),
        })
    }

    /// Advisor mode: what Dynamis Power would set, as a TLP or TuneD file.
    pub fn advise(&self, format: &str) -> Result<String, String> {
        use dynamis_core::advisor::{self, Format, Plan};
        let format = Format::parse(format).ok_or("format must be 'tlp' or 'tuned'")?;
        let ac: std::collections::BTreeMap<String, String> =
            profiles::plan_with(&self.root, self.active(), &self.profile_overrides)
                .into_iter()
                .map(|s| (s.id.to_string(), s.value))
                .collect();
        let mut bat = ac.clone();
        for r in self.learn.learned() {
            bat.insert(r.setting.clone(), r.value.clone());
        }
        if let Some(c) = self
            .predict
            .charge
            .as_ref()
            .filter(|_| self.predict.adaptive_charging)
        {
            bat.insert("battery.charge_limit".into(), c.limit_pct.to_string());
        }
        let header = format!(
            "Written by Dynamis Power {} from the {} mode and what it learned on this laptop.",
            env!("CARGO_PKG_VERSION"),
            self.active().as_str()
        );
        Ok(advisor::write(&Plan { ac, bat }, format, &header))
    }

    pub fn set_dry_run(&mut self, on: bool) -> Result<(), String> {
        let mut e = open_engine(&self.root, &self.state_dir)?;
        e.set_dry_run(on).map_err(|e| e.to_string())
    }
}

// --- Importing a TLP configuration (decision 0017) -------------------------

impl Core {
    fn tlp_plan(&self) -> Option<Vec<dynamis_core::tlpimport::Line>> {
        use dynamis_core::tlpimport;
        let conf = tlpimport::read(&self.root)?;
        let engine = open_engine(&self.root, &self.state_dir).ok();
        let blocked = engine.as_ref().map(|e| e.blocked()).unwrap_or_default();
        let refused = |id: &str, v: &str| {
            engine
                .as_ref()
                .and_then(|e| e.quirks().blocking(id, v).map(|q| q.reason.clone()))
        };
        Some(tlpimport::plan(&self.root, &conf, &blocked, refused))
    }

    /// What importing TLP's configuration would change, line by line (for
    /// the app's preview). `found` is false when there is no TLP
    /// configuration here.
    pub fn tlp_import_json(&self) -> Value {
        match self.tlp_plan() {
            None => json!({"found": false, "lines": []}),
            Some(lines) => json!({"found": true, "lines": lines}),
        }
    }

    /// Apply the preview's lines at these positions (the ones the person
    /// left ticked). Settings are kept after restarts; mode values go into
    /// the person's choices for that mode. Returns how many were applied
    /// and what failed.
    pub fn apply_tlp_import(&mut self, pick: &[usize]) -> Result<Value, String> {
        let lines = self.tlp_plan().ok_or("no TLP configuration was found")?;
        let mut applied = 0;
        let mut failed: Vec<Value> = Vec::new();
        let mut overrides = self.profile_overrides.clone();
        let mut mode_changes = 0;
        for &i in pick {
            let Some(l) = lines.get(i) else { continue };
            let (Some(id), Some(v), Some(target)) = (l.setting, l.value.as_deref(), l.target)
            else {
                continue;
            };
            match target.profile() {
                None => {
                    let r = self
                        .set_setting(id, v, false)
                        .and_then(|_| self.set_kept(id, true));
                    match r {
                        Ok(()) => applied += 1,
                        Err(e) => failed.push(json!({"tlp": l.tlp, "error": e})),
                    }
                }
                Some(p) => {
                    overrides
                        .profiles
                        .entry(p)
                        .or_default()
                        .insert(id.to_string(), v.to_string());
                    mode_changes += 1;
                }
            }
        }
        if mode_changes > 0 {
            match self.set_profile_overrides(overrides) {
                Ok(()) => applied += mode_changes,
                Err(e) => failed.push(json!({"tlp": "modes", "error": e})),
            }
        }
        // Mark it in the journal so Activity shows where these came from.
        if let Ok(e) = open_engine(&self.root, &self.state_dir) {
            let _ = e.note(
                "import",
                "tlp",
                &format!("{applied} values imported from TLP's configuration"),
            );
        }
        Ok(json!({"applied": applied, "failed": failed}))
    }
}
