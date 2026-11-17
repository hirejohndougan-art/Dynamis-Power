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

use std::collections::BTreeMap;
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
pub(crate) const MIN_BATTERY_PCT: f64 = 40.0;
pub(crate) const MAX_TEMP_C: f64 = 80.0;

/// Below this share of the whole processor, busier interactive use still
/// counts as quiet for learning: 120% of one core, so one busy core on a
/// two-core chip (50%) is quiet, and between 40% and 80% of the chip.
pub(crate) fn busy_limit(cores: usize) -> f64 {
    (120.0 / cores.max(1) as f64).clamp(40.0, 80.0)
}

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

/// One single-setting test learning can run here.
struct SingleTest {
    setting: &'static str,
    current: String,
    value: String,
    done: bool,
}

/// How far learning has got (for the app).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    /// Whole percent, rounded down so it reads 100 only when done.
    pub pct: f64,
    pub done: usize,
    pub left: usize,
    /// "single", "together" or "done".
    pub stage: &'static str,
    /// Quiet time on battery the tests left need, roughly.
    pub hours_left: f64,
    /// Of `left`: settings tested on their own, and steps tuned together.
    #[serde(skip)]
    pub singles_left: usize,
    #[serde(skip)]
    pub together_left: usize,
}

pub(crate) struct Running {
    pub(crate) exp: Experiment,
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
    /// "Keep it this way" after "It feels slow": keep only changes with no
    /// measurable lag at all.
    #[serde(default)]
    strict: bool,
}

pub struct Learning {
    pub enabled: bool,
    /// Keep only changes with no measurable lag (see `SavedSwitch`).
    pub strict: bool,
    /// "It feels slow" is in force: speed-related learned values are not
    /// applied and not tested.
    pub(crate) relief: bool,
    /// `relief` when the learned values were last applied.
    overlay_relief: bool,
    pub records: Vec<Record>,
    pub display: DisplayModel,
    pub timing: Timing,
    pub(crate) running: Option<Running>,
    last_end: f64,
    /// "Learn now": a session started by hand (`crate::learnnow`).
    pub(crate) session: Option<crate::learnnow::Session>,
    /// Whether learned settings are currently applied (on battery).
    overlay_on: Option<bool>,
    display_unsaved: usize,
    pub(crate) status: String,
    /// The power mode's own setting values when the overlay was last applied.
    tune_ctx: State,
    /// Seconds spent waiting, per reason, fading over about a day.
    waits: BTreeMap<&'static str, f64>,
    last_tick: Option<f64>,
    /// How often the battery's power reading actually changes.
    power_seen: Option<(f64, f64)>,
    power_gaps: Vec<f64>,
    /// Power readings over about the last block, to judge whether power use
    /// is steady (decision 0020).
    recent_power: std::collections::VecDeque<(f64, f64)>,
    /// The kernel and firmware running now (everyday problem §6).
    pub(crate) system: dynamis_core::sysid::SystemId,
    /// Learned settings whose control is gone, already noted in Activity.
    vanished: std::collections::BTreeSet<String>,
}

impl Learning {
    pub fn load(state_dir: &Path, timing: Timing) -> Learning {
        let read = |name: &str| fs::read_to_string(state_dir.join(name)).ok();
        Learning {
            enabled: read("learning.json")
                .and_then(|t| serde_json::from_str::<SavedSwitch>(&t).ok())
                .is_some_and(|s| s.enabled),
            strict: read("learning.json")
                .and_then(|t| serde_json::from_str::<SavedSwitch>(&t).ok())
                .is_some_and(|s| s.strict),
            relief: false,
            overlay_relief: false,
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
            session: None,
            last_end: f64::MIN,
            overlay_on: None,
            display_unsaved: 0,
            status: String::new(),
            tune_ctx: State::new(),
            waits: BTreeMap::new(),
            last_tick: None,
            power_seen: None,
            power_gaps: Vec::new(),
            recent_power: std::collections::VecDeque::new(),
            system: dynamis_core::sysid::SystemId::default(),
            vanished: std::collections::BTreeSet::new(),
        }
    }

