//! Advice the app shows when a reading suggests it (decision 0027, "Advice").
//! Nothing here changes anything; the person decides.
//!
//! - **Video decoded by the processor**: during a video lasting two minutes
//!   or more, the processor stayed busy (30% of all cores or more on
//!   average) while the graphics chip stayed nearly idle (under 15%). Video
//!   decoded by the graphics chip's own video engine uses far less power, so
//!   the browser or player is probably decoding it in software.
//! - **Screen dimming**: on battery at light use, the screen used 35% or
//!   more of the power over half an hour (from the learned screen model). The
//!   desktop's own dim and screen-off times are the person's settings: the
//!   app only says where to shorten them.

use serde::Serialize;

pub const VIDEO_MIN_S: f64 = 120.0;
pub const VIDEO_CPU_PCT: f64 = 30.0;
pub const VIDEO_GPU_PCT: f64 = 15.0;
pub const SCREEN_WINDOW_S: f64 = 1800.0;
pub const SCREEN_SHARE: f64 = 0.35;

/// One reading's facts.
#[derive(Debug, Clone, Copy, Default)]
pub struct Reading {
    /// Seconds since the previous reading (0 for the first, or after a gap).
    pub dt: f64,
    pub on_ac: bool,
    pub video: bool,
    pub cpu_busy_pct: f64,
    pub gpu_busy_pct: Option<f64>,
    /// The screen's estimated watts now and the whole laptop's, on battery.
    pub screen_w: Option<f64>,
    pub power_w: Option<f64>,
    /// Light use: not heavy work, not a game, the screen not locked.
    pub light: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tip {
    /// `video_decode` or `screen_dimming`.
    pub id: &'static str,
    /// The figure behind it: the processor's busy share during video, or the
    /// screen's share of the power, in percent.
    pub pct: f64,
}

#[derive(Debug, Default)]
struct Sums {
    s: f64,
    a: f64,
    b: f64,
}

#[derive(Debug, Default)]
pub struct Watch {
    video: Sums,
    screen: Sums,
    pub video_tip: Option<Tip>,
    pub screen_tip: Option<Tip>,
}

impl Watch {
    pub fn add(&mut self, r: Reading) {
        // Video: a session ends when the video stops.
        if r.video {
            if let Some(g) = r.gpu_busy_pct.filter(|_| r.dt > 0.0) {
                self.video.s += r.dt;
                self.video.a += r.cpu_busy_pct * r.dt;
                self.video.b += g * r.dt;
            }
        } else if self.video.s > 0.0 {
            let v = std::mem::take(&mut self.video);
            if v.s >= VIDEO_MIN_S {
                let (cpu, gpu) = (v.a / v.s, v.b / v.s);
                // A later video decoded well clears the tip.
                self.video_tip = (cpu >= VIDEO_CPU_PCT && gpu < VIDEO_GPU_PCT).then(|| Tip {
                    id: "video_decode",
                    pct: cpu.round(),
                });
            }
        }
        // Screen: light use on battery, half an hour at a time.
        if let (false, true, Some(sw), Some(pw)) =
            (r.on_ac, r.light && !r.video, r.screen_w, r.power_w)
        {
            if r.dt > 0.0 && pw > 0.5 {
                self.screen.s += r.dt;
                self.screen.a += (sw / pw).clamp(0.0, 1.0) * r.dt;
            }
        }
        if self.screen.s >= SCREEN_WINDOW_S {
            let share = self.screen.a / self.screen.s;
            self.screen = Sums::default();
            self.screen_tip = (share >= SCREEN_SHARE).then(|| Tip {
                id: "screen_dimming",
                pct: (share * 100.0).round(),
            });
        }
    }

    pub fn tips(&self) -> Vec<Tip> {
        self.video_tip
            .iter()
            .chain(&self.screen_tip)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(cpu: f64, gpu: f64) -> Reading {
        Reading {
            dt: 10.0,
            video: true,
            cpu_busy_pct: cpu,
            gpu_busy_pct: Some(gpu),
            ..Reading::default()
        }
    }

    #[test]
    fn video_with_a_busy_processor_and_idle_graphics() {
        let mut w = Watch::default();
        for _ in 0..15 {
            w.add(video(45.0, 5.0));
        }
        assert!(w.tips().is_empty(), "said when the video ends");
        w.add(Reading::default());
        assert_eq!(
            w.tips(),
            [Tip {
                id: "video_decode",
                pct: 45.0
            }]
        );
        // A later video decoded by the graphics chip clears it.
        for _ in 0..15 {
            w.add(video(8.0, 30.0));
        }
        w.add(Reading::default());
        assert!(w.tips().is_empty());
        // Too short to judge: nothing changes.
        for _ in 0..5 {
            w.add(video(60.0, 1.0));
        }
        w.add(Reading::default());
        assert!(w.tips().is_empty());
    }

    #[test]
    fn screen_taking_a_large_share_on_battery() {
        let mut w = Watch::default();
        let r = Reading {
            dt: 60.0,
            light: true,
            screen_w: Some(2.0),
            power_w: Some(5.0),
            ..Reading::default()
        };
        for _ in 0..29 {
            w.add(r);
        }
        assert!(w.tips().is_empty());
        w.add(r);
        assert_eq!(
            w.tips(),
            [Tip {
                id: "screen_dimming",
                pct: 40.0
            }]
        );
        // Plugged in: not counted.
        for _ in 0..40 {
            w.add(Reading { on_ac: true, ..r });
        }
        assert_eq!(w.tips().len(), 1);
    }
}
