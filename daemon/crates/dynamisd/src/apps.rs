//! Apps (phase 6) inside the daemon: per-app energy, and giving apps a
//! bigger or smaller share of the CPU.
//!
//! The share is the cgroup `cpu.weight` of the app's own group. It only
//! matters when apps compete for the CPU, and it is never a cap: an app with a
//! small share still gets all the CPU nobody else wants. So the worst a wrong
//! choice can do is make a background app slower while something else is
//! busy. Every change goes through the engine (journaled, put back by
//! `restore`), and only groups under `user.slice` can be touched.
//!
//! Defaults: energy is counted; nothing is slowed or boosted until the person
//! switches it on.

use std::collections::BTreeMap;
use std::path::Path;

use dynamis_core::apps::{self, Meter, Proc, SYSTEM};
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{open_engine, write_atomic};

const POLICY_VERSION: u32 = 1;
const POLICY_FILE: &str = "apps-policy.json";
const METER_FILE: &str = "apps.json";
/// How often the per-day totals are saved.
const SAVE_EVERY_S: f64 = 600.0;

/// Shares are relative to the normal one, 100.
/// The app in front, when "give the app in front priority" is on.
pub const WEIGHT_FRONT: u32 = 400;
/// Background apps on battery, and apps marked "always slow".
pub const WEIGHT_SLOW: u32 = 25;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppPolicy {
    pub v: u32,
    /// Count energy per app.
    pub track: bool,
    /// On battery, give apps that are not in front a smaller share.
    pub slow_background: bool,
    /// Give the app in front a bigger share.
    pub boost_front: bool,
    /// Never given a smaller share.
    pub never_slow: Vec<String>,
    /// Always given a smaller share (on battery and plugged in).
    pub always_slow: Vec<String>,
}

impl Default for AppPolicy {
    fn default() -> Self {
        AppPolicy {
            v: POLICY_VERSION,
            track: true,
            slow_background: false,
            boost_front: false,
            never_slow: Vec::new(),
            always_slow: Vec::new(),
        }
    }
}

impl AppPolicy {
    /// Whether any share is changed at all.
    pub fn changes_shares(&self) -> bool {
        self.slow_background || self.boost_front || !self.always_slow.is_empty()
    }

    fn check(&self) -> Result<(), String> {
        for name in self.never_slow.iter().chain(&self.always_slow) {
            if name.is_empty() || name.len() > 200 || name == SYSTEM {
                return Err(format!("'{name}' is not an app name"));
            }
        }
        if let Some(n) = self
            .never_slow
            .iter()
            .find(|n| self.always_slow.contains(n))
        {
            return Err(format!("'{n}' cannot be both never and always slowed"));
        }
        if self.never_slow.len() + self.always_slow.len() > 500 {
            return Err("too many apps listed".into());
        }
        Ok(())
    }
}

/// The share each app group should have now, for groups that should not be
/// normal: cgroup → (app, weight). Pure, so it is easy to test.
pub fn wanted(
    policy: &AppPolicy,
    groups: &BTreeMap<String, Vec<String>>,
    on_ac: bool,
    front: Option<&str>,
) -> BTreeMap<String, (String, u32)> {
    let mut out = BTreeMap::new();
    for (app, cgroups) in groups {
        if app == SYSTEM || policy.never_slow.contains(app) {
            continue;
        }
        let weight = if policy.always_slow.contains(app) {
            WEIGHT_SLOW
        } else if policy.boost_front && front == Some(app.as_str()) {
            WEIGHT_FRONT
        } else if policy.slow_background
            && !on_ac
            // Only when the app in front is known: otherwise every app
            // would count as "in the background".
            && front.is_some_and(|f| f != app)
        {
            WEIGHT_SLOW
        } else {
            continue;
        };
        for cg in cgroups {
            out.insert(cg.clone(), (app.clone(), weight));
        }
    }
    out
}

#[derive(Default)]
pub struct AppsState {
    pub policy: AppPolicy,
    pub meter: Meter,
    /// What this run has set: cgroup → (app, weight).
    applied: BTreeMap<String, (String, u32)>,
    last_save: Option<f64>,
    /// The app in front, from the last reading.
    pub front: Option<String>,
    /// Logged once, so a refused change (safe mode) does not fill the log.
    complained: bool,
    /// "Last until" at its strongest level slows background apps on
    /// battery, whatever the switch says. Not saved: it ends with the target.
    pub forced_slow: bool,
    /// "It feels slow": background apps are not slowed today (unless "Last
    /// until" needs it).
    pub relief: bool,
}