    /// What is learned: the latest kept result per setting tested on its
    /// own, plus the values tuned together for the current power mode, each
    /// as a record from its original value (`baseline`) to the learned one.
    pub fn learned(&self) -> Vec<Record> {
        let mut out: Vec<Record> = Vec::new();
        // The latest settled result per setting: an inconclusive re-check
        // leaves the earlier result in place.
        for r in self.records.iter().rev() {
            if r.verdict != Verdict::Inconclusive
                && !tune::in_group(&r.setting)
                && !out.iter().any(|o| o.setting == r.setting)
            {
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

    /// Learned settings to measure again (everyday problem §6): measured on
    /// another kernel or firmware, or more than a few months ago, and not
    /// yet re-tested enough times since. Tested on their own, from the
    /// original value to the learned one. With why, for the app.
    pub fn rechecks(&self, now_s: u64) -> Vec<(Record, String)> {
        let mut out = Vec::new();
        for r in self.learned() {
            if tune::in_group(&r.setting) || !r.stale(&self.system, now_s) {
                continue;
            }
            let since = self
                .records
                .iter()
                .filter(|o| o.setting == r.setting && o.time > r.time)
                .count();
            if since >= MAX_TRIES {
                continue;
            }
            let why = match r.system.as_ref().and_then(|s| self.system.change_from(s)) {
                Some(change) => format!("after a system update ({change})"),
                None => "a few months on".to_string(),
            };
            out.push((r, why));
        }
        out
    }

    /// Apply learned settings again at the next reading (after sleep).
    pub fn reapply_soon(&mut self) {
        self.overlay_on = None;
    }

    /// The main reason learning has been waiting over about the last day
    /// (rests between tests do not count), with its share of the waiting.
    pub fn main_wait(&self) -> Option<(&'static str, f64)> {
        let total: f64 = self.waits.values().sum();
        self.waits
            .iter()
            .filter(|(k, _)| **k != "resting")
            .max_by(|a, b| a.1.total_cmp(b.1))
            .filter(|(_, v)| **v > 0.0)
            .map(|(k, v)| (*k, (v / total.max(1e-9) * 100.0).round()))
    }

    /// How often the battery's power reading changes (median of the last
    /// gaps), once a few have been seen.
    pub fn power_update_s(&self) -> Option<f64> {
        if self.power_gaps.len() < 3 {
            return None;
        }
        let mut g = self.power_gaps.clone();
        g.sort_by(f64::total_cmp);
        Some(g[g.len() / 2])
    }

    /// Test block length: the configured one, or four of the battery's own
    /// update periods if that is longer, so slow-reporting batteries still
    /// give several real readings per block.
    pub(crate) fn block_s(&self) -> f64 {
        self.timing
            .block_s
            .max(self.power_update_s().unwrap_or(0.0) * 4.0)
    }

    pub(crate) fn note_wait(&mut self, reason: &'static str, now: f64) {
        let dt = self.last_tick.map_or(0.0, |t| (now - t).clamp(0.0, 3600.0));
        let fade = (-dt / 86_400.0).exp();
        for v in self.waits.values_mut() {
            *v *= fade;
        }
        *self.waits.entry(reason).or_default() += dt;
    }

    fn note_power(&mut self, power_w: Option<f64>, now: f64) {
        let Some(w) = power_w else {
            self.power_seen = None;
            self.recent_power.clear();
            return;
        };
        let window = self.steady_window_s();
        self.recent_power.push_back((now, w));
        while self
            .recent_power
            .front()
            .is_some_and(|(t, _)| now - t > window)
        {
            self.recent_power.pop_front();
        }
        match self.power_seen {
            Some((pw, pt)) if (pw - w).abs() > 1e-6 => {
                self.power_gaps.push(now - pt);
                if self.power_gaps.len() > 20 {
                    self.power_gaps.remove(0);
                }
                self.power_seen = Some((w, now));
            }
            None => self.power_seen = Some((w, now)),
            _ => {}
        }
    }

    /// How long power must have been steady: one test block (a minute by
    /// default).
    fn steady_window_s(&self) -> f64 {
        self.timing.block_s.min(60.0)
    }

    /// Whether power use has been steady over the last window (decision
    /// 0020): its spread is small next to its level. `None` when that
    /// cannot be told (too few readings, or a battery that updates its
    /// reading too slowly), and the processor check alone is used.
    pub(crate) fn steady(&self, now: f64) -> Option<bool> {
        let window = self.steady_window_s();
        if self.power_update_s().is_some_and(|u| u > window / 4.0) {
            return None;
        }
        let n = self.recent_power.len();
        let first = self.recent_power.front()?.0;
        if n < 4 || now - first < window * 0.75 {
            return None;
        }
        let mean = self.recent_power.iter().map(|(_, w)| w).sum::<f64>() / n as f64;
        let var = self
            .recent_power
            .iter()
            .map(|(_, w)| (w - mean).powi(2))
            .sum::<f64>()
            / n as f64;
        Some(var.sqrt() <= (0.12 * mean).max(0.25))
    }

    /// The limits a test is judged by: no measurable lag at all when
    /// strict.
    pub(crate) fn limits(&self) -> Limits {
        if self.strict {
            Limits {
                max_stall_increase_pct: 0.0,
                ..Limits::default()
            }
        } else {
            Limits::default()
        }
    }

    pub fn testing(&self) -> bool {
        self.running.is_some()
    }

    pub fn status(&self) -> String {
        if !self.enabled {
            "Learning is off".into()
        } else if let Some(r) = &self.running {
            let (b, n) = r.exp.progress();
            let now_s = crate::core::unix_now() as u64;
            match self
                .rechecks(now_s)
                .into_iter()
                .find(|(rec, _)| rec.setting == r.exp.plan.setting)
            {
                Some((_, why)) => {
                    format!("Re-checking {} {why}: step {b} of {n}", r.exp.plan.setting)
                }
                None => format!(
                    "Testing {} = {}: step {b} of {n}",
                    r.exp.plan.setting, r.exp.plan.value
                ),
            }
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
            self.stop_learn_session("learning was switched off");
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
                strict: self.learn.strict,
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
        // Where the values tuned together go back to (the mode's own value
        // where the mode sets them); worked out before the records go.
        let base = if tuned_together {
            self.tune_base()
        } else {
            None
        };
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
            let to = base
                .as_ref()
                .and_then(|b| b.get(&r.setting))
                .unwrap_or(&r.baseline);
            self.apply_value(&r.setting, to, "learned setting forgotten", None)?;
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
        self.learn.note_power(ctx.power_w, now);
        if self.learn.session.as_ref().is_some_and(|s| s.active()) {
            self.session_tick(now, ctx, workload);
            self.learn.last_tick = Some(now);
            return;
        }
        if self.learn.running.is_some() {
            self.continue_experiment(now, ctx, workload);
        } else if let Err((key, why)) = self.preflight(now, ctx, workload, false) {
            self.learn.note_wait(key, now);
            self.learn.status = format!("Waiting: {why}");
        } else {
            let with_tune = !self.learn.relief;
            self.start_experiment(now, ctx, workload, with_tune);
        }
        self.learn.last_tick = Some(now);
    }

    /// Whether a test may start now; if not, a reason key (counted, and
    /// turned into advice in the app) and a line for people.
    /// In a Learn now session (`session`) there are no rests between tests.
    pub(crate) fn preflight(
        &self,
        now: f64,
        ctx: &Context,
        workload: Workload,
        session: bool,
    ) -> Result<(), (&'static str, String)> {
        if self.trouble.waiting(crate::core::unix_now() as u64) {
            return Err((
                "trouble",
                "you said something stopped working; tests wait for your answer".into(),
            ));
        }
        if !session && now - self.learn.last_end < self.learn.timing.cooldown_s {
            return Err(("resting", "resting between tests".into()));
        }
        if ctx.on_ac {
            return Err(("plugged_in", "tests run on battery".into()));
        }
        if ctx.battery_pct.is_none_or(|b| b < MIN_BATTERY_PCT) {
            return Err((
                "low_battery",
                format!("battery below {MIN_BATTERY_PCT:.0}%"),
            ));
        }
        if ctx.power_w.is_none() {
            return Err(if ctx.energy_wh.is_some() {
                (
                    "power_coming",
                    "working out power use from the battery level".into(),
                )
            } else {
                (
                    "no_power",
                    "this battery reports neither power use nor energy left, so learning cannot measure here".into(),
                )
            });
        }
        if ctx.max_temp_c.is_some_and(|t| t >= MAX_TEMP_C) {
            return Err(("warm", "the laptop is warm".into()));
        }
        if !self.quiet_enough(workload, ctx) {
            return Err(("busy", format!("{} right now", workload.describe())));
        }
        // Light work on a slow laptop counts only while power is steady.
        match self.learn.steady(now) {
            Some(false) => {
                return Err(("unsteady", "power use is changing".into()));
            }
            None if workload == Workload::BurstyInteractive => {
                return Err(("busy", format!("{} right now", workload.describe())));
            }
            _ => {}
        }
        if !self.holds.is_empty() {
            return Err(("app_hold", "an app is holding a power mode".into()));
        }
        Ok(())
    }

    /// The processor check (decision 0020): idle and light use are quiet;
    /// so is busier interactive use below a limit that grows as the number
    /// of cores falls (one busy core on a two-core chip is half the chip).
    fn quiet_enough(&self, workload: Workload, ctx: &Context) -> bool {
        match workload {
            Workload::Idle | Workload::LightInteractive => true,
            Workload::BurstyInteractive => {
                ctx.cpu_busy_pct < busy_limit(dynamis_core::sensors::cpu::count(&self.root))
            }
            _ => false,
        }
    }

    /// Current values of the settings tuned together and of the power
    /// mode's own settings (those this machine has).
    fn tune_state(&self) -> State {
        let mut state: State = tune::GROUP
            .iter()
            .chain(tune::CONTEXT)
            .filter_map(|id| {
                let s = settings::by_id(id)?;
                let v = s.read(&self.root).ok()?;
                (v != "mixed").then(|| (id.to_string(), v))
            })
            .collect();
        if dynamis_core::profiles::uses_fallback(&self.root) {
            state.insert("mode".into(), self.active().as_str().into());
        }
        state
    }

    /// Where the values tuned together go back to when nothing is learned
    /// for this mode, or on mains: what the mode itself sets, if it sets
    /// them (laptops without a speed preference), else the values from
    /// before any tuning.
    fn tune_base(&self) -> Option<State> {
        let mut base = tune::original(&self.learn.records)?;
        for step in
            dynamis_core::profiles::plan_with(&self.root, self.active(), &self.profile_overrides)
        {
            if tune::in_group(step.id) {
                base.insert(step.id.to_string(), step.value);
            }
        }
        Some(base)
    }

    /// The next experiment, and the tuned values to record with it.
    /// Without `with_tune`, only settings tested on their own.
    pub(crate) fn next_plan(
        &self,
        engine: &dynamis_core::engine::Engine,
        with_tune: bool,
    ) -> Option<(Plan, State)> {
        let blocked = engine.blocked();
        let t = self.learn.timing;
        let plan = |setting: &str, baseline: String, value: String| Plan {
            setting: setting.into(),
            baseline,
            value,
            block_s: self.learn.block_s(),
            // Long blocks for a slow battery: let the reading catch up too.
            settle_s: t.settle_s.max(self.learn.power_update_s().unwrap_or(0.0)),
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

        // Re-checks come first (everyday problem §6): the learned value
        // against the original, if both are still allowed here.
        let now_s = crate::core::unix_now() as u64;
        for (r, _) in self.learn.rechecks(now_s) {
            let (Some(base), Some(v)) = (
                allowed(&r.setting, &r.baseline),
                allowed(&r.setting, &r.value),
            ) else {
                continue;
            };
            if base != v {
                return Some((plan(&r.setting, base, v), State::new()));
            }
        }

        // Phase 4: every setting on its own first.
        if let Some(p) = self.single_tests(engine).into_iter().find(|p| !p.done) {
            return Some((plan(p.setting, p.current, p.value), State::new()));
        }
        if !with_tune {
            return None;
        }

        // Phase 5: the processor settings that affect each other, together.
        let state = self.tune_state();
        let (id, v) = tune::next(&self.learn.records, &state, MAX_TRIES, |id, v| {
            allowed(id, v).is_some_and(|a| a == v)
        })?;
        let current = state.get(&id)?.clone();
        Some((plan(&id, current, v), tune::record_group(&state, &id)))
    }

    /// Every single-setting test this machine allows, and whether it is
    /// done (settled, tried enough times, or the setting already learned).
    fn single_tests(&self, engine: &dynamis_core::engine::Engine) -> Vec<SingleTest> {
        let blocked = engine.blocked();
        let learned = self.learn.learned();
        let mut out = Vec::new();
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
            if current == "mixed" {
                continue;
            }
            let is_learned = learned.iter().any(|r| r.setting == id);
            for v in calibrate::candidates(id) {
                let Ok(v) = allowed.check(v) else { continue };
                if engine.quirks().blocking(id, &v).is_some() {
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
                // The value it already has needs no test, unless that is
                // only because learning set it.
                let nothing_to_test = v == current && !is_learned;
                if nothing_to_test {
                    continue;
                }
                out.push(SingleTest {
                    setting: id,
                    current: current.clone(),
                    value: v,
                    done: is_learned || settled || tries.len() >= MAX_TRIES,
                });
            }
        }
        out
    }

    /// How far learning has got: tests done and left, as a percentage.
    /// The tests together (phase 5) cannot all be known in advance: when a
    /// step is kept, the steps from the new values are added, so the
    /// percentage can step back a little. The app says it is an estimate.
    pub fn learning_progress(&self) -> Option<Progress> {
        let engine = open_engine(&self.root, &self.state_dir).ok()?;
        let singles = self.single_tests(&engine);
        let singles_done = singles.iter().filter(|t| t.done).count();
        let singles_left = singles.len() - singles_done;
        let blocked = engine.blocked();
        let allowed = |id: &str, v: &str| -> bool {
            let Some(s) = settings::by_id(id) else {
                return false;
            };
            if blocked.contains_key(id) || s.risk().needs_opt_in() {
                return false;
            }
            let Probe::Supported { allowed } = s.probe(&self.root) else {
                return false;
            };
            allowed.check(v).is_ok_and(|a| a == v) && engine.quirks().blocking(id, v).is_none()
        };
        let state = self.tune_state();
        let together_left = tune::remaining(&self.learn.records, &state, MAX_TRIES, allowed).len();
        let together_done = self
            .learn
            .records
            .iter()
            .filter(|r| tune::in_group(&r.setting) && !r.group.is_empty())
            .count();
        // Before the tests together start, count the first steps they will
        // take from here, so the total does not jump when they begin.
        let done = singles_done + together_done;
        let left = singles_left + together_left;
        let total = done + left;
        let pct = if total == 0 {
            100.0
        } else {
            (done as f64 / total as f64 * 100.0).floor()
        };
        // One test: its blocks, the settling at each switch, then the rest
        // before the next.
        let t = self.learn.timing;
        let per_test_s = self.learn.block_s() * 8.0 + t.cooldown_s;
        Some(Progress {
            pct,
            done,
            left,
            stage: if singles_left > 0 {
                "single"
            } else if together_left > 0 {
                "together"
            } else {
                "done"
            },
            hours_left: left as f64 * per_test_s / 3600.0,
            singles_left,
            together_left,
        })
    }

    pub(crate) fn start_experiment(
        &mut self,
        now: f64,
        _ctx: &Context,
        workload: Workload,
        with_tune: bool,
    ) {
        let plan = match open_engine(&self.root, &self.state_dir) {
            Ok(e) => {
                if let Some(why) = e.safe_mode() {
                    self.learn.status = format!("Waiting: safe mode is on ({why})");
                    self.learn.note_wait("safe_mode", now);
                    return;
                }
                if e.pending().is_some() {
                    self.learn.status = "Waiting: another trial is in progress".into();
                    self.learn.note_wait("trial", now);
                    return;
                }
                if e.dry_run() {
                    self.learn.status = "Waiting: dry-run mode is on, so nothing is tested".into();
                    self.learn.note_wait("dry_run", now);
                    return;
                }
                self.next_plan(&e, with_tune)
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
            exp: Experiment::start(plan, self.learn.limits(), context, now),
            monitor: Monitor::start(&self.root, health::Limits::default()),
            workload,
            profile: self.active(),
            b_applied: false,
            group,
        });
    }

    pub(crate) fn continue_experiment(&mut self, now: f64, ctx: &Context, workload: Workload) {
        // Read these before borrowing the running experiment mutably.
        let active = self.active();
        let holds_empty = self.holds.is_empty();
        // Moving between quiet kinds of use (idle, light, light work on a
        // slow laptop) does not end a test: A and B blocks alternate, so a
        // slow change cancels out. Anything busier does.
        let still_quiet = self.quiet_enough(workload, ctx);
        let Some(run) = self.learn.running.as_mut() else {
            return;
        };
        let problems = run.monitor.check();
        let changed = if ctx.on_ac {
            Some("the charger was plugged in".to_string())
        } else if workload != run.workload && !still_quiet {
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
                let limit = self.learn.limits().max_stall_increase_pct;
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

    /// Stop a test in progress (from outside learning: sleep, for one).
    pub(crate) fn stop_experiment_for(&mut self, why: &str) {
        self.stop_experiment(why);
    }

    pub(crate) fn stop_experiment(&mut self, why: &str) {
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
        rec.system = Some(self.learn.system.clone());
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
    pub(crate) fn apply_overlay(&mut self, on_ac: bool) {
        let want = !on_ac;
        if self.learn.running.is_some() {
            return;
        }
        let ctx = tune::context_of(&self.tune_state());
        let relief = self.learn.relief;
        if self.learn.overlay_on == Some(want)
            && ctx == self.learn.tune_ctx
            && relief == self.learn.overlay_relief
        {
            return;
        }
        self.learn.overlay_relief = relief;
        // Values tuned together: the best for this power mode on battery,
        // otherwise (mains, or nothing learned for this mode) the original.
        if let Some(orig) = self.tune_base() {
            // "It feels slow": the mode's own values, not the learned ones.
            let best = tune::learned(&self.learn.records, &ctx).filter(|_| want && !relief);
            for (id, o) in &orig {
                let (v, why) = match best.as_ref().and_then(|b| b.get(id)) {
                    Some(v) => (v.clone(), "learned: tuned together for this power mode"),
                    None if want && relief => {
                        (o.clone(), "it feels slow: the mode's own value today")
                    }
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
            // A control gone after an update: dropped from what is applied,
            // with one line in Activity.
            let gone = settings::by_id(&r.setting)
                .is_none_or(|s| matches!(s.probe(&self.root), Probe::Unsupported { .. }));
            if gone {
                if self.learn.vanished.insert(r.setting.clone()) {
                    if let Ok(e) = open_engine(&self.root, &self.state_dir) {
                        let _ = e.note(
                            "vanished",
                            &r.setting,
                            "this laptop no longer offers this setting (after a system update?); the learned value is no longer applied",
                        );
                    }
                }
                continue;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn learning() -> Learning {
        let dir = std::env::temp_dir().join(format!("dynamis-learn-unit-{}", std::process::id()));
        Learning::load(&dir, Timing::default())
    }

    fn rec(verdict: Verdict, time: u64, system: Option<&str>) -> Record {
        let e = dynamis_core::calibrate::Effect {
            mean: -0.2,
            ci_low: -0.3,
            ci_high: -0.1,
            confidence: 1.0,
        };
        Record {
            setting: "kernel.nmi_watchdog".into(),
            value: "off".into(),
            baseline: "on".into(),
            context: dynamis_core::calibrate::RecordContext {
                power_source: PowerSource::Battery,
                workload: Workload::LightInteractive,
            },
            metric: "system_watts".into(),
            effect: e,
            stall: e,
            baseline_w: 6.0,
            samples: 4,
            verdict,
            reason: String::new(),
            time,
            group: Default::default(),
            system: system.map(|k| dynamis_core::sysid::SystemId {
                kernel: k.into(),
                firmware: "X 01/01/2025".into(),
            }),
        }
    }

    #[test]
    fn results_from_another_kernel_or_long_ago_are_rechecked() {
        let now = 1_800_000_000;
        let mut l = learning();
        l.system = dynamis_core::sysid::SystemId {
            kernel: "6.11.0".into(),
            firmware: "X 01/01/2025".into(),
        };
        // Same kernel, recent: nothing to re-check.
        l.records = vec![rec(Verdict::Keep, now - 86_400, Some("6.11.0"))];
        assert!(l.rechecks(now).is_empty());
        // Older results without a system: re-checked only by age.
        l.records = vec![rec(Verdict::Keep, now - 86_400, None)];
        assert!(l.rechecks(now).is_empty());
        l.records = vec![rec(Verdict::Keep, now - 200 * 86_400, None)];
        assert_eq!(l.rechecks(now)[0].1, "a few months on");
        // Measured on another kernel: re-checked, saying why.
        l.records = vec![rec(Verdict::Keep, now - 86_400, Some("6.8.0"))];
        let r = l.rechecks(now);
        assert_eq!(r.len(), 1);
        assert!(r[0].1.contains("kernel 6.8.0 → 6.11.0"), "{}", r[0].1);
        // An inconclusive re-check keeps the learned value and tries again,
        // up to the limit.
        for i in 1..=MAX_TRIES as u64 {
            l.records
                .push(rec(Verdict::Inconclusive, now - 86_400 + i, Some("6.11.0")));
            assert_eq!(l.learned().len(), 1, "still learned");
        }
        assert!(l.rechecks(now).is_empty(), "tried enough");
        // A re-check that no longer saves power: not learned any more.
        l.records.push(rec(Verdict::Reject, now, Some("6.11.0")));
        assert!(l.learned().is_empty());
        // Kept again on the new kernel: learned, nothing to re-check.
        l.records.push(rec(Verdict::Keep, now, Some("6.11.0")));
        assert_eq!(l.learned().len(), 1);
        assert!(l.rechecks(now).is_empty());
    }

    #[test]
    fn busy_limit_grows_as_cores_fall() {
        assert_eq!(busy_limit(1), 80.0);
        assert_eq!(busy_limit(2), 60.0);
        assert_eq!(busy_limit(4), 40.0);
        assert_eq!(busy_limit(16), 40.0);
    }

    #[test]
    fn steady_power_needs_a_full_window_and_a_small_spread() {
        let mut l = learning();
        l.timing.block_s = 60.0;
        // Not enough yet.
        l.note_power(Some(6.0), 0.0);
        assert_eq!(l.steady(0.0), None);
        for (i, w) in [6.1, 5.9, 6.0, 6.05, 5.95, 6.0].iter().enumerate() {
            l.note_power(Some(*w), 10.0 * (i + 1) as f64);
        }
        assert_eq!(l.steady(60.0), Some(true));
        // A jump: not steady.
        for (i, w) in [9.0, 4.0, 9.5, 3.5, 9.0, 4.0].iter().enumerate() {
            l.note_power(Some(*w), 70.0 + 10.0 * i as f64);
        }
        assert_eq!(l.steady(120.0), Some(false));
        // Gone: cannot tell.
        l.note_power(None, 130.0);
        assert_eq!(l.steady(130.0), None);
    }

    #[test]
    fn the_main_reason_for_waiting_leaves_out_rests() {
        let mut l = learning();
        assert_eq!(l.main_wait(), None);
        let mut t = 0.0;
        for (reason, ticks) in [("resting", 100), ("plugged_in", 60), ("busy", 20)] {
            for _ in 0..ticks {
                l.note_wait(reason, t);
                l.last_tick = Some(t);
                t += 60.0;
            }
        }
        let (main, share) = l.main_wait().unwrap();
        assert_eq!(main, "plugged_in");
        // About 60 of 180 minutes, the older ones faded a little.
        assert!((25.0..40.0).contains(&share), "{share}");
    }

    #[test]
    fn slow_batteries_get_longer_blocks() {
        let mut l = learning();
        assert_eq!(l.block_s(), 60.0);
        // Power changes every 30 s, read every 5 s.
        let mut t = 0.0;
        for i in 0..40 {
            l.note_power(Some(5.0 + (i / 6) as f64 * 0.1), t);
            t += 5.0;
        }
        assert_eq!(l.power_update_s(), Some(30.0));
        assert_eq!(l.block_s(), 120.0);
        // A fast battery leaves the configured length alone.
        let mut fast = learning();
        for i in 0..10 {
            fast.note_power(Some(i as f64), i as f64 * 5.0);
        }
        assert_eq!(fast.power_update_s(), Some(5.0));
        assert_eq!(fast.block_s(), 60.0);
    }
}
