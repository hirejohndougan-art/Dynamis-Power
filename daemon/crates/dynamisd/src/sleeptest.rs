//! The guided sleep test, run only when the person asks.
//!
//! Laptops can sleep in different ways (`s2idle`, "modern standby", and
//! `deep`, suspend to RAM). Which uses less battery, and which wakes cleanly,
//! differs per laptop. For each type the laptop offers, the test:
//!
//! 1. sets the sleep type (through the engine, journaled),
//! 2. notes the battery's energy and which devices are present,
//! 3. sets the wake-up alarm and asks the system to sleep (logind),
//! 4. after waking and a minute to settle, compares: battery used per hour,
//!    devices that did not come back, and kernel messages about failed
//!    resumes or device trouble.
//!
//! At the end the original sleep type is put back and the type that woke
//! cleanly and used least is recommended; it is applied only if the person
//! says yes. The test needs the laptop on battery, stops if it is plugged
//! in, waits while a call is running, and refuses in safe mode or dry run.
//! If the laptop does not wake properly (the service starts again after an
//! unclean shutdown in the middle of a sleep), that type is recorded as
//! failed and never recommended.

use std::collections::BTreeSet;
use std::path::Path;

use dynamis_core::auto::Context;
use dynamis_core::devices;
use dynamis_core::engine::Request;
use dynamis_core::fs::parse_bracketed;
use dynamis_core::health::{self, Monitor};
use dynamis_core::sensors::power;
use dynamis_core::settings::{self, Allowed, Probe};
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{open_engine, write_atomic};

