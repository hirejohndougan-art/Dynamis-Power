//! Learning (phase 4): calibration experiments, the learned settings they
//! produce, and the display power model.
//!
//! An experiment only starts at a quiet moment ("pre-flight"): on battery,
//! battery at least 40%, not hot, idle or light use, no app holds, no other
//! trial, not in safe mode. It stops early if any of that changes, if the
//! profile switches, or if the health monitor sees trouble. The candidate is
//! applied as a trial, so a crash or freeze mid-experiment is undone (and
//! the setting blocked) the next time Dynamis Power starts.
//!
//! Kept results are **learned settings**. They were measured on battery, so
//! they are applied on battery and put back to their original values on
//! mains power.
//!
//! Phase 5: once every setting has been tested on its own, the processor
//! settings that affect each other are tuned together, one step at a time
//! (`dynamis_core::tune`). Their learned values depend on the power mode's
//! own settings, so they are re-applied whenever the power mode changes.

use std::fs;
use std::path::Path;

use dynamis_core::auto::{Context, Workload};
use dynamis_core::calibrate::{
    self, Action, Experiment, Limits, Plan, PowerSource, Record, RecordContext, Sample, Verdict,
};
use dynamis_core::display::DisplayModel;
use dynamis_core::engine::Request;
use dynamis_core::health::{self, Monitor};
use dynamis_core::profiles::Profile;
use dynamis_core::settings::{self, Probe};
use dynamis_core::tune::{self, State};
use serde::{Deserialize, Serialize};

use crate::core::{open_engine, write_atomic, Core};

/// Version 2 adds `group` to records (phase 5). Version 1 files load as is;
/// their single results for the settings now tuned together are dropped,
/// since those are re-tested together.
const FORMAT_VERSION: u32 = 2;
/// Inconclusive results are retried up to this many times in all.
const MAX_TRIES: usize = 3;
const MIN_BATTERY_PCT: f64 = 40.0;
const MAX_TEMP_C: f64 = 80.0;

/// Timing, adjustable for tests.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub block_s: f64,
    pub settle_s: f64,
    /// Rest between experiments.
    pub cooldown_s: f64,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            block_s: 60.0,
            settle_s: 10.0,
            cooldown_s: 300.0,
        }
    }
}

struct Running {
    exp: Experiment,
    monitor: Monitor,
    workload: Workload,
    profile: Profile,
    /// The candidate is applied right now (a trial is pending).
    b_applied: bool,
    /// Phase 5: the other tuned values while testing (empty for singles).
    group: State,
}

#[derive(Default, Serialize, Deserialize)]
struct SavedRecords {
    v: u32,
    records: Vec<Record>,
}

#[derive(Default, Serialize, Deserialize)]
struct SavedSwitch {
    v: u32,
    enabled: bool,
}

pub struct Learning {
    pub enabled: bool,
    pub records: Vec<Record>,
    pub display: DisplayModel,
    pub timing: Timing,
    running: Option<Running>,
    last_end: f64,
    /// Whether learned settings are currently applied (on battery).
    overlay_on: Option<bool>,
    display_unsaved: usize,
    status: String,
    /// The power mode's own setting values when the overlay was last applied.
    tune_ctx: State,
}

impl Learning {
    pub fn load(state_dir: &Path, timing: Timing) -> Learning {
        let read = |name: &str| fs::read_to_string(state_dir.join(name)).ok();
        Learning {
            enabled: read("learning.json")
                .and_then(|t| serde_json::from_str::<SavedSwitch>(&t).ok())
                .is_some_and(|s| s.enabled),
            records: read("calibration.json")
                .and_then(|t| serde_json::from_str::<SavedRecords>(&t).ok())
                .map(|s| {
                    let mut r = s.records;
                    if s.v < 2 {
                        r.retain(|r| !tune::in_group(&r.setting));
                    }
                    r
                })
                .unwrap_or_default(),
            display: read("display.json")
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_else(DisplayModel::new),
            timing,
            running: None,
            last_end: f64::MIN,
            overlay_on: None,
            display_unsaved: 0,
            status: String::new(),
            tune_ctx: State::new(),
        }
    }

