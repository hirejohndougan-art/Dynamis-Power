//! "Learn now": a learning session the person starts by hand and leaves
//! the laptop alone for.
//!
//! Every test runs back to back (no rests between tests, no waiting for a
//! quiet moment beyond the checks that keep a measurement honest), to set a
//! baseline in a couple of hours instead of weeks. Learning stays on
//! afterwards and carries on in everyday use.
//!
//! - **Battery only**, from at least 40%.
//! - **Waits for the screen to turn off** (the session helper reports the
//!   person idle or the screen locked) before the first test, so every test
//!   is measured in the same state.
//! - **Idle plus a light, steady workload.** Settings tested on their own
//!   are measured idle. The settings whose cost is lag (turbo and the speed
//!   limit, tuned together) are measured with a built-in light load, so lag
//!   can be judged; the person can leave the load out, and those settings
//!   are then left for everyday learning.
//! - **Hands the laptop back** on any use of keyboard or mouse, plugging
//!   in, battery below 40%, warmth, a call or safe mode. Finished tests are
//!   kept; the rest run later.
//! - **Keeps the laptop awake** with a logind sleep inhibitor (taken by the
//!   reading thread, released when the session ends). The lid still puts it
//!   to sleep: logind ignores inhibitors for the lid by default.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dynamis_core::auto::{Context, Workload};
use dynamis_core::calibrate::Verdict;
use dynamis_core::settings;
use dynamis_core::tune;
use serde::Serialize;
use serde_json::{json, Value};

use crate::core::{open_engine, Core};
use crate::learn::{MAX_TEMP_C, MIN_BATTERY_PCT};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    WaitingScreen,
    Testing,
    Done,
    Stopped,
}

pub struct Session {
    pub phase: Phase,
    pub with_workload: bool,
    /// Records from this index on were made in this session.
    first_record: usize,
    pub stopped_reason: Option<String>,
    /// While waiting for the screen to turn off: why, for people.
    pub waiting: Option<String>,
    load: Option<LightLoad>,
    /// The logind sleep inhibitor, held while the session runs.
    pub inhibit: Option<zbus::OwnedFd>,
    inhibit_asked: bool,
}

impl Session {
    pub fn active(&self) -> bool {
        matches!(self.phase, Phase::WaitingScreen | Phase::Testing)
    }

    /// The reading thread should ask logind to keep the laptop awake.
    /// Asked once per session.
    pub fn take_inhibit_request(&mut self) -> bool {
        if self.active() && !self.inhibit_asked {
            self.inhibit_asked = true;
            true
        } else {
            false
        }
    }

    fn end(&mut self, phase: Phase, why: Option<String>) {
        self.phase = phase;
        self.stopped_reason = why;
        self.waiting = None;
        self.load = None;
        // Closing the descriptor releases the inhibitor.
        self.inhibit = None;
    }
}

/// A small, steady load: about 10% of one core (10 ms of work every
/// 100 ms), the same every time, so tests of settings that cost lag see
/// some work to be slow at. Stops within 100 ms of being dropped.
pub struct LightLoad {
    stop: Arc<AtomicBool>,
}

impl LightLoad {
    pub fn start() -> LightLoad {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let spawned = std::thread::Builder::new()
            .name("learn-now-load".into())
            .spawn(move || {
                let mut x: u64 = 1;
                while !flag.load(Ordering::Relaxed) {
                    let t = Instant::now();
                    while t.elapsed() < Duration::from_millis(10) {
                        x = x
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        std::hint::black_box(x);
                    }
                    std::thread::sleep(Duration::from_millis(90));
                }
            });
        if let Err(e) = spawned {
            eprintln!("dynamisd: learn now: no light load: {e}");
        }
        LightLoad { stop }
    }
}

