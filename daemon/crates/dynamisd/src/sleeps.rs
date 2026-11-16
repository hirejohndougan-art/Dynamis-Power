//! What woke the laptop, and how much battery each sleep cost (overnight
//! drain), kept for the app. A reading of the wake-up counters and the
//! battery when the laptop goes to sleep, another when it wakes.

use std::path::Path;

use dynamis_core::sensors::power;
use dynamis_core::wakeups::{self, Counts, Sleep};
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::write_atomic;

const FILE: &str = "sleeps.json";
const VERSION: u32 = 1;
const KEEP: usize = 30;

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    v: u32,
    sleeps: Vec<Sleep>,
}

#[derive(Default)]
pub struct SleepLog {
    before: Option<(f64, Counts, Option<f64>, bool)>,
    pub sleeps: Vec<Sleep>,
}

fn battery(root: &SysRoot) -> (Option<f64>, bool) {
    let ps = power::read(root);
    (
        ps.batteries
            .first()
            .and_then(|b| b.capacity_pct)
            .map(f64::from),
        ps.on_ac(),
    )
}

impl SleepLog {
    pub fn load(dir: &Path) -> SleepLog {
        let sleeps = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
            .map(|s| s.sleeps)
            .unwrap_or_default();
        SleepLog {
            before: None,
            sleeps,
        }
    }

    pub fn going(&mut self, root: &SysRoot, now: f64) {
        let (pct, ac) = battery(root);
        self.before = Some((now, wakeups::read(root), pct, ac));
    }

    pub fn woke(&mut self, root: &SysRoot, dir: &Path, now: f64) {
        let Some((from, counts, pct, ac)) = self.before.take() else {
            return;
        };
        let (after_pct, after_ac) = battery(root);
        self.sleeps.push(Sleep {
            from,
            to: now,
            woke_by: wakeups::woke_by(&counts, &wakeups::read(root)),
            battery_before: pct,
            battery_after: after_pct,
            on_ac: ac || after_ac,
        });
        if self.sleeps.len() > KEEP {
            let extra = self.sleeps.len() - KEEP;
            self.sleeps.drain(..extra);
        }
        if let Ok(t) = serde_json::to_string(&Saved {
            v: VERSION,
            sleeps: self.sleeps.clone(),
        }) {
            let _ = write_atomic(&dir.join(FILE), &t);
        }
    }

    pub fn json(&self) -> Value {
        let list: Vec<Value> = self
            .sleeps
            .iter()
            .rev()
            .map(|s| {
                json!({
                    "from": s.from,
                    "to": s.to,
                    "woke_by": s.woke_by.iter().map(|n| json!({"name": n, "kind": wakeups::kind(n)})).collect::<Vec<_>>(),
                    "pct_per_hour": s.pct_per_hour(),
                    "battery_before": s.battery_before,
                    "battery_after": s.battery_after,
                    "on_ac": s.on_ac,
                })
            })
            .collect();
        json!({ "sleeps": list })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sleep_is_remembered_with_what_woke_it() {
        let dir = std::env::temp_dir().join(format!("dynamis-sleeps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let w = dir.join("root/sys/class/wakeup/wakeup3");
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(w.join("name"), "PNP0C0D:00\n").unwrap();
        std::fs::write(w.join("wakeup_count"), "2\n").unwrap();
        std::fs::write(w.join("event_count"), "2\n").unwrap();
        let state = dir.join("state");
        std::fs::create_dir_all(&state).unwrap();
        let root = SysRoot::at(dir.join("root"));
        let mut log = SleepLog::load(&state);
        log.going(&root, 1000.0);
        std::fs::write(w.join("wakeup_count"), "3\n").unwrap();
        log.woke(&root, &state, 1000.0 + 7.0 * 3600.0);
        let j = log.json();
        assert_eq!(j["sleeps"][0]["woke_by"][0]["kind"], "lid");
        assert_eq!(SleepLog::load(&state).sleeps.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