    /// What is learned: the latest kept result per setting tested on its
    /// own, plus the values tuned together for the current power mode, each
    /// as a record from its original value (`baseline`) to the learned one.
    pub fn learned(&self) -> Vec<Record> {
        let mut out: Vec<Record> = Vec::new();
        for r in self.records.iter().rev() {
            if !tune::in_group(&r.setting) && !out.iter().any(|o| o.setting == r.setting) {
                out.push(r.clone());
            }
        }
        out.retain(|r| r.verdict == Verdict::Keep);
        out.extend(self.tuned());
        out
    }

    /// The values tuned together that differ from the original, for the
    /// current power mode.
    fn tuned(&self) -> Vec<Record> {
        let (Some(best), Some(orig)) = (
            tune::learned(&self.records, &self.tune_ctx),
            tune::original(&self.records),
        ) else {
            return Vec::new();
        };
        let steps = tune::kept_steps(&self.records, &self.tune_ctx);
        let mut out = Vec::new();
        for (id, value) in &best {
            let Some(o) = orig.get(id).filter(|o| *o != value) else {
                continue;
            };
            let Some(last) = steps
                .iter()
                .rev()
                .find(|r| &r.setting == id)
                .or(steps.last())
            else {
                continue;
            };
            let mut r = (*last).clone();
            r.setting = id.clone();
            r.value = value.clone();
            r.baseline = o.clone();
            r.reason = format!(
                "tuned together with the other processor settings: {}",
                last.reason
            );
            out.push(r);
        }
        out
    }

    pub fn testing(&self) -> bool {
        self.running.is_some()
    }

    pub fn status(&self) -> String {
        if !self.enabled {
            "Learning is off".into()
        } else if let Some(r) = &self.running {
            let (b, n) = r.exp.progress();
            format!(
                "Testing {} = {}: step {b} of {n}",
                r.exp.plan.setting, r.exp.plan.value
            )
        } else if self.status.is_empty() {
            "Waiting for a quiet moment on battery".into()
        } else {
            self.status.clone()
        }
    }
}

impl Core {
    pub fn set_learning(&mut self, on: bool) -> Result<(), String> {
        self.learn.enabled = on;
        if !on {
            // Off means off: no tests, and learned settings back to original.
            self.stop_experiment("learning was switched off");
            for r in self.learn.learned() {
                if let Err(e) =
                    self.apply_value(&r.setting, &r.baseline, "learning switched off", None)
                {
                    eprintln!("dynamisd: {}: {e}", r.setting);
                }
            }
            self.learn.overlay_on = None;
        }
        write_atomic(
            &self.state_dir.join("learning.json"),
            &serde_json::to_string(&SavedSwitch {
                v: FORMAT_VERSION,
                enabled: on,
            })
            .map_err(|e| e.to_string())?,
        )
    }

    /// Forget what was learned about a setting and put its original value back.
    pub fn forget(&mut self, setting: &str) -> Result<bool, String> {
        let tuned_together = tune::in_group(setting);
        let learned: Vec<Record> = self
            .learn
            .learned()
            .into_iter()
            .filter(|r| r.setting == setting || (tuned_together && tune::in_group(&r.setting)))
            .collect();
        let before = self.learn.records.len();
        // Values tuned together depend on each other: forgetting one
        // forgets the set.
        self.learn
            .records
            .retain(|r| r.setting != setting && !(tuned_together && tune::in_group(&r.setting)));
        if before == self.learn.records.len() {
            return Ok(false);
        }
        for r in learned {
            self.apply_value(&r.setting, &r.baseline, "learned setting forgotten", None)?;
        }
        self.save_records()?;
        Ok(true)
    }

    /// One learning reading, with the same context auto mode uses.
    pub(crate) fn learn_tick(&mut self, now: f64, ctx: &Context, workload: Workload) {
        self.apply_overlay(ctx.on_ac);

        if let (Some(b), Some(w)) = (ctx.brightness_pct, ctx.power_w) {
            if self.learn.running.is_none()
                && matches!(workload, Workload::Idle | Workload::LightInteractive)
            {
                self.learn.display.add(b, w);
                self.learn.display_unsaved += 1;
                if self.learn.display_unsaved >= 20 {
                    self.learn.display_unsaved = 0;
                    if let Ok(t) = serde_json::to_string(&self.learn.display) {
                        let _ = write_atomic(&self.state_dir.join("display.json"), &t);
                    }
                }
            }
        }

        if !self.learn.enabled {
            return;
        }
        if self.learn.running.is_some() {
            self.continue_experiment(now, ctx, workload);
        } else if let Err(why) = self.preflight(now, ctx, workload) {
            self.learn.status = format!("Waiting: {why}");
        } else {
            self.start_experiment(now, ctx, workload);
        }
    }

