//! "Check deep idle" (Learning screen): a one-minute, read-only measurement
//! started by the person. The service reads the counters at the start and
//! after a minute (on its usual readings, so nothing extra wakes), and keeps
//! the last report in `deep-idle.json`.

use std::path::Path;

use dynamis_core::deepidle::{self, Report, Snapshot, CHECK_S};
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::write_atomic;

const FILE: &str = "deep-idle.json";

#[derive(Debug, Serialize, Deserialize)]
struct Saved {
    v: u32,
    /// Unix time the check ended.
    at: u64,
    report: Value,
}

#[derive(Default)]
pub struct DeepIdle {
    start: Option<Snapshot>,
    touched: bool,
    last: Option<(u64, Value)>,
}

impl DeepIdle {
    pub fn load(dir: &Path) -> Self {
        let last = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|s| serde_json::from_str::<Saved>(&s).ok())
            .filter(|s| s.v == deepidle::REPORT_VERSION)
            .map(|s| (s.at, s.report));
        DeepIdle {
            last,
            ..Default::default()
        }
    }

    pub fn running(&self) -> bool {
        self.start.is_some()
    }

    pub fn start(&mut self, root: &SysRoot, now: f64) {
        self.start = Some(deepidle::snapshot(root, now));
        self.touched = false;
    }

    /// One reading. `user_idle` is the session helper's idle report: any use
    /// during the check is said in the report.
    pub fn tick(&mut self, root: &SysRoot, dir: &Path, now: f64, user_idle: bool) {
        let Some(a) = &self.start else {
            return;
        };
        if !user_idle {
            self.touched = true;
        }
        // A clock jump back or a sleep in between: start again.
        if now < a.time_s || now - a.time_s > CHECK_S * 5.0 {
            self.start = Some(deepidle::snapshot(root, now));
            return;
        }
        if now - a.time_s < CHECK_S {
            return;
        }
        let b = deepidle::snapshot(root, now);
        let r: Report = deepidle::report(root, a, &b, self.touched);
        let value = serde_json::to_value(&r).unwrap_or(Value::Null);
        let at = now as u64;
        let saved = Saved {
            v: deepidle::REPORT_VERSION,
            at,
            report: value.clone(),
        };
        if let Ok(s) = serde_json::to_string(&saved) {
            if let Err(e) = write_atomic(&dir.join(FILE), &s) {
                eprintln!("dynamisd: {FILE}: {e}");
            }
        }
        self.last = Some((at, value));
        self.start = None;
    }

    /// Forget a check that cannot finish (sleep).
    pub fn cancel(&mut self) {
        self.start = None;
    }

    pub fn json(&self, now: f64) -> Value {
        json!({
            "running": self.running(),
            "seconds_left": self.start.as_ref().map(|a| (CHECK_S - (now - a.time_s)).clamp(0.0, CHECK_S).round()),
            "at": self.last.as_ref().map(|(at, _)| at),
            "report": self.last.as_ref().map(|(_, r)| r),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_a_minute_and_keeps_the_report() {
        let base = std::env::temp_dir().join(format!("dynamisd-deepidle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (rootdir, state) = (base.join("root"), base.join("state"));
        std::fs::create_dir_all(&state).unwrap();
        deepidle::fixture::core(&rootdir, 0, &["POLL", "C8"], 0);
        let root = SysRoot::at(&rootdir);
        let mut d = DeepIdle::load(&state);
        d.start(&root, 1000.0);
        assert!(d.running());
        d.tick(&root, &state, 1030.0, false);
        assert!(d.running());
        assert_eq!(d.json(1030.0)["seconds_left"], 30.0);
        deepidle::fixture::core(&rootdir, 0, &["POLL", "C8"], 45_000_000);
        d.tick(&root, &state, 1060.0, true);
        assert!(!d.running());
        let j = d.json(1060.0);
        assert_eq!(j["report"]["core_deepest_pct"], 75.0);
        assert_eq!(j["report"]["touched"], true, "used during the check");
        // Kept across restarts.
        let again = DeepIdle::load(&state);
        assert_eq!(again.json(2000.0)["at"], 1060);
        let _ = std::fs::remove_dir_all(&base);
    }
}
