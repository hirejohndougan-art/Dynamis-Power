//! "It feels slow" (everyday problem §3): for the rest of the day, undo the
//! things Dynamis Power does that cost speed. Longer battery becomes
//! Balanced, the learned turbo and speed limit go back to the mode's own
//! values, background apps are not slowed, and learning does not test the
//! settings that cost lag. Choosing a mode by hand ends it, as does
//! midnight. Saved, so a restart keeps it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::write_atomic;

const FILE: &str = "relief.json";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Saved {
    v: u32,
    until: f64,
}

#[derive(Default)]
pub struct Relief {
    pub until: Option<f64>,
}

impl Relief {
    pub fn load(dir: &Path, now: f64) -> Relief {
        let until = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
            .map(|s| s.until)
            .filter(|u| *u > now);
        Relief { until }
    }

    pub fn active(&self, now: f64) -> bool {
        self.until.is_some_and(|u| u > now)
    }

    pub fn start(&mut self, dir: &Path, until: f64) -> Result<(), String> {
        self.until = Some(until);
        write_atomic(
            &dir.join(FILE),
            &serde_json::to_string(&Saved { v: VERSION, until }).map_err(|e| e.to_string())?,
        )
    }

    pub fn end(&mut self, dir: &Path) {
        self.until = None;
        let _ = std::fs::remove_file(dir.join(FILE));
    }
}