    fn preflight(&self, now: f64, ctx: &Context, workload: Workload) -> Result<(), String> {
        if now - self.learn.last_end < self.learn.timing.cooldown_s {
            return Err("resting between tests".into());
        }
        if ctx.on_ac {
            return Err("tests run on battery".into());
        }
        if ctx.battery_pct.is_none_or(|b| b < MIN_BATTERY_PCT) {
            return Err(format!("battery below {MIN_BATTERY_PCT:.0}%"));
        }
        if ctx.power_w.is_none() {
            return Err("the battery does not report power use".into());
        }
        if ctx.max_temp_c.is_some_and(|t| t >= MAX_TEMP_C) {
            return Err("the laptop is warm".into());
        }
        if !matches!(workload, Workload::Idle | Workload::LightInteractive) {
            return Err(format!("{} right now", workload.describe()));
        }
        if !self.holds.is_empty() {
            return Err("an app is holding a power mode".into());
        }
        Ok(())
    }

    /// Current values of the settings tuned together and of the power
    /// mode's own settings (those this machine has).
    fn tune_state(&self) -> State {
        tune::GROUP
            .iter()
            .chain(tune::CONTEXT)
            .filter_map(|id| {
                let s = settings::by_id(id)?;
                let v = s.read(&self.root).ok()?;
                (v != "mixed").then(|| (id.to_string(), v))
            })
            .collect()
    }

    /// The next experiment, and the tuned values to record with it.
    fn next_plan(&self, engine: &dynamis_core::engine::Engine) -> Option<(Plan, State)> {
        let blocked = engine.blocked();
        let t = self.learn.timing;
        let plan = |setting: &str, baseline: String, value: String| Plan {
            setting: setting.into(),
            baseline,
            value,
            block_s: t.block_s,
            settle_s: t.settle_s,
            blocks: 8,
        };
        // A value this machine allows for a setting learning may change.
        let allowed = |id: &str, v: &str| -> Option<String> {
            let s = settings::by_id(id)?;
            if blocked.contains_key(id) || s.risk().needs_opt_in() {
                return None;
            }
            let Probe::Supported { allowed } = s.probe(&self.root) else {
                return None;
            };
            let v = allowed.check(v).ok()?;
            engine.quirks().blocking(id, &v).is_none().then_some(v)
        };

        // Phase 4: every setting on its own first.
        for s in settings::all() {
            let id = s.id();
            if tune::in_group(id) || blocked.contains_key(id) || s.risk().needs_opt_in() {
                continue;
            }
            let Probe::Supported { allowed } = s.probe(&self.root) else {
                continue;
            };
            let Ok(current) = s.read(&self.root) else {
                continue;
            };
            if current == "mixed" || self.learn.learned().iter().any(|r| r.setting == id) {
                continue;
            }
            for v in calibrate::candidates(id) {
                let Ok(v) = allowed.check(v) else { continue };
                if v == current || engine.quirks().blocking(id, &v).is_some() {
                    continue;
                }
                let tries: Vec<&Record> = self
                    .learn
                    .records
                    .iter()
                    .filter(|r| {
                        r.setting == id
                            && r.value == v
                            && r.context.power_source == PowerSource::Battery
                    })
                    .collect();
                let settled = tries.iter().any(|r| r.verdict != Verdict::Inconclusive);
                if settled || tries.len() >= MAX_TRIES {
                    continue;
                }
                return Some((plan(id, current.clone(), v), State::new()));
            }
        }

        // Phase 5: the processor settings that affect each other, together.
        let state = self.tune_state();
        let (id, v) = tune::next(&self.learn.records, &state, MAX_TRIES, |id, v| {
            allowed(id, v).is_some_and(|a| a == v)
        })?;
        let current = state.get(&id)?.clone();
        Some((plan(&id, current, v), tune::record_group(&state, &id)))
    }

