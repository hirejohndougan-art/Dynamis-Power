//! Auto mode: choosing a profile from what the laptop is doing.
//!
//! This is layers 1 and 2 of the auto-mode design (docs/DESIGN.md): context
//! rules and rule-based workload detection. It is a pure function of the
//! readings it is given and the time, with no I/O, so every decision can be
//! replayed in scripted scenarios (`tests/scenarios/*.json`, the phase 3 gate).
//!
//! Each call to [`Auto::step`] answers: which workload is this, which profile
//! fits it, why (in plain language), and whether to switch now. Switches are
//! smoothed so the profile does not flap:
//!
//! - **Hysteresis.** A state is entered at one threshold and left at another
//!   (heavy work starts at 70% CPU for 30 s and ends below 40%).
//! - **Dwell time.** After a switch, wait at least `min_dwell_s` before the
//!   next, unless the change is urgent.
//! - **Rate limit.** At most `max_switches_per_hour` non-urgent switches.
//!
//! Urgent changes skip both: the charger being plugged in or out, the battery
//! reaching the critical level, a call starting, and the user coming back to
//! an idle machine (which must feel instant).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::profiles::Profile;

/// What the laptop is doing right now. System readings come from
/// [`crate::context`]; session readings from the user's session agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Context {
    pub on_ac: bool,
    pub battery_pct: Option<f64>,
    /// Share of CPU time busy since the last reading, all cores.
    pub cpu_busy_pct: f64,
    /// Network traffic, megabytes per second, both directions.
    pub net_mb_s: f64,
    pub max_temp_c: Option<f64>,
    /// Whole-system power from the battery, watts (on battery only).
    pub power_w: Option<f64>,
    /// Share of time tasks stalled (CPU + memory + disk) since the last
    /// reading, percent. The lag measure calibration holds as a hard limit.
    pub stall_pct: f64,
    pub brightness_pct: Option<f64>,
    /// Energy left in the battery (all batteries), watt-hours.
    pub energy_wh: Option<f64>,
    /// A microphone is recording: almost always a call.
    pub mic_in_use: bool,
    // From the session agent:
    pub media_playing: bool,
    /// Something asked the desktop not to dim the screen (full-screen video,
    /// a presentation).
    pub screen_inhibited: bool,
    /// A game registered with GameMode.
    pub game_running: bool,
    pub screen_locked: bool,
    pub user_idle: bool,
    /// Apps with a process right now (names from [`crate::apps`]).
    pub running_apps: Vec<String>,
    /// The app in front, when the session agent knows it.
    pub focused_app: Option<String>,
}

/// "Use this mode while this app is open."
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppRule {
    pub app: String,
    pub profile: Profile,
}

/// The workload classes from docs/DESIGN.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Workload {
    Idle,
    LightInteractive,
    BurstyInteractive,
    Video,
    Call,
    SustainedCompute,
    Gaming,
    BackgroundTransfer,
}

impl Workload {
    pub fn describe(self) -> &'static str {
        match self {
            Workload::Idle => "nobody is using the laptop",
            Workload::LightInteractive => "light use",
            Workload::BurstyInteractive => "busy use",
            Workload::Video => "playing video or media",
            Workload::Call => "on a call",
            Workload::SustainedCompute => "heavy work",
            Workload::Gaming => "playing a game",
            Workload::BackgroundTransfer => "a download or upload is running",
        }
    }
}

/// Choices a person can change. Defaults are deliberately mild.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Prefs {
    /// Profile for ordinary use when plugged in.
    pub on_ac: Profile,
    /// Profile for ordinary use on battery.
    pub on_battery: Profile,
    /// Use performance for heavy work and games when plugged in.
    pub boost_on_ac: bool,
    /// Below this, prefer power-saver for ordinary use.
    pub low_battery_pct: f64,
    /// Below this, power-saver whatever is happening.
    pub critical_battery_pct: f64,
    /// Above this, never performance.
    pub hot_c: f64,
    pub busy_start_pct: f64,
    pub busy_end_pct: f64,
    pub busy_start_s: f64,
    /// At least this much traffic counts as a transfer.
    pub transfer_mb_s: f64,
    pub min_dwell_s: f64,
    pub max_switches_per_hour: usize,
    /// Per-app modes. The app in front wins; otherwise the first rule whose
    /// app is open.
    pub app_rules: Vec<AppRule>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            on_ac: Profile::Balanced,
            on_battery: Profile::Balanced,
            boost_on_ac: true,
            low_battery_pct: 30.0,
            critical_battery_pct: 15.0,
            hot_c: 85.0,
            busy_start_pct: 70.0,
            busy_end_pct: 40.0,
            busy_start_s: 30.0,
            transfer_mb_s: 1.0,
            min_dwell_s: 60.0,
            max_switches_per_hour: 12,
            app_rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decision {
    pub profile: Profile,
    pub workload: Workload,
    /// One plain-language line for people.
    pub reason: String,
    /// Skips dwell time and the rate limit.
    pub urgent: bool,
}