impl AppsState {
    pub fn load(state_dir: &Path) -> AppsState {
        let policy = std::fs::read_to_string(state_dir.join(POLICY_FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<AppPolicy>(&t).ok())
            .unwrap_or_default();
        let meter = std::fs::read_to_string(state_dir.join(METER_FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Meter>(&t).ok())
            .unwrap_or_default();
        AppsState {
            policy,
            meter,
            ..AppsState::default()
        }
    }

    pub fn needs_ticks(&self) -> bool {
        self.policy.track || self.effective().changes_shares()
    }

    /// The policy in force: the person's, plus background slowing while
    /// "Last until" needs it.
    fn effective(&self) -> AppPolicy {
        AppPolicy {
            slow_background: (self.policy.slow_background && !self.relief) || self.forced_slow,
            ..self.policy.clone()
        }
    }

    /// Read processes and count energy. Afterwards `meter.running()` and
    /// `front` say which apps are open and which is in front.
    pub fn read(&mut self, root: &SysRoot, now_unix: f64, power_w: Option<f64>, front_pid: u32) {
        let procs: Vec<Proc> = apps::scan(root);
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        self.meter.add(&procs, now_unix, power_w, cores);
        if !self.policy.track {
            self.meter.forget();
        }
        self.front = (front_pid != 0)
            .then(|| procs.iter().find(|p| p.pid == front_pid))
            .flatten()
            .map(|p| p.app.clone())
            .filter(|a| a != SYSTEM);
    }

    /// Bring every app group's share to what the policy wants. Opens the
    /// engine only when something has to change.
    pub fn apply(&mut self, root: &SysRoot, state_dir: &Path, on_ac: bool) {
        let policy = self.effective();
        let want = if policy.changes_shares() {
            wanted(&policy, &self.meter.cgroups, on_ac, self.front.as_deref())
        } else {
            BTreeMap::new()
        };
        // Groups that closed are gone; nothing to put back.
        let open: Vec<&String> = self.meter.cgroups.values().flatten().collect();
        self.applied.retain(|cg, _| open.contains(&cg));
        if want == self.applied {
            return;
        }
        let mut engine = match open_engine(root, state_dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("dynamisd: apps: {e}");
                return;
            }
        };
        let back: Vec<(String, String)> = self
            .applied
            .iter()
            .filter(|(cg, _)| !want.contains_key(*cg))
            .map(|(cg, (app, _))| (cg.clone(), app.clone()))
            .collect();
        for (cg, app) in back {
            if let Err(e) = engine.reset_app_weight(&app, &cg, "back to a normal share") {
                eprintln!("dynamisd: apps: {app}: {e}");
            }
            self.applied.remove(&cg);
        }
        for (cg, (app, w)) in &want {
            if self.applied.get(cg).map(|a| a.1) == Some(*w) {
                continue;
            }
            let why = match *w {
                WEIGHT_FRONT => "the app in front gets priority",
                _ if self.policy.always_slow.contains(app) => "you chose to always slow this app",
                _ => "in the background on battery",
            };
            match engine.set_app_weight(app, cg, *w, why) {
                Ok(_) => {
                    self.applied.insert(cg.clone(), (app.clone(), *w));
                    self.complained = false;
                }
                Err(e) => {
                    if !self.complained {
                        eprintln!("dynamisd: apps: {app}: {e}");
                        self.complained = true;
                    }
                }
            }
        }
    }

    /// Forget what this run believes it has set, so the next reading sets
    /// every share again (after sleep, or when groups may be new).
    pub fn reassert(&mut self) {
        self.applied.clear();
    }

    /// Save the per-day totals every few minutes (and when `force`).
    pub fn maybe_save(&mut self, state_dir: &Path, now_unix: f64, force: bool) {
        if !self.policy.track && !force {
            return;
        }
        if !force && self.last_save.is_some_and(|t| now_unix - t < SAVE_EVERY_S) {
            return;
        }
        self.last_save = Some(now_unix);
        match serde_json::to_string(&self.meter) {
            Ok(t) => {
                if let Err(e) = write_atomic(&state_dir.join(METER_FILE), &t) {
                    eprintln!("dynamisd: {e}");
                }
            }
            Err(e) => eprintln!("dynamisd: apps: {e}"),
        }
    }

    pub fn set_policy(
        &mut self,
        p: AppPolicy,
        root: &SysRoot,
        state_dir: &Path,
        on_ac: bool,
    ) -> Result<(), String> {
        p.check()?;
        let p = AppPolicy {
            v: POLICY_VERSION,
            ..p
        };
        write_atomic(
            &state_dir.join(POLICY_FILE),
            &serde_json::to_string(&p).map_err(|e| e.to_string())?,
        )?;
        self.policy = p;
        self.apply(root, state_dir, on_ac);
        Ok(())
    }

    /// "Restore original settings": stop giving apps other shares. Turns
    /// both switches off and empties the "always slow" list; the engine's
    /// restore then puts every share back.
    pub fn stop_changing_shares(&mut self, state_dir: &Path) -> Result<(), String> {
        let p = AppPolicy {
            slow_background: false,
            boost_front: false,
            always_slow: Vec::new(),
            ..self.policy.clone()
        };
        write_atomic(
            &state_dir.join(POLICY_FILE),
            &serde_json::to_string(&p).map_err(|e| e.to_string())?,
        )?;
        self.policy = p;
        self.applied.clear();
        Ok(())
    }

    /// Forget every per-day total (the person asked).
    pub fn forget_usage(&mut self, state_dir: &Path, now_unix: f64) {
        self.meter.forget();
        self.maybe_save(state_dir, now_unix, true);
    }

    /// Everything the Apps screen shows.
    pub fn json(&self, now_unix: f64, rules: &[dynamis_core::auto::AppRule]) -> Value {
        let totals = |days| {
            self.meter
                .totals(days, now_unix)
                .into_iter()
                // Older totals may still hold Dynamis Power's own app.
                .filter(|(app, _)| !dynamis_core::apps::is_own(app))
                .map(|(app, u)| json!({"app": app, "cpu_s": u.cpu_s, "wh": u.wh}))
                .collect::<Vec<_>>()
        };
        let now: Vec<Value> = self
            .meter
            .last
            .iter()
            .map(|n| {
                let share = self
                    .applied
                    .values()
                    .find(|(a, _)| *a == n.app)
                    .map(|(_, w)| *w);
                json!({"app": n.app, "cpu_pct": n.cpu_pct, "watts": n.watts, "share": share, "wakeups_per_s": n.wakeups_per_s})
            })
            .collect();
        json!({
            "policy": self.policy,
            "rules": rules,
            "front": self.front,
            "running": self.meter.running(),
            "now": now,
            "today": totals(1),
            "week": totals(7),
            // Apps costing more (or less) than last month on battery.
            "trends": self
                .meter
                .trends(now_unix)
                .into_iter()
                .filter(|t| !dynamis_core::apps::is_own(&t.app))
                .collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups() -> BTreeMap<String, Vec<String>> {
        let mut g = BTreeMap::new();
        for a in ["firefox", "thunderbird", "steam"] {
            g.insert(a.to_string(), vec![format!("/user.slice/app-{a}.scope")]);
        }
        g
    }

    #[test]
    fn nothing_changes_by_default() {
        let p = AppPolicy::default();
        assert!(wanted(&p, &groups(), false, Some("firefox")).is_empty());
        assert!(!p.changes_shares());
    }

    #[test]
    fn background_is_slowed_only_on_battery_and_only_when_front_is_known() {
        let p = AppPolicy {
            slow_background: true,
            never_slow: vec!["steam".into()],
            ..AppPolicy::default()
        };
        let w = wanted(&p, &groups(), false, Some("firefox"));
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w["/user.slice/app-thunderbird.scope"].1, WEIGHT_SLOW);
        assert!(wanted(&p, &groups(), true, Some("firefox")).is_empty());
        assert!(wanted(&p, &groups(), false, None).is_empty());
    }

    #[test]
    fn front_boost_and_always_slow() {
        let p = AppPolicy {
            boost_front: true,
            always_slow: vec!["steam".into()],
            ..AppPolicy::default()
        };
        let w = wanted(&p, &groups(), true, Some("firefox"));
        assert_eq!(w["/user.slice/app-firefox.scope"].1, WEIGHT_FRONT);
        assert_eq!(w["/user.slice/app-steam.scope"].1, WEIGHT_SLOW);
        assert!(!w.contains_key("/user.slice/app-thunderbird.scope"));
    }

    #[test]
    fn policy_is_checked() {
        let both = AppPolicy {
            never_slow: vec!["a".into()],
            always_slow: vec!["a".into()],
            ..AppPolicy::default()
        };
        assert!(both.check().is_err());
        let system = AppPolicy {
            always_slow: vec![SYSTEM.into()],
            ..AppPolicy::default()
        };
        assert!(system.check().is_err());
    }
}