    fn start_experiment(&mut self, now: f64, _ctx: &Context, workload: Workload) {
        let plan = match open_engine(&self.root, &self.state_dir) {
            Ok(e) => {
                if let Some(why) = e.safe_mode() {
                    self.learn.status = format!("Waiting: safe mode is on ({why})");
                    return;
                }
                if e.pending().is_some() {
                    self.learn.status = "Waiting: another trial is in progress".into();
                    return;
                }
                if e.dry_run() {
                    self.learn.status = "Waiting: dry-run mode is on, so nothing is tested".into();
                    return;
                }
                self.next_plan(&e)
            }
            Err(e) => {
                self.learn.status = format!("Waiting: {e}");
                return;
            }
        };
        let Some((plan, group)) = plan else {
            self.learn.status = "Every setting has been tested".into();
            return;
        };
        eprintln!(
            "dynamisd: learning: testing {} = {} (now {})",
            plan.setting, plan.value, plan.baseline
        );
        let context = RecordContext {
            power_source: PowerSource::Battery,
            workload,
        };
        self.learn.running = Some(Running {
            exp: Experiment::start(plan, Limits::default(), context, now),
            monitor: Monitor::start(&self.root, health::Limits::default()),
            workload,
            profile: self.active(),
            b_applied: false,
            group,
        });
    }

    fn continue_experiment(&mut self, now: f64, ctx: &Context, workload: Workload) {
        // Read these before borrowing the running experiment mutably.
        let active = self.active();
        let holds_empty = self.holds.is_empty();
        let Some(run) = self.learn.running.as_mut() else {
            return;
        };
        let problems = run.monitor.check();
        let changed = if ctx.on_ac {
            Some("the charger was plugged in".to_string())
        } else if workload != run.workload {
            Some(format!("{} started", workload.describe()))
        } else if active != run.profile {
            Some("the power mode changed".to_string())
        } else if !holds_empty {
            Some("an app asked for a power mode".to_string())
        } else {
            None
        };

        if !problems.is_empty() {
            let why: Vec<String> = problems.iter().map(|p| p.to_string()).collect();
            let mut rec = run.exp.abort(&why.join("; "), now);
            rec.verdict = Verdict::Reject;
            rec.reason = format!("caused a problem: {}", why.join("; "));
            rec.group = run.group.clone();
            let b_applied = run.b_applied;
            self.learn.running = None;
            if b_applied {
                if let Ok(mut e) = open_engine(&self.root, &self.state_dir) {
                    let _ = e
                        .revert_unhealthy(&format!("calibration health check: {}", why.join("; ")));
                }
            }
            self.finish(now, rec);
            return;
        }
        if let Some(why) = changed {
            self.stop_experiment(&why);
            self.learn.last_end = now;
            return;
        }

        let Some(power_w) = ctx.power_w else { return };
        let action = run.exp.observe(
            now,
            Sample {
                power_w,
                stall_pct: ctx.stall_pct,
            },
        );
        let setting = run.exp.plan.setting.clone();
        let (step, total) = run.exp.progress();
        match action {
            Action::Continue => {}
            Action::Apply(v) => {
                let to_b = v == run.exp.plan.value;
                let remaining = run.exp.plan.block_s * (total - step + 2) as f64;
                let result = open_engine(&self.root, &self.state_dir).and_then(|mut e| {
                    if to_b {
                        e.apply(&Request {
                            id: &setting,
                            value: &v,
                            allow_risky: false,
                            trial_secs: Some(remaining.max(60.0) as u64),
                            reason: &format!("calibration: step {step} of {total}"),
                        })
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                    } else {
                        e.revert(&format!("calibration: step {step} of {total}"))
                            .map_err(|e| e.to_string())
                    }
                });
                match result {
                    Ok(()) => {
                        if let Some(r) = self.learn.running.as_mut() {
                            r.b_applied = to_b;
                        }
                    }
                    Err(e) => self.stop_experiment(&format!("could not switch the setting: {e}")),
                }
            }
            Action::Finish(mut rec) => {
                if let Some(r) = self.learn.running.take() {
                    rec.group = r.group;
                }
                // Tuned together: the lag limit holds for the whole set.
                let limit = Limits::default().max_stall_increase_pct;
                if rec.verdict == Verdict::Keep && !rec.group.is_empty() {
                    let ctx = tune::context_of(&rec.group);
                    let total = tune::total_stall(&self.learn.records, &ctx, rec.stall.mean);
                    if total > limit {
                        rec.verdict = Verdict::Reject;
                        rec.reason = format!(
                            "together with the earlier changes, made the laptop less \
                             responsive (+{total:.1} points of stall)"
                        );
                    }
                }
                if let Ok(mut e) = open_engine(&self.root, &self.state_dir) {
                    let _ = if rec.verdict == Verdict::Keep {
                        e.keep()
                    } else {
                        e.revert(&format!("calibration: {}", rec.reason))
                    };
                }
                self.finish(now, *rec);
            }
        }
    }