/// What one step decided.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    pub decision: Decision,
    /// `Some` when the profile should change now.
    pub switch_to: Option<Profile>,
    /// Why a wanted switch is being held back, if it is.
    pub waiting: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Auto {
    pub prefs: Prefs,
    busy_since: Option<f64>,
    busy: bool,
    low: bool,
    hot: bool,
    current: Option<Profile>,
    last_switch: Option<f64>,
    last_on_ac: Option<bool>,
    last_workload: Option<Workload>,
    last_running: Vec<String>,
    recent: VecDeque<f64>,
}

impl Auto {
    pub fn new(prefs: Prefs) -> Auto {
        Auto {
            prefs,
            ..Default::default()
        }
    }

    /// The profile auto mode last switched to (or was told is active).
    pub fn current(&self) -> Option<Profile> {
        self.current
    }

    /// Tell auto mode what is active now, without counting a switch (for
    /// example when it is turned on, or after a person picked a profile).
    pub fn set_current(&mut self, p: Profile, now: f64) {
        self.current = Some(p);
        self.last_switch = Some(now);
    }

    pub fn step(&mut self, now: f64, ctx: &Context, available: &[Profile]) -> Step {
        let p = &self.prefs;

        // Hysteresis for heavy work, low battery and heat.
        if ctx.cpu_busy_pct >= p.busy_start_pct {
            let since = *self.busy_since.get_or_insert(now);
            if now - since >= p.busy_start_s {
                self.busy = true;
            }
        } else {
            self.busy_since = None;
            if ctx.cpu_busy_pct < p.busy_end_pct {
                self.busy = false;
            }
        }
        let pct = ctx.battery_pct.unwrap_or(100.0);
        if ctx.on_ac || pct >= p.low_battery_pct + 5.0 {
            self.low = false;
        } else if pct <= p.low_battery_pct {
            self.low = true;
        }
        match ctx.max_temp_c {
            Some(t) if t >= p.hot_c => self.hot = true,
            Some(t) if t < p.hot_c - 10.0 => self.hot = false,
            None => self.hot = false,
            _ => {}
        }

        let mut d = self.decide(ctx, available);

        // Urgency.
        let power_changed = self.last_on_ac.is_some_and(|a| a != ctx.on_ac);
        let woke_up = self.last_workload == Some(Workload::Idle) && d.workload != Workload::Idle;
        let call_started =
            d.workload == Workload::Call && self.last_workload != Some(Workload::Call);
        // Opening an app with its own mode should feel instant, like the
        // user's own choice. Moving between apps already open does not skip
        // the smoothing, so clicking around cannot make the mode flap.
        let rule_started = self
            .app_rule(ctx)
            .is_some_and(|r| !self.last_running.contains(&r.app));
        self.last_running = ctx.running_apps.clone();
        d.urgent |= power_changed || woke_up || call_started || rule_started;
        self.last_on_ac = Some(ctx.on_ac);
        self.last_workload = Some(d.workload);

        // Should we switch now?
        let wanted = Some(d.profile) != self.current;
        let mut waiting = None;
        let mut switch_to = None;
        if wanted {
            while self.recent.front().is_some_and(|t| now - t > 3600.0) {
                self.recent.pop_front();
            }
            let since_last = self.last_switch.map(|t| now - t);
            if self.current.is_none() || d.urgent {
                switch_to = Some(d.profile);
            } else if since_last.is_some_and(|s| s < self.prefs.min_dwell_s) {
                waiting = Some(format!(
                    "waiting {:.0} s before switching again",
                    self.prefs.min_dwell_s - since_last.unwrap_or(0.0)
                ));
            } else if self.recent.len() >= self.prefs.max_switches_per_hour {
                waiting = Some(format!(
                    "limit of {} automatic switches an hour reached",
                    self.prefs.max_switches_per_hour
                ));
            } else {
                switch_to = Some(d.profile);
                self.recent.push_back(now);
            }
        }
        if let Some(p) = switch_to {
            self.current = Some(p);
            self.last_switch = Some(now);
        }
        Step {
            decision: d,
            switch_to,
            waiting,
        }
    }