impl Drop for LightLoad {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Core {
    /// Start Learn now. Refused, with a plain reason, when it cannot run.
    pub fn start_learn_session(&mut self, with_workload: bool) -> Result<(), String> {
        if self.learn.session.as_ref().is_some_and(|s| s.active()) {
            return Err("Learn now is already running".into());
        }
        let ctx = self.auto.sampler.read(&self.root, 0.0);
        if ctx.on_ac {
            return Err("Unplug the charger first: the tests run on battery.".into());
        }
        if ctx.battery_pct.is_none_or(|b| b < MIN_BATTERY_PCT) {
            return Err(format!(
                "Charge the battery to at least {MIN_BATTERY_PCT:.0}% first."
            ));
        }
        let engine = open_engine(&self.root, &self.state_dir)?;
        if let Some(why) = engine.safe_mode() {
            return Err(format!("Safe mode is on ({why}), so nothing is tested."));
        }
        if engine.dry_run() {
            return Err("Dry-run mode is on, so nothing is tested.".into());
        }
        drop(engine);
        self.stop_experiment("Learn now started");
        if !self.learn.enabled {
            self.set_learning(true)?;
        }
        self.learn.session = Some(Session {
            phase: Phase::WaitingScreen,
            with_workload,
            first_record: self.learn.records.len(),
            stopped_reason: None,
            waiting: None,
            load: None,
            inhibit: None,
            inhibit_asked: false,
        });
        self.learn.status = "Learn now: waiting for the screen to turn off".into();
        Ok(())
    }

    /// Stop Learn now (by hand, or because something changed). The test in
    /// progress is undone; finished tests are kept.
    pub fn stop_learn_session(&mut self, why: &str) {
        if !self.learn.session.as_ref().is_some_and(|s| s.active()) {
            return;
        }
        self.stop_experiment(why);
        if let Some(s) = self.learn.session.as_mut() {
            s.end(Phase::Stopped, Some(why.to_string()));
        }
        self.learn.status = format!("Learn now stopped: {why}");
        eprintln!("dynamisd: learn now: stopped ({why})");
    }

    /// One reading during Learn now (from `learn_tick`).
    pub(crate) fn session_tick(&mut self, now: f64, ctx: &Context, workload: Workload) {
        let stop = if ctx.on_ac {
            Some("the charger was plugged in".to_string())
        } else if ctx.battery_pct.is_none_or(|b| b < MIN_BATTERY_PCT) {
            Some(format!("the battery went below {MIN_BATTERY_PCT:.0}%"))
        } else if ctx.max_temp_c.is_some_and(|t| t >= MAX_TEMP_C) {
            Some("the laptop got warm".to_string())
        } else if workload == Workload::Call {
            Some("a call started".to_string())
        } else {
            None
        };
        if let Some(why) = stop {
            self.stop_learn_session(&why);
            return;
        }
        // The person is away: the screen went off (idle) or is locked.
        let away = ctx.user_idle || ctx.screen_locked;
        let agent = self.auto.session_owner.is_some();
        let Some(phase) = self.learn.session.as_ref().map(|s| s.phase) else {
            return;
        };
        match phase {
            Phase::WaitingScreen if !away => {
                let why = if agent {
                    "Waiting for the screen to turn off. Leave the laptop alone."
                } else {
                    "Waiting for the screen to turn off. The session helper is not \
                     running, so Dynamis Power cannot tell yet; log out and in again \
                     to start it."
                };
                if let Some(s) = self.learn.session.as_mut() {
                    s.waiting = Some(why.into());
                }
                self.learn.status = format!("Learn now: {}", why.to_lowercase());
                return;
            }
            Phase::WaitingScreen => {
                if let Some(s) = self.learn.session.as_mut() {
                    s.phase = Phase::Testing;
                    s.waiting = None;
                }
            }
            Phase::Testing if !away => {
                self.stop_learn_session("you used the laptop");
                return;
            }
            Phase::Testing => {}
            Phase::Done | Phase::Stopped => return,
        }

        let with_tune =
            self.learn.session.as_ref().is_some_and(|s| s.with_workload) && !self.learn.relief;
        if self.learn.running.is_some() {
            self.continue_experiment(now, ctx, workload);
        } else {
            let nothing_left = match open_engine(&self.root, &self.state_dir) {
                Ok(e) => {
                    if let Some(why) = e.safe_mode() {
                        drop(e);
                        self.stop_learn_session(&format!("safe mode is on ({why})"));
                        return;
                    }
                    self.next_plan(&e, with_tune, dynamis_core::activity::Activity::Everyday)
                        .is_none()
                }
                Err(e) => {
                    self.learn.status = format!("Learn now: waiting: {e}");
                    return;
                }
            };
            if nothing_left {
                if let Some(s) = self.learn.session.as_mut() {
                    s.end(Phase::Done, None);
                }
                self.learn.status =
                    "Learn now finished; learning carries on in everyday use".into();
                eprintln!("dynamisd: learn now: finished");
                return;
            }
            match self.preflight(now, ctx, workload, true) {
                Err((key, why)) => {
                    self.learn.note_wait(key, now);
                    self.learn.status = format!("Learn now: waiting: {why}");
                }
                Ok(()) => self.start_experiment(now, ctx, workload, with_tune),
            }
        }
        // The light load runs during the tests of settings that cost lag.
        let want_load = with_tune
            && self
                .learn
                .running
                .as_ref()
                .is_some_and(|r| tune::in_group(&r.exp.plan.setting));
        if let Some(s) = self.learn.session.as_mut() {
            match (want_load, s.load.is_some()) {
                (true, false) => s.load = Some(LightLoad::start()),
                (false, true) => s.load = None,
                _ => {}
            }
        }
    }

