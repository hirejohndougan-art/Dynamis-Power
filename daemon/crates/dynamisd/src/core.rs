//! The daemon's state, with no D-Bus in it so it can be tested directly.
//!
//! - `chosen` is the profile the user picked (saved across reboots).
//! - Holds are temporary requests from apps ("performance while I compile",
//!   "power-saver while the battery is low"). A power-saver hold wins over a
//!   performance hold, and any hold wins over the user's choice.
//! - The active profile is what is applied to the machine.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use dynamis_core::auto::{Auto, Context, Prefs, Step, Workload};
use dynamis_core::context::Sampler;
use dynamis_core::engine::{Engine, EngineError};
use dynamis_core::profiles::{self, Profile};
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};

use crate::apps::AppsState;
use crate::kept::KeptState;
use crate::learn::{Learning, Timing};
use crate::predict::PredictState;

/// Version of `profile.json`.
const PROFILE_FILE_VERSION: u32 = 1;
/// Version of `auto.json`.
const AUTO_FILE_VERSION: u32 = 1;

/// What the session agent last reported. Cleared when the agent leaves the bus.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionReport {
    pub media_playing: bool,
    pub screen_inhibited: bool,
    pub game_running: bool,
    pub screen_locked: bool,
    pub user_idle: bool,
    /// Process ID of the window in front (0: unknown). Only used to tell
    /// which app is in front; never stored.
    pub focused_pid: u32,
}

#[derive(Serialize, Deserialize)]
struct SavedAuto {
    v: u32,
    enabled: bool,
    #[serde(default)]
    prefs: Prefs,
}

/// Auto mode's state inside the daemon.
#[derive(Default)]
pub struct AutoState {
    pub enabled: bool,
    rules: Auto,
    sampler: Sampler,
    session: SessionReport,
    session_owner: Option<String>,
    /// Set when a person picked a profile by hand while auto mode was on:
    /// auto mode then waits until the charger is plugged in or out (the value
    /// here is the charger state at the time).
    paused_on_ac: Option<bool>,
    last: Option<Step>,
    /// Classifies the workload for learning even while auto mode is off.
    observer: Auto,
    observed: Option<Workload>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hold {
    pub profile: Profile,
    pub reason: String,
    pub application_id: String,
    /// D-Bus unique name of the caller; the hold ends when it disconnects.
    pub owner: String,
}

pub struct Core {
    pub(crate) root: SysRoot,
    pub(crate) state_dir: PathBuf,
    available: Vec<Profile>,
    chosen: Profile,
    pub(crate) holds: BTreeMap<u32, Hold>,
    next_cookie: u32,
    pub auto: AutoState,
    pub learn: Learning,
    pub apps: AppsState,
    pub predict: PredictState,
    /// The person's own choices for what each profile sets (Advanced).
    pub profile_overrides: profiles::Overrides,
    /// Settings set by hand that are applied again after restarts and sleep.
    pub kept: KeptState,
}

#[derive(Serialize, Deserialize)]
struct SavedProfile {
    v: u32,
    profile: String,
}

impl Core {
    /// Load state and recover from anything an earlier run left unfinished.
    /// Does not apply anything yet; call [`Core::apply_active`] for that.
    #[cfg(test)]
    pub fn new(root: SysRoot, state_dir: PathBuf) -> Core {
        Self::with_timing(root, state_dir, Timing::default())
    }