const FILE: &str = "sleep-test.json";
const VERSION: u32 = 1;
const MEM_SLEEP: &str = "system.mem_sleep";
/// The battery must hold at least this much to start a sleep.
const MIN_BATTERY_PCT: f64 = 30.0;
/// Woke before this share of the planned time: woken early (lid, key).
const EARLY: f64 = 0.5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeResult {
    /// `s2idle` or `deep`.
    pub kind: String,
    /// Minutes asleep.
    pub minutes: f64,
    /// Battery used per hour asleep, in watt-hours and in % of a full battery.
    pub wh_per_h: Option<f64>,
    pub pct_per_h: Option<f64>,
    /// Woke cleanly: every device back and no resume trouble.
    pub clean: bool,
    /// What went wrong, in plain words (devices missing, kernel messages).
    #[serde(default)]
    pub problems: Vec<String>,
    /// The measurement does not count (plugged in, woken early).
    #[serde(default)]
    pub invalid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    Idle,
    /// About to sleep with the next type.
    Ready,
    Sleeping {
        kind: String,
        at: f64,
        energy_wh: Option<f64>,
        devices: Vec<String>,
    },
    /// Woke; waiting a minute for devices to come back.
    Settling {
        kind: String,
        slept_s: f64,
        used_wh: Option<f64>,
        on_ac: bool,
        until: f64,
        devices: Vec<String>,
    },
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Saved {
    v: u32,
    phase: Phase,
    minutes: f64,
    original: Option<String>,
    queue: Vec<String>,
    results: Vec<TypeResult>,
    recommended: Option<String>,
    note: Option<String>,
    started: f64,
}

pub struct SleepTest {
    phase: Phase,
    minutes: f64,
    /// The sleep type before the test, to put back.
    original: Option<String>,
    queue: Vec<String>,
    results: Vec<TypeResult>,
    recommended: Option<String>,
    /// Why it stopped early, or what is holding it, for the app.
    note: Option<String>,
    started: f64,
    /// Seconds per "minute" (60; shorter in tests).
    minute_s: f64,
    /// Kernel messages from the moment of sleeping.
    kmsg: Option<Monitor>,
    /// Ask the system to sleep now (taken by the reading thread).
    suspend_now: bool,
}

/// Sleep types this laptop offers, and the active one.
pub fn offered(root: &SysRoot) -> (Vec<String>, Option<String>) {
    let active = settings::by_id(MEM_SLEEP).and_then(|s| s.read(root).ok());
    match settings::by_id(MEM_SLEEP).map(|s| s.probe(root)) {
        Some(Probe::Supported {
            allowed: Allowed::Choices { values },
        }) => (values, active),
        _ => {
            // Only one type: it can still be measured, not compared.
            let (a, all) = root
                .read_string("/sys/power/mem_sleep")
                .map(|t| parse_bracketed(&t))
                .unwrap_or((None, Vec::new()));
            let a = a.or(active).or_else(|| all.first().cloned());
            (a.clone().into_iter().collect(), a)
        }
    }
}

/// What must come back after waking: USB device models, PCI functions with
/// their driver, sound cards and input devices.
fn snapshot(root: &SysRoot) -> Vec<String> {
    let mut s: BTreeSet<String> = BTreeSet::new();
    for d in devices::scan(root) {
        match d.kind {
            devices::Kind::Usb => {
                if let Some(m) = d.model {
                    s.insert(format!("usb {m} {}", d.name.unwrap_or_default()));
                }
            }
            devices::Kind::Pci => {
                if let Ok(drv) = root.link_name(&format!("{}/driver", d.path)) {
                    s.insert(format!("pci {} {drv}", d.address));
                }
            }
        }
    }
    for c in root.list_dir("/proc/asound") {
        if c.starts_with("card") {
            s.insert(format!("sound {c}"));
        }
    }
    let inputs = root
        .list_dir("/sys/class/input")
        .into_iter()
        .filter(|i| i.starts_with("input"))
        .count();
    s.insert(format!("input devices: {inputs}"));
    s.into_iter().collect()
}

fn energy(root: &SysRoot) -> Option<f64> {
    let ps = power::read(root);
    let e: Vec<f64> = ps
        .batteries
        .iter()
        .filter_map(|b| b.energy_now_wh)
        .collect();
    (!e.is_empty()).then(|| e.iter().sum())
}

fn full_wh(root: &SysRoot) -> Option<f64> {
    let ps = power::read(root);
    let e: Vec<f64> = ps
        .batteries
        .iter()
        .filter_map(|b| b.energy_full_wh)
        .collect();
    (!e.is_empty()).then(|| e.iter().sum())
}

/// Kernel messages that mean a resume went wrong.
fn resume_trouble(level: u32, message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("failed to resume")
        || m.contains("dpm_run_callback")
        || (level <= 3 && (m.contains("resume") || m.contains("pm: ")))
        || health::is_trouble(message)
}

impl SleepTest {
    pub fn load(dir: &Path, minute_s: f64) -> SleepTest {
        let saved: Option<Saved> = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let mut t = SleepTest {
            phase: Phase::Idle,
            minutes: 30.0,
            original: None,
            queue: Vec::new(),
            results: Vec::new(),
            recommended: None,
            note: None,
            started: 0.0,
            minute_s,
            kmsg: None,
            suspend_now: false,
        };
        if let Some(s) = saved {
            t.phase = s.phase;
            t.minutes = s.minutes;
            t.original = s.original;
            t.queue = s.queue;
            t.results = s.results;
            t.recommended = s.recommended;
            t.note = s.note;
            t.started = s.started;
        }
        t
    }

    fn save(&self, dir: &Path) {
        let s = Saved {
            v: VERSION,
            phase: self.phase.clone(),
            minutes: self.minutes,
            original: self.original.clone(),
            queue: self.queue.clone(),
            results: self.results.clone(),
            recommended: self.recommended.clone(),
            note: self.note.clone(),
            started: self.started,
        };
        match serde_json::to_string_pretty(&s) {
            Ok(t) => {
                if let Err(e) = write_atomic(&dir.join(FILE), &t) {
                    eprintln!("dynamisd: sleep test: {e}");
                }
            }
            Err(e) => eprintln!("dynamisd: sleep test: {e}"),
        }
    }

    pub fn set_minute_s(&mut self, s: f64) {
        self.minute_s = s;
    }

    pub fn running(&self) -> bool {
        !matches!(self.phase, Phase::Idle | Phase::Done)
    }

    /// The service started again. A test that was asleep did not end in a
    /// normal wake: if the boot before was unclean, that type failed.
    pub fn after_start(&mut self, root: &SysRoot, dir: &Path, unclean: bool) {
        let kind = match &self.phase {
            Phase::Sleeping { kind, .. } | Phase::Settling { kind, .. } => kind.clone(),
            Phase::Ready => {
                self.finish(root, dir, Some("the service restarted during the test"));
                return;
            }
            _ => return,
        };
        let sleeping = matches!(self.phase, Phase::Sleeping { .. });
        if unclean && sleeping {
            self.results.push(TypeResult {
                kind,
                minutes: 0.0,
                wh_per_h: None,
                pct_per_h: None,
                clean: false,
                problems: vec!["the laptop did not wake properly and had to be restarted".into()],
                invalid: None,
            });
            self.finish(root, dir, Some("the laptop did not wake properly"));
        } else {
            self.finish(root, dir, Some("the service restarted during the test"));
        }
    }

    /// Start, if the laptop is ready. `minutes` per sleep type.
    pub fn start(
        &mut self,
        root: &SysRoot,
        dir: &Path,
        minutes: f64,
        ctx: &Context,
        now: f64,
    ) -> Result<(), String> {
        if self.running() {
            return Err("a sleep test is already running".into());
        }
        if !(5.0..=60.0).contains(&minutes) && self.minute_s >= 60.0 {
            return Err("each sleep must be between 5 and 60 minutes".into());
        }
        let engine = open_engine(root, dir)?;
        if let Some(why) = engine.safe_mode() {
            return Err(format!("safe mode is on ({why})"));
        }
        if engine.dry_run() {
            return Err("dry run is on, so nothing can be tested".into());
        }
        drop(engine);
        if !root.exists("/sys/class/rtc/rtc0/wakealarm") {
            return Err("this laptop has no wake-up alarm, so it could not wake by itself".into());
        }
        if ctx.on_ac {
            return Err("unplug the charger first: the test measures the battery".into());
        }
        if ctx.battery_pct.is_none_or(|b| b < MIN_BATTERY_PCT) {
            return Err(format!("the battery needs at least {MIN_BATTERY_PCT:.0}%"));
        }
        let (types, active) = offered(root);
        if types.is_empty() {
            return Err("this laptop offers no sleep type to test".into());
        }
        self.minutes = minutes;
        self.original = active;
        self.queue = types;
        self.results.clear();
        self.recommended = None;
        self.note = None;
        self.started = now;
        self.phase = Phase::Ready;
        self.save(dir);
        Ok(())
    }

    /// Stop now and put the original sleep type back.
    pub fn stop(&mut self, root: &SysRoot, dir: &Path) {
        if self.running() {
            self.finish(root, dir, Some("you stopped it"));
        }
    }

    /// Whether the reading thread should ask the system to sleep now.
    pub fn take_suspend(&mut self) -> bool {
        std::mem::take(&mut self.suspend_now)
    }

    /// Asking the system to sleep failed.
    pub fn suspend_failed(&mut self, root: &SysRoot, dir: &Path, why: &str) {
        self.finish(
            root,
            dir,
            Some(&format!("the laptop could not be put to sleep: {why}")),
        );
    }

    /// One reading (unix time).
    pub fn tick(&mut self, root: &SysRoot, dir: &Path, now: f64, ctx: &Context) {
        match self.phase.clone() {
            Phase::Ready => {
                if ctx.on_ac {
                    self.finish(root, dir, Some("the charger was plugged in"));
                } else if ctx.mic_in_use {
                    self.note = Some("waiting: a call is running".into());
                } else {
                    self.note = None;
                    self.begin_sleep(root, dir, now);
                }
            }
            Phase::Settling {
                kind,
                slept_s,
                used_wh,
                on_ac,
                until,
                devices: before,
            } if now >= until => {
                let after = snapshot(root);
                let mut problems: Vec<String> = before
                    .iter()
                    .filter(|d| !after.contains(d))
                    .map(|d| format!("did not come back: {d}"))
                    .collect();
                if let Some(m) = self.kmsg.as_mut() {
                    problems.extend(
                        m.kernel_messages()
                            .into_iter()
                            .filter(|(l, msg)| resume_trouble(*l, msg))
                            .map(|(_, msg)| {
                                format!("kernel: {}", msg.chars().take(160).collect::<String>())
                            })
                            .take(5),
                    );
                }
                self.kmsg = None;
                let hours = slept_s / 3600.0;
                let planned = self.minutes * self.minute_s;
                let invalid = if on_ac {
                    Some("the charger was plugged in".to_string())
                } else if slept_s < planned * EARLY {
                    Some("woken early".to_string())
                } else {
                    None
                };
                let wh_per_h = used_wh
                    .filter(|_| hours > 0.0)
                    .map(|w| (w / hours * 100.0).round() / 100.0);
                let pct_per_h = wh_per_h
                    .zip(full_wh(root))
                    .map(|(w, f)| (w / f * 1000.0).round() / 10.0);
                self.results.push(TypeResult {
                    kind,
                    minutes: (slept_s / self.minute_s * 10.0).round() / 10.0,
                    wh_per_h,
                    pct_per_h,
                    clean: problems.is_empty(),
                    problems,
                    invalid,
                });
                if self.queue.is_empty() {
                    self.finish(root, dir, None);
                } else if on_ac {
                    self.finish(root, dir, Some("the charger was plugged in"));
                } else {
                    self.phase = Phase::Ready;
                    self.save(dir);
                }
            }
            _ => {}
        }
    }

    fn begin_sleep(&mut self, root: &SysRoot, dir: &Path, now: f64) {
        let Some(kind) = self.queue.first().cloned() else {
            self.finish(root, dir, None);
            return;
        };
        let result = open_engine(root, dir).and_then(|mut e| {
            let current = settings::by_id(MEM_SLEEP).and_then(|s| s.read(root).ok());
            if current.as_deref() != Some(kind.as_str()) {
                e.apply(&Request {
                    id: MEM_SLEEP,
                    value: &kind,
                    allow_risky: true,
                    trial_secs: None,
                    reason: "sleep test",
                })
                .map_err(|e| e.to_string())?;
            }
            let wake = (now + self.minutes * self.minute_s).ceil() as u64;
            e.set_wake_alarm(
                Some(wake),
                &format!(
                    "sleep test: wake after {} minutes with {kind}",
                    self.minutes
                ),
            )
            .map_err(|e| e.to_string())
        });
        if let Err(e) = result {
            self.finish(root, dir, Some(&e));
            return;
        }
        self.queue.remove(0);
        self.kmsg = Some(Monitor::start(root, health::Limits::default()));
        self.phase = Phase::Sleeping {
            kind,
            at: now,
            energy_wh: energy(root),
            devices: snapshot(root),
        };
        self.save(dir);
        self.suspend_now = true;
    }

    /// The laptop woke.
    pub fn on_resume(&mut self, root: &SysRoot, dir: &Path, now: f64) {
        let Phase::Sleeping {
            kind,
            at,
            energy_wh,
            devices,
        } = self.phase.clone()
        else {
            return;
        };
        if let Ok(mut e) = open_engine(root, dir) {
            let _ = e.set_wake_alarm(None, "sleep test: awake");
        }
        let after = energy(root);
        self.phase = Phase::Settling {
            kind,
            slept_s: (now - at).max(0.0),
            used_wh: energy_wh.zip(after).map(|(b, a)| (b - a).max(0.0)),
            on_ac: power::read(root).on_ac(),
            until: now + self.minute_s,
            devices,
        };
        self.save(dir);
    }

    fn finish(&mut self, root: &SysRoot, dir: &Path, note: Option<&str>) {
        self.kmsg = None;
        self.suspend_now = false;
        if let Ok(mut e) = open_engine(root, dir) {
            let _ = e.set_wake_alarm(None, "sleep test ended");
            if let Some(o) = &self.original {
                let current = settings::by_id(MEM_SLEEP).and_then(|s| s.read(root).ok());
                if current.as_deref() != Some(o.as_str()) {
                    if let Err(err) = e.apply(&Request {
                        id: MEM_SLEEP,
                        value: o,
                        allow_risky: true,
                        trial_secs: None,
                        reason: "sleep test ended: the original sleep type is back",
                    }) {
                        eprintln!("dynamisd: sleep test: {err}");
                    }
                }
            }
        }
        self.queue.clear();
        self.note = note.map(String::from);
        self.recommended = recommend(&self.results, self.original.as_deref());
        self.phase = Phase::Done;
        self.save(dir);
    }

    pub fn json(&self, root: &SysRoot) -> Value {
        let (types, active) = offered(root);
        json!({
            "phase": match &self.phase {
                Phase::Idle => "idle",
                Phase::Ready => "ready",
                Phase::Sleeping { .. } => "sleeping",
                Phase::Settling { .. } => "settling",
                Phase::Done => "done",
            },
            "current": match &self.phase {
                Phase::Sleeping { kind, .. } | Phase::Settling { kind, .. } => Some(kind.clone()),
                _ => None,
            },
            "left": self.queue,
            "minutes": self.minutes,
            "results": self.results,
            "recommended": self.recommended,
            "original": self.original,
            "note": self.note,
            "offered": types,
            "active": active,
            "alarm": root.exists("/sys/class/rtc/rtc0/wakealarm"),
            // Whether "sleep, then hibernate" could be set up here, and what
            // is missing (advice only; decision 0025).
            "hibernate": dynamis_core::hibernate::check(root),
        })
    }
}

/// The type to recommend: among those that woke cleanly with a valid
/// measurement, the one that used least. Within 10% of the original type,
/// the original (no change for a small difference). None if only one type
/// was measured or none woke cleanly.
pub fn recommend(results: &[TypeResult], original: Option<&str>) -> Option<String> {
    let good: Vec<&TypeResult> = results
        .iter()
        .filter(|r| r.clean && r.invalid.is_none() && r.wh_per_h.is_some())
        .collect();
    let failed: BTreeSet<&str> = results
        .iter()
        .filter(|r| !r.clean)
        .map(|r| r.kind.as_str())
        .collect();
    let good: Vec<&TypeResult> = good
        .into_iter()
        .filter(|r| !failed.contains(r.kind.as_str()))
        .collect();
    if results
        .iter()
        .map(|r| &r.kind)
        .collect::<BTreeSet<_>>()
        .len()
        < 2
    {
        return None;
    }
    let best = good.iter().min_by(|a, b| {
        a.wh_per_h
            .unwrap_or(f64::MAX)
            .total_cmp(&b.wh_per_h.unwrap_or(f64::MAX))
    })?;
    if let Some(o) = original {
        if let Some(orig) = good.iter().find(|r| r.kind == o) {
            if orig.wh_per_h.unwrap_or(f64::MAX) <= best.wh_per_h.unwrap_or(0.0) * 1.1 {
                return Some(o.to_string());
            }
        }
    }
    Some(best.kind.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(kind: &str, wh: f64, clean: bool) -> TypeResult {
        TypeResult {
            kind: kind.into(),
            minutes: 30.0,
            wh_per_h: Some(wh),
            pct_per_h: None,
            clean,
            problems: Vec::new(),
            invalid: None,
        }
    }

    #[test]
    fn recommends_the_clean_one_that_used_least() {
        assert_eq!(
            recommend(
                &[r("s2idle", 1.2, true), r("deep", 0.3, true)],
                Some("s2idle")
            )
            .as_deref(),
            Some("deep")
        );
        // Within 10%: keep what was there.
        assert_eq!(
            recommend(
                &[r("s2idle", 0.32, true), r("deep", 0.3, true)],
                Some("s2idle")
            )
            .as_deref(),
            Some("s2idle")
        );
        // Deep did not wake cleanly: never recommended.
        assert_eq!(
            recommend(
                &[r("s2idle", 1.2, true), r("deep", 0.3, false)],
                Some("s2idle")
            )
            .as_deref(),
            Some("s2idle")
        );
        // One type only: nothing to compare.
        assert_eq!(recommend(&[r("s2idle", 1.2, true)], Some("s2idle")), None);
        // Woken early does not count.
        let mut early = r("deep", 0.1, true);
        early.invalid = Some("woken early".into());
        assert_eq!(
            recommend(&[r("s2idle", 1.2, true), early], Some("s2idle")).as_deref(),
            Some("s2idle")
        );
    }

    #[test]
    fn a_type_that_never_woke_is_recorded_as_failed() {
        let dir = std::env::temp_dir().join(format!("dynamis-sleeptest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/sysfs/synthetic-laptop");
        assert!(std::process::Command::new("cp")
            .args(["-a"])
            .arg(&src)
            .arg(dir.join("root"))
            .status()
            .unwrap()
            .success());
        std::fs::create_dir_all(dir.join("state")).unwrap();
        let (root, state) = (SysRoot::at(dir.join("root")), dir.join("state"));
        let mut t = SleepTest::load(&state, 60.0);
        t.original = Some("deep".into());
        t.results.push(r("s2idle", 1.0, true));
        t.phase = Phase::Sleeping {
            kind: "deep".into(),
            at: 0.0,
            energy_wh: None,
            devices: Vec::new(),
        };
        t.save(&state);
        // The service starts again after an unclean shutdown.
        let mut t = SleepTest::load(&state, 60.0);
        t.after_start(&root, &state, true);
        assert!(!t.running());
        let deep = t.results.iter().find(|r| r.kind == "deep").unwrap();
        assert!(!deep.clean);
        assert_eq!(t.recommended.as_deref(), Some("s2idle"));
        assert_eq!(t.note.as_deref(), Some("the laptop did not wake properly"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resume_trouble_is_not_every_error() {
        assert!(resume_trouble(
            6,
            "PM: Device 0000:00:14.0 failed to resume async: error -110"
        ));
        assert!(resume_trouble(3, "PM: resume devices took too long"));
        assert!(resume_trouble(
            4,
            "i915 0000:00:02.0: [drm] GPU HANG: ecode"
        ));
        assert!(!resume_trouble(
            3,
            "ACPI Error: AE_NOT_FOUND, While resolving a named reference"
        ));
    }
}