    fn stop_experiment(&mut self, why: &str) {
        let Some(run) = self.learn.running.take() else {
            return;
        };
        if run.b_applied {
            if let Ok(mut e) = open_engine(&self.root, &self.state_dir) {
                let _ = e.revert(&format!("calibration stopped: {why}"));
            }
        }
        let rec = run.exp.abort(why, 0.0);
        eprintln!(
            "dynamisd: learning: stopped testing {} ({why})",
            rec.setting
        );
        self.learn.status = format!("Stopped a test: {why}");
    }

    fn finish(&mut self, now: f64, mut rec: Record) {
        rec.time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        eprintln!(
            "dynamisd: learning: {} = {}: {:?}, {}",
            rec.setting, rec.value, rec.verdict, rec.reason
        );
        self.learn.status = format!("Last test: {} = {}: {}", rec.setting, rec.value, rec.reason);
        if rec.verdict == Verdict::Keep {
            // It is applied now, on battery.
            self.learn.overlay_on = Some(true);
            if !rec.group.is_empty() {
                self.learn.tune_ctx = tune::context_of(&rec.group);
            }
        }
        self.learn.records.push(rec);
        self.learn.last_end = now;
        if let Err(e) = self.save_records() {
            eprintln!("dynamisd: {e}");
        }
    }

    /// Learned settings apply on battery only. Values tuned together are
    /// chosen for the power mode in use, so they are re-applied when it
    /// changes.
    fn apply_overlay(&mut self, on_ac: bool) {
        let want = !on_ac;
        if self.learn.running.is_some() {
            return;
        }
        let ctx = tune::context_of(&self.tune_state());
        if self.learn.overlay_on == Some(want) && ctx == self.learn.tune_ctx {
            return;
        }
        // Values tuned together: the best for this power mode on battery,
        // otherwise (mains, or nothing learned for this mode) the original.
        if let Some(orig) = tune::original(&self.learn.records) {
            let best = tune::learned(&self.learn.records, &ctx).filter(|_| want);
            for (id, o) in &orig {
                let (v, why) = match best.as_ref().and_then(|b| b.get(id)) {
                    Some(v) => (v.clone(), "learned: tuned together for this power mode"),
                    None if want => (o.clone(), "nothing tuned yet for this power mode"),
                    None => (
                        o.clone(),
                        "learned settings are for battery; original value on mains",
                    ),
                };
                if let Err(e) = self.apply_value(id, &v, why, None) {
                    eprintln!("dynamisd: tuned setting {id}: {e}");
                }
            }
        }
        self.learn.tune_ctx = ctx;
        let learned: Vec<Record> = self
            .learn
            .learned()
            .into_iter()
            .filter(|r| !tune::in_group(&r.setting))
            .collect();
        for r in &learned {
            let (value, why) = if want {
                (&r.value, format!("learned: {}", r.reason))
            } else {
                (
                    &r.baseline,
                    "learned settings are for battery; original value on mains".to_string(),
                )
            };
            if let Err(e) = self.apply_value(&r.setting, value, &why, None) {
                eprintln!("dynamisd: learned setting {}: {e}", r.setting);
            }
        }
        self.learn.overlay_on = Some(want);
    }

    fn apply_value(
        &self,
        id: &str,
        value: &str,
        reason: &str,
        trial: Option<u64>,
    ) -> Result<(), String> {
        let mut e = open_engine(&self.root, &self.state_dir)?;
        e.apply(&Request {
            id,
            value,
            allow_risky: false,
            trial_secs: trial,
            reason,
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    fn save_records(&self) -> Result<(), String> {
        let saved = SavedRecords {
            v: FORMAT_VERSION,
            records: self.learn.records.clone(),
        };
        write_atomic(
            &self.state_dir.join("calibration.json"),
            &serde_json::to_string_pretty(&saved).map_err(|e| e.to_string())?,
        )
    }
}