    pub fn with_timing(root: SysRoot, state_dir: PathBuf, timing: Timing) -> Core {
        let available = profiles::available(&root);
        let saved = fs::read_to_string(state_dir.join("profile.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<SavedProfile>(&t).ok())
            .and_then(|s| Profile::parse(&s.profile))
            .filter(|p| available.contains(p));
        let saved_auto = fs::read_to_string(state_dir.join("auto.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<SavedAuto>(&t).ok());
        let learn = Learning::load(&state_dir, timing);
        let apps = AppsState::load(&state_dir);
        let predict = PredictState::load(&state_dir);
        let kept = KeptState::load(&state_dir, unix_now());
        let profile_overrides = fs::read_to_string(state_dir.join("profiles.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<profiles::Overrides>(&t).ok())
            .unwrap_or_default();
        let auto = AutoState {
            enabled: saved_auto.as_ref().is_some_and(|a| a.enabled),
            rules: Auto::new(saved_auto.map(|a| a.prefs).unwrap_or_default()),
            ..AutoState::default()
        };
        Core {
            root,
            state_dir,
            available,
            chosen: saved.unwrap_or(Profile::Balanced),
            holds: BTreeMap::new(),
            next_cookie: 1,
            auto,
            learn,
            apps,
            predict,
            profile_overrides,
            kept,
        }
    }

    pub fn available(&self) -> &[Profile] {
        &self.available
    }

    pub fn root(&self) -> &SysRoot {
        &self.root
    }

    pub fn holds(&self) -> &BTreeMap<u32, Hold> {
        &self.holds
    }

    pub fn active(&self) -> Profile {
        let held = |p| self.holds.values().any(|h| h.profile == p);
        let p = if held(Profile::PowerSaver) {
            Profile::PowerSaver
        } else if held(Profile::Performance) {
            Profile::Performance
        } else {
            self.chosen
        };
        // "Last until" is the person's own explicit goal: it caps everything,
        // app requests included.
        match self.predict.level().cap() {
            Some(cap) if rank(p) > rank(cap) && self.available.contains(&cap) => cap,
            _ => p,
        }
    }

    /// The user picked a profile. Releases every hold (as the standard
    /// interface requires) and returns their cookies.
    pub fn choose(&mut self, p: Profile) -> Result<Vec<u32>, String> {
        if !self.available.contains(&p) {
            return Err(format!("'{}' is not available on this machine", p.as_str()));
        }
        let before = (self.chosen, std::mem::take(&mut self.holds));
        self.chosen = p;
        if let Err(e) = self.apply_active(&format!("profile {} chosen", p.as_str())) {
            // Nothing applied: keep the old state so what we report stays true.
            (self.chosen, self.holds) = before;
            return Err(e);
        }
        self.save()?;
        if self.auto.enabled {
            let on_ac = self.auto.sampler.read(&self.root, 0.0).on_ac;
            self.auto.paused_on_ac = Some(on_ac);
        }
        Ok(before.1.into_keys().collect())
    }

    pub fn hold(
        &mut self,
        p: Profile,
        reason: &str,
        app: &str,
        owner: &str,
    ) -> Result<u32, String> {
        if p == Profile::Balanced {
            return Err("only power-saver and performance can be held".into());
        }
        if !self.available.contains(&p) {
            return Err(format!("'{}' is not available on this machine", p.as_str()));
        }
        let cookie = self.next_cookie;
        self.next_cookie = self.next_cookie.wrapping_add(1).max(1);
        self.holds.insert(
            cookie,
            Hold {
                profile: p,
                reason: reason.into(),
                application_id: app.into(),
                owner: owner.into(),
            },
        );
        // A failed apply still records the hold: the app asked, and the next
        // successful apply honours it. The error is logged.
        if let Err(e) = self.apply_active(&format!("{app} asked for {}: {reason}", p.as_str())) {
            eprintln!("dynamisd: hold {cookie}: {e}");
        }
        Ok(cookie)
    }

    pub fn release(&mut self, cookie: u32, caller: &str) -> Result<(), String> {
        match self.holds.get(&cookie) {
            None => Err(format!("no hold with cookie {cookie}")),
            Some(h) if h.owner != caller => {
                Err("only the app that made a hold can release it".into())
            }
            Some(_) => {
                self.holds.remove(&cookie);
                self.reapply_after_release();
                Ok(())
            }
        }
    }

    /// An app left the bus: drop its holds. Returns their cookies.
    pub fn release_owner(&mut self, owner: &str) -> Vec<u32> {
        let gone: Vec<u32> = self
            .holds
            .iter()
            .filter(|(_, h)| h.owner == owner)
            .map(|(c, _)| *c)
            .collect();
        if !gone.is_empty() {
            self.holds.retain(|_, h| h.owner != owner);
            self.reapply_after_release();
        }
        gone
    }

    fn reapply_after_release(&mut self) {
        if let Err(e) = self.apply_active("hold released") {
            eprintln!("dynamisd: {e}");
        }
    }

    /// Apply the active profile. Settings already at the right value are
    /// left untouched (the engine reads first and writes nothing), so this is
    /// cheap to repeat and stays correct after `dynamis restore`. Fails without
    /// changing anything in safe mode, or when every step fails.
    pub fn apply_active(&self, reason: &str) -> Result<(), String> {
        let target = self.active();
        let mut engine = open_engine(&self.root, &self.state_dir)?;
        if let Some(why) = engine.safe_mode() {
            return Err(format!("safe mode is on ({why}); profiles are not applied"));
        }
        let results = profiles::apply_with(
            &mut engine,
            &self.root,
            target,
            &self.profile_overrides,
            reason,
        );
        let failures: Vec<String> = results
            .iter()
            .filter_map(|a| {
                a.result
                    .as_ref()
                    .err()
                    .map(|e| format!("{}: {e}", a.step.id))
            })
            .collect();
        for f in &failures {
            eprintln!("dynamisd: {} profile, {f}", target.as_str());
        }
        if !results.is_empty() && failures.len() == results.len() {
            return Err(format!(
                "could not apply {}: {}",
                target.as_str(),
                failures.join("; ")
            ));
        }
        Ok(())
    }

    // --- Auto mode ---------------------------------------------------------

    /// Switch auto mode on or off, and remember the choice.
    pub fn set_auto(&mut self, on: bool, now: f64) -> Result<(), String> {
        self.auto.enabled = on;
        self.auto.paused_on_ac = None;
        if on {
            // Start from what is active, so the first decision is a real one.
            self.auto.rules = Auto::new(self.auto.rules.prefs.clone());
            self.auto.rules.set_current(self.active(), now - 3600.0);
        }
        let saved = SavedAuto {
            v: AUTO_FILE_VERSION,
            enabled: on,
            prefs: self.auto.rules.prefs.clone(),
        };
        write_atomic(
            &self.state_dir.join("auto.json"),
            &serde_json::to_string(&saved).map_err(|e| e.to_string())?,
        )
    }

    pub fn auto_prefs(&self) -> &Prefs {
        &self.auto.rules.prefs
    }

    /// Change auto mode's preferences (Modes screen). Values are checked and
    /// refused when out of range, rather than silently clamped.
    pub fn set_auto_prefs(&mut self, p: Prefs, now: f64) -> Result<(), String> {
        let within = |v: f64, lo: f64, hi: f64, what: &str| {
            if (lo..=hi).contains(&v) {
                Ok(())
            } else {
                Err(format!("{what} must be between {lo} and {hi}"))
            }
        };
        within(p.low_battery_pct, 10.0, 60.0, "low battery level")?;
        within(
            p.critical_battery_pct,
            5.0,
            p.low_battery_pct,
            "very low battery level",
        )?;
        within(p.hot_c, 60.0, 100.0, "hot temperature")?;
        within(p.busy_start_pct, 30.0, 100.0, "busy level")?;
        within(p.busy_end_pct, 5.0, p.busy_start_pct, "busy end level")?;
        within(p.busy_start_s, 5.0, 600.0, "busy time")?;
        within(p.min_dwell_s, 10.0, 3600.0, "time between switches")?;
        within(
            p.max_switches_per_hour as f64,
            1.0,
            60.0,
            "switches an hour",
        )?;
        if p.app_rules.len() > 100 {
            return Err("at most 100 app modes".into());
        }
        if p.app_rules
            .iter()
            .any(|r| r.app.is_empty() || r.app.len() > 200)
        {
            return Err("an app mode has no app name".into());
        }
        for prof in [p.on_ac, p.on_battery]
            .into_iter()
            .chain(p.app_rules.iter().map(|r| r.profile))
        {
            if !self.available.contains(&prof) {
                return Err(format!(
                    "'{}' is not available on this machine",
                    prof.as_str()
                ));
            }
        }
        self.auto.rules.prefs = p.clone();
        self.auto.observer.prefs = p;
        let enabled = self.auto.enabled;
        // Saving goes through set_auto so the file format stays in one place.
        self.set_auto(enabled, now)
    }

    /// Change how apps share the CPU (Apps screen).
    pub fn set_app_policy(&mut self, p: crate::apps::AppPolicy) -> Result<(), String> {
        let on_ac = self.auto.sampler.read(&self.root, 0.0).on_ac;
        let (root, dir) = (self.root.clone(), self.state_dir.clone());
        self.apps.set_policy(p, &root, &dir, on_ac)
    }

    /// The laptop woke from sleep: firmware may have reset settings, a test
    /// in progress is no longer valid, and devices may be new. Apply the
    /// mode, kept settings, learned settings and app shares again.
    pub fn after_resume(&mut self) {
        self.stop_experiment_for("the laptop slept");
        if let Err(e) = self.apply_active("woke from sleep") {
            eprintln!("dynamisd: {e}");
        }
        let (root, dir) = (self.root.clone(), self.state_dir.clone());
        self.kept
            .apply_all(&root, &dir, false, "after waking from sleep");
        self.learn.reapply_soon();
        self.apps.reassert();
    }

    pub fn report_session(&mut self, owner: &str, report: SessionReport) {
        self.auto.session = report;
        self.auto.session_owner = Some(owner.to_string());
    }

    /// The session agent left the bus: forget what it said.
    pub fn session_gone(&mut self, owner: &str) {
        if self.auto.session_owner.as_deref() == Some(owner) {
            self.auto.session = SessionReport::default();
            self.auto.session_owner = None;
        }
    }

    /// Whether anything needs regular readings (auto mode, learning or apps).
    pub fn needs_ticks(&self) -> bool {
        self.auto.enabled
            || self.learn.enabled
            || self.apps.needs_ticks()
            || self.predict.needs_ticks()
    }

    /// Whether processes need reading this tick.
    fn needs_apps(&self) -> bool {
        self.apps.needs_ticks()
            || self.apps.forced_slow
            || (self.auto.enabled && !self.auto.rules.prefs.app_rules.is_empty())
    }

    /// One reading, shared by auto mode and learning. Returns true if the
    /// active profile changed.
    pub fn tick(&mut self, now: f64) -> bool {
        if !self.needs_ticks() {
            return false;
        }
        let mut ctx: Context = self.auto.sampler.read(&self.root, now);
        let s = &self.auto.session;
        ctx.media_playing = s.media_playing;
        ctx.screen_inhibited = s.screen_inhibited;
        ctx.game_running = s.game_running;
        ctx.screen_locked = s.screen_locked;
        ctx.user_idle = s.user_idle;
        let front_pid = s.focused_pid;
        // Prediction first: "Last until" can change what is allowed.
        let unix = unix_now();
        let (root, dir) = (self.root.clone(), self.state_dir.clone());
        self.kept.tick(&root, &dir, unix);
        let mut target_changed = false;
        if self
            .predict
            .tick(&root, &dir, unix, ctx.on_ac, ctx.power_w, ctx.energy_wh)
        {
            let before = self.active();
            let why = self
                .predict
                .last
                .as_ref()
                .map_or("Last until ended".to_string(), |s| {
                    format!("last until: {}", s.reason)
                });
            if let Err(e) = self.apply_active(&why) {
                eprintln!("dynamisd: {e}");
            }
            target_changed = self.active() != before;
        }
        self.apps.forced_slow = self.predict.level() == dynamis_core::predict::Level::SaveMore;
        if self.needs_apps() {
            let unix = unix_now();
            self.apps.read(&self.root, unix, ctx.power_w, front_pid);
            ctx.running_apps = self.apps.meter.running();
            ctx.focused_app = self.apps.front.clone();
            self.apps.apply(&self.root, &self.state_dir, ctx.on_ac);
            self.apps.maybe_save(&self.state_dir, unix, false);
        }
        let workload = self
            .auto
            .observer
            .step(now, &ctx, &self.available)
            .decision
            .workload;
        self.auto.observed = Some(workload);

        let changed = (self.auto.enabled && self.auto_step(now, &ctx)) || target_changed;
        if self.learn.enabled {
            self.learn_tick(now, &ctx, workload);
        }
        changed
    }

    fn auto_step(&mut self, now: f64, ctx: &Context) -> bool {
        let ctx = ctx.clone();
        if let Some(was) = self.auto.paused_on_ac {
            if was == ctx.on_ac {
                // Keep reading so the explanation stays current, but do not act.
                let mut probe = self.auto.rules.clone();
                self.auto.last = Some(probe.step(now, &ctx, &self.available));
                return false;
            }
            self.auto.paused_on_ac = None;
            self.auto.rules.set_current(self.chosen, now - 3600.0);
        }

        let step = self.auto.rules.step(now, &ctx, &self.available);
        let switch = step.switch_to.filter(|p| *p != self.chosen);
        self.auto.last = Some(step.clone());
        let Some(p) = switch else { return false };
        let before = self.active();
        self.chosen = p;
        if let Err(e) = self.apply_active(&format!("auto: {}", step.decision.reason)) {
            eprintln!("dynamisd: auto mode: {e}");
        }
        self.active() != before
    }

    /// The workload auto mode sees, and one line on what it is doing.
    pub fn auto_status(&self) -> (Option<Workload>, String) {
        if !self.auto.enabled {
            return (None, "Auto mode is off".into());
        }
        let Some(last) = &self.auto.last else {
            return (None, "Auto mode is starting".into());
        };
        let mut line = last.decision.reason.clone();
        if self.auto.paused_on_ac.is_some() {
            line = "You picked a mode by hand; auto mode resumes when the charger is plugged in or out".into();
        } else if !self.holds.is_empty() {
            line = format!("{line} (an app is holding {})", self.active().as_str());
        } else if let Some(w) = &last.waiting {
            line = format!("{line} ({w})");
        }
        (Some(last.decision.workload), line)
    }

    /// How long to wait before the next reading: longer while idle, to cost
    /// less when nothing is happening.
    pub fn auto_interval(&self, base: Duration) -> Duration {
        if self.learn.testing() {
            // Several readings per test block.
            return base.min(Duration::from_secs_f64(self.learn.timing.block_s / 6.0));
        }
        let workload = match &self.auto.last {
            Some(s) if self.auto.enabled => Some(s.decision.workload),
            _ => self.auto.observed,
        };
        match workload {
            Some(Workload::Idle) if !self.learn.enabled => base * 4,
            _ => base,
        }
    }

    fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string(&SavedProfile {
            v: PROFILE_FILE_VERSION,
            profile: self.chosen.as_str().into(),
        })
        .map_err(|e| e.to_string())?;
        write_atomic(&self.state_dir.join("profile.json"), &text)
    }
}

/// Slowest first.
fn rank(p: Profile) -> u8 {
    match p {
        Profile::PowerSaver => 0,
        Profile::Balanced => 1,
        Profile::Performance => 2,
    }
}

pub(crate) fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    };
    write().map_err(|e| format!("could not save {}: {e}", path.display()))
}

/// Open the engine, waiting briefly if `dynamis` on the command line is
/// changing a setting at the same moment.
pub fn open_engine(root: &SysRoot, state_dir: &Path) -> Result<Engine, String> {
    for _ in 0..40 {
        match Engine::open(root.clone(), state_dir) {
            Ok(engine) => {
                if let Some(r) = engine.recovered() {
                    eprintln!(
                        "dynamisd: a change to {} did not finish last time; put back to {} ({}) and blocked",
                        r.id,
                        r.restored_to,
                        if r.ok { "ok" } else { "FAILED" }
                    );
                }
                return Ok(engine);
            }
            Err(EngineError::Locked(_)) => sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("another Dynamis Power process kept the settings lock for 2 seconds".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A copy of the fixture with a firmware profile added, so every profile
    /// changes something.
    fn sandbox(name: &str) -> (PathBuf, Core) {
        let dir = std::env::temp_dir().join(format!("dynamisd-core-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/sysfs/synthetic-laptop");
        let ok = std::process::Command::new("cp")
            .arg("-a")
            .arg(&src)
            .arg(dir.join("root"))
            .status()
            .unwrap()
            .success();
        assert!(ok, "copying the fixture failed");
        let core = Core::new(SysRoot::at(dir.join("root")), dir.join("state"));
        (dir, core)
    }

    fn epp(dir: &Path) -> String {
        fs::read_to_string(
            dir.join("root/sys/devices/system/cpu/cpufreq/policy0/energy_performance_preference"),
        )
        .unwrap()
        .trim()
        .to_string()
    }

    #[test]
    fn choose_hold_release() {
        let (dir, mut c) = sandbox("chr");
        assert_eq!(c.active(), Profile::Balanced);

        assert!(c.choose(Profile::PowerSaver).unwrap().is_empty());
        assert_eq!(epp(&dir), "power");

        // Any hold outranks the user's choice.
        let perf = c
            .hold(Profile::Performance, "compiling", "ide", ":1.5")
            .unwrap();
        assert_eq!(c.active(), Profile::Performance);
        assert_eq!(epp(&dir), "performance");

        // A power-saver hold outranks a performance hold.
        let saver = c
            .hold(Profile::PowerSaver, "battery low", "shell", ":1.9")
            .unwrap();
        assert_eq!(c.active(), Profile::PowerSaver);

        assert!(
            c.release(saver, ":1.5").is_err(),
            "only the owner may release"
        );
        c.release(saver, ":1.9").unwrap();
        assert_eq!(c.active(), Profile::Performance);

        // The app disconnects: its hold ends and the user's choice returns.
        assert_eq!(c.release_owner(":1.5"), vec![perf]);
        assert_eq!(c.active(), Profile::PowerSaver);
        assert_eq!(epp(&dir), "power");

        assert!(c.hold(Profile::Balanced, "", "x", ":1.1").is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn choosing_releases_holds_and_is_saved() {
        let (dir, mut c) = sandbox("save");
        let h = c
            .hold(Profile::Performance, "game", "steam", ":1.2")
            .unwrap();
        assert_eq!(c.choose(Profile::PowerSaver).unwrap(), vec![h]);
        assert!(c.holds().is_empty());

        let again = Core::new(SysRoot::at(dir.join("root")), dir.join("state"));
        assert_eq!(again.active(), Profile::PowerSaver);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn safe_mode_refuses_and_keeps_state() {
        let (dir, mut c) = sandbox("safe");
        fs::create_dir_all(dir.join("root/etc/dynamis-power")).unwrap();
        fs::write(dir.join("root/etc/dynamis-power/disable"), "").unwrap();
        assert!(c
            .choose(Profile::Performance)
            .unwrap_err()
            .contains("safe mode"));
        assert_eq!(c.active(), Profile::Balanced);
        assert_eq!(epp(&dir), "balance_performance");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reapplies_after_an_outside_restore() {
        let (dir, mut c) = sandbox("restore");
        c.choose(Profile::Performance).unwrap();
        open_engine(c.root(), &dir.join("state"))
            .unwrap()
            .restore()
            .unwrap();
        assert_eq!(epp(&dir), "balance_performance");
        c.choose(Profile::Performance).unwrap();
        assert_eq!(epp(&dir), "performance");
        fs::remove_dir_all(&dir).ok();
    }
}