    /// For the app: the session's phase, progress and results (JSON).
    pub fn learn_session_json(&self) -> Value {
        let Some(s) = &self.learn.session else {
            return json!({
                "phase": "idle", "total": 0, "done": 0, "current": null,
                "minutes_left": null, "results": [], "stopped_reason": null,
                "saves_w": 0.0, "waiting": null, "with_workload": true, "agent_missing": false,
            });
        };
        let label = |id: &str| {
            settings::by_id(id)
                .map(|s| s.label())
                .unwrap_or(id)
                .to_string()
        };
        let start = s.first_record.min(self.learn.records.len());
        let mine = &self.learn.records[start..];
        let under_load = |id: &str| s.with_workload && tune::in_group(id);
        let results: Vec<Value> = mine
            .iter()
            .map(|r| {
                json!({
                    "setting": r.setting,
                    "label": label(&r.setting),
                    "value": r.value,
                    "verdict": match r.verdict {
                        Verdict::Keep => "keep",
                        Verdict::Reject => "reject",
                        Verdict::Inconclusive => "unclear",
                    },
                    "reason": r.reason,
                    "saves_w": (r.verdict == Verdict::Keep).then(|| -r.effect.mean),
                    "under_load": under_load(&r.setting),
                })
            })
            .collect();
        let saves: f64 = mine
            .iter()
            .filter(|r| r.verdict == Verdict::Keep)
            .map(|r| -r.effect.mean)
            .sum();
        let left = if s.active() {
            self.learning_progress().map_or(0, |p| {
                p.singles_left + if s.with_workload { p.together_left } else { 0 }
            })
        } else {
            0
        };
        let per_test_min = self.learn.block_s() * 8.0 / 60.0;
        let current = self.learn.running.as_ref().map(|r| {
            json!({
                "setting": r.exp.plan.setting,
                "label": label(&r.exp.plan.setting),
                "value": r.exp.plan.value,
                "under_load": under_load(&r.exp.plan.setting),
            })
        });
        json!({
            "phase": s.phase,
            "total": results.len() + left,
            "done": results.len(),
            "current": current,
            "minutes_left": s.active().then(|| (left as f64 * per_test_min).ceil()),
            "results": results,
            "stopped_reason": s.stopped_reason,
            "saves_w": (saves * 100.0).round() / 100.0,
            "waiting": s.waiting,
            "agent_missing": s.phase == Phase::WaitingScreen && self.auto.session_owner.is_none(),
            "with_workload": s.with_workload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_load_stops_when_dropped() {
        let load = LightLoad::start();
        let flag = load.stop.clone();
        drop(load);
        assert!(flag.load(Ordering::Relaxed));
    }
}