    /// The rules, in order. The first that applies wins.
    fn decide(&self, ctx: &Context, available: &[Profile]) -> Decision {
        let p = &self.prefs;
        let pct = ctx.battery_pct;
        let on_batt = !ctx.on_ac;
        let workload = self.workload(ctx);

        let (profile, reason, urgent) =
            if on_batt && pct.is_some_and(|b| b <= p.critical_battery_pct) {
                (
                    Profile::PowerSaver,
                    format!(
                        "Battery very low ({:.0}%): saving every bit",
                        pct.unwrap_or(0.0)
                    ),
                    true,
                )
            } else {
                let ordinary = || {
                    if on_batt && self.low {
                        (
                            Profile::PowerSaver,
                            format!("Battery below {:.0}%: saving battery", p.low_battery_pct),
                        )
                    } else if ctx.on_ac {
                        (p.on_ac, "Plugged in".to_string())
                    } else {
                        (p.on_battery, "On battery".to_string())
                    }
                };
                // Heavy work and games: the most speed the situation allows.
                let demanding = |what: &str| {
                    if ctx.on_ac && p.boost_on_ac {
                        (
                            Profile::Performance,
                            format!("{what} while plugged in: full speed"),
                        )
                    } else if on_batt && self.low {
                        (
                            Profile::PowerSaver,
                            format!("{what}, but the battery is below {:.0}%", p.low_battery_pct),
                        )
                    } else {
                        (Profile::Balanced, format!("{what} on battery: balanced"))
                    }
                };
                let rule = match workload {
                    Workload::Idle | Workload::Call => None,
                    _ => self.app_rule(ctx),
                };
                let (profile, reason) = if let Some(r) = rule {
                    if r.profile == Profile::Performance && on_batt && self.low {
                        (
                            Profile::PowerSaver,
                            format!(
                                "{} is open, but the battery is below {:.0}%",
                                r.app, p.low_battery_pct
                            ),
                        )
                    } else {
                        (r.profile, format!("{} is open: its chosen mode", r.app))
                    }
                } else {
                    match workload {
                        Workload::Idle => (
                            Profile::PowerSaver,
                            "Nobody is using the laptop: saving power".to_string(),
                        ),
                        Workload::Call => (
                            Profile::Balanced,
                            "On a call: keeping sound and video steady".to_string(),
                        ),
                        Workload::Gaming => demanding("Playing a game"),
                        Workload::SustainedCompute => demanding("Heavy work"),
                        Workload::Video if on_batt && self.low => (
                            Profile::PowerSaver,
                            format!(
                                "Playing media with the battery below {:.0}%",
                                p.low_battery_pct
                            ),
                        ),
                        Workload::Video => (
                            Profile::Balanced,
                            "Playing media: keeping it smooth".to_string(),
                        ),
                        Workload::BackgroundTransfer => {
                            let (prof, why) = ordinary();
                            (
                                if prof == Profile::PowerSaver && !self.low {
                                    Profile::Balanced
                                } else {
                                    prof
                                },
                                format!("{why}; a transfer is running"),
                            )
                        }
                        Workload::LightInteractive | Workload::BurstyInteractive => ordinary(),
                    }
                };
                (profile, reason, false)
            };

        // Heat caps everything at balanced.
        let (profile, reason) = if self.hot && profile == Profile::Performance {
            (
                Profile::Balanced,
                format!("{reason} — but running hot, so balanced"),
            )
        } else {
            (profile, reason)
        };
        // Use the nearest profile this machine offers.
        let profile = if available.contains(&profile) {
            profile
        } else {
            Profile::Balanced
        };
        Decision {
            profile,
            workload,
            reason,
            urgent,
        }
    }

    /// The per-app rule that applies now, if any: the app in front first,
    /// then the first rule (in the user's order) whose app is open.
    fn app_rule(&self, ctx: &Context) -> Option<&AppRule> {
        let rules = &self.prefs.app_rules;
        if let Some(f) = &ctx.focused_app {
            if let Some(r) = rules.iter().find(|r| &r.app == f) {
                return Some(r);
            }
        }
        rules.iter().find(|r| ctx.running_apps.contains(&r.app))
    }

    fn workload(&self, ctx: &Context) -> Workload {
        if ctx.mic_in_use {
            Workload::Call
        } else if ctx.game_running {
            Workload::Gaming
        } else if ctx.screen_locked
            || (ctx.user_idle && !ctx.media_playing && !ctx.screen_inhibited)
        {
            if ctx.net_mb_s >= self.prefs.transfer_mb_s {
                Workload::BackgroundTransfer
            } else {
                Workload::Idle
            }
        } else if self.busy {
            Workload::SustainedCompute
        } else if ctx.media_playing || ctx.screen_inhibited {
            Workload::Video
        } else if ctx.net_mb_s >= self.prefs.transfer_mb_s
            && ctx.cpu_busy_pct < self.prefs.busy_end_pct
        {
            Workload::BackgroundTransfer
        } else if ctx.cpu_busy_pct >= self.prefs.busy_end_pct {
            Workload::BurstyInteractive
        } else {
            Workload::LightInteractive
        }
    }
}
