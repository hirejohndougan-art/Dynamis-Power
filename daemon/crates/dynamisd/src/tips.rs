//! Advice from readings (video decoded by the processor, screen dimming):
//! gathered on the service's usual readings, shown on Home until the person
//! dismisses it. Dismissed advice stays away for 30 days (`tips.json`).

use std::collections::BTreeMap;
use std::path::Path;

use dynamis_core::auto::Context;
use dynamis_core::display::Fit;
use dynamis_core::sensors::gpu::BusyMeter;
use dynamis_core::tips::{Reading, Watch};
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::write_atomic;

const FILE: &str = "tips.json";
const VERSION: u32 = 1;
const DISMISS_S: u64 = 30 * 86400;
/// A gap longer than this between readings starts the counts again.
const MAX_GAP_S: f64 = 120.0;

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    v: u32,
    /// Advice id → Unix time until which it stays hidden.
    dismissed: BTreeMap<String, u64>,
}

#[derive(Default)]
pub struct Tips {
    watch: Watch,
    gpu: BusyMeter,
    last: Option<f64>,
    dismissed: BTreeMap<String, u64>,
}

impl Tips {
    pub fn load(dir: &Path) -> Self {
        let dismissed = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
            .filter(|s| s.v == VERSION)
            .map(|s| s.dismissed)
            .unwrap_or_default();
        Tips {
            dismissed,
            ..Tips::default()
        }
    }

    pub fn tick(&mut self, root: &SysRoot, ctx: &Context, fit: Option<Fit>, now: f64) {
        let dt = self
            .last
            .replace(now)
            .map(|t| now - t)
            .filter(|d| *d > 0.0 && *d <= MAX_GAP_S)
            .unwrap_or(0.0);
        let video = ctx.media_playing || ctx.screen_inhibited;
        // The graphics chip is read only during video (cheap otherwise).
        let gpu = if video {
            self.gpu.sample(root, now)
        } else {
            self.gpu.reset();
            None
        };
        let screen_w = fit
            .filter(|f| f.r2 >= 0.3 && f.w_per_pct > 0.0)
            .zip(ctx.brightness_pct)
            .map(|(f, b)| f.w_per_pct * b);
        self.watch.add(Reading {
            dt,
            on_ac: ctx.on_ac,
            video,
            cpu_busy_pct: ctx.cpu_busy_pct,
            gpu_busy_pct: gpu,
            screen_w,
            power_w: ctx.power_w,
            light: !ctx.game_running && !ctx.screen_locked && ctx.cpu_busy_pct < 30.0,
        });
    }

    pub fn dismiss(&mut self, id: &str, dir: &Path, now: u64) -> Result<(), String> {
        if !matches!(id, "video_decode" | "screen_dimming") {
            return Err(format!("{id} is not a piece of advice"));
        }
        self.dismissed.insert(id.to_string(), now + DISMISS_S);
        self.dismissed.retain(|_, until| *until > now);
        let saved = Saved {
            v: VERSION,
            dismissed: self.dismissed.clone(),
        };
        write_atomic(
            &dir.join(FILE),
            &serde_json::to_string_pretty(&saved).map_err(|e| e.to_string())?,
        )
    }

    pub fn json(&self, now: u64) -> Value {
        let shown: Vec<_> = self
            .watch
            .tips()
            .into_iter()
            .filter(|t| self.dismissed.get(t.id).is_none_or(|u| *u <= now))
            .collect();
        json!(shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_tip_shown_until_dismissed() {
        let dir = std::env::temp_dir().join(format!("dynamisd-tips-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let rootdir = dir.join("root");
        let card = rootdir.join("sys/class/drm/card0/device");
        std::fs::create_dir_all(&card).unwrap();
        std::fs::write(card.join("gpu_busy_percent"), "3\n").unwrap();
        std::fs::create_dir_all(dir.join("state")).unwrap();
        let root = SysRoot::at(&rootdir);
        let mut t = Tips::load(&dir.join("state"));
        let mut ctx = Context {
            media_playing: true,
            cpu_busy_pct: 50.0,
            ..Context::default()
        };
        for i in 0..20 {
            t.tick(&root, &ctx, None, 1000.0 + i as f64 * 10.0);
        }
        ctx.media_playing = false;
        t.tick(&root, &ctx, None, 1200.0);
        assert_eq!(t.json(2000)[0]["id"], "video_decode");
        t.dismiss("video_decode", &dir.join("state"), 2000).unwrap();
        assert_eq!(t.json(2000), json!([]));
        assert_eq!(Tips::load(&dir.join("state")).dismissed.len(), 1, "kept");
        assert!(t.dismiss("anything", &dir.join("state"), 2000).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
