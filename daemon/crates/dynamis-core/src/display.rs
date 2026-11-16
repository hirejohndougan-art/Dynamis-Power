//! Display power model: how many watts each step of screen brightness costs
//! on this laptop.
//!
//! Learned passively: whenever the laptop is on battery and quiet (idle or
//! light use), each reading of brightness and whole-system power is added to
//! a running least-squares fit, `power = base + slope × brightness`. Nothing
//! is changed to learn it; people change brightness themselves often enough.
//!
//! The screen is often the single biggest power user on a small laptop, so
//! this is what lets the app say "dimming to 40% gives about 25 more
//! minutes" later. Readings are only used once brightness has varied enough
//! to separate its effect from everything else.

use serde::{Deserialize, Serialize};

const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DisplayModel {
    pub v: u32,
    n: f64,
    sx: f64,
    sy: f64,
    sxx: f64,
    sxy: f64,
    syy: f64,
    min_x: f64,
    max_x: f64,
}

/// What the model says, once it has enough to say anything.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fit {
    /// Watts per percentage point of brightness.
    pub w_per_pct: f64,
    /// Estimated power with the screen at 0% (everything else).
    pub base_w: f64,
    /// How much of the variation in power brightness explains, 0–1.
    pub r2: f64,
    pub samples: u64,
    /// Lowest and highest brightness seen.
    pub range_pct: (f64, f64),
}

impl DisplayModel {
    pub fn new() -> Self {
        DisplayModel {
            v: FORMAT_VERSION,
            min_x: f64::MAX,
            max_x: f64::MIN,
            ..Default::default()
        }
    }

    pub fn add(&mut self, brightness_pct: f64, power_w: f64) {
        if !(0.0..=100.0).contains(&brightness_pct) || !(0.0..200.0).contains(&power_w) {
            return;
        }
        if self.n == 0.0 {
            self.min_x = f64::MAX;
            self.max_x = f64::MIN;
        }
        self.v = FORMAT_VERSION;
        self.n += 1.0;
        self.sx += brightness_pct;
        self.sy += power_w;
        self.sxx += brightness_pct * brightness_pct;
        self.sxy += brightness_pct * power_w;
        self.syy += power_w * power_w;
        self.min_x = self.min_x.min(brightness_pct);
        self.max_x = self.max_x.max(brightness_pct);
    }

    /// `None` until at least 30 readings spread over 20+ points of brightness.
    pub fn fit(&self) -> Option<Fit> {
        if self.n < 30.0 || self.max_x - self.min_x < 20.0 {
            return None;
        }
        let n = self.n;
        let vx = self.sxx - self.sx * self.sx / n;
        let vy = self.syy - self.sy * self.sy / n;
        let cxy = self.sxy - self.sx * self.sy / n;
        if vx <= 0.0 {
            return None;
        }
        let slope = cxy / vx;
        let base = (self.sy - slope * self.sx) / n;
        let r2 = if vy > 0.0 {
            (cxy * cxy / (vx * vy)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let r = |x: f64, p: f64| (x * p).round() / p;
        Some(Fit {
            w_per_pct: r(slope, 10000.0),
            base_w: r(base, 100.0),
            r2: r(r2, 100.0),
            samples: n as u64,
            range_pct: (self.min_x, self.max_x),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_the_slope() {
        let mut m = DisplayModel::new();
        for i in 0..20 {
            assert!(m.fit().is_none() || i >= 15);
            m.add(50.0, 5.0);
        }
        assert!(
            m.fit().is_none(),
            "brightness never changed: nothing to learn"
        );
        // 0.03 W per point, 4 W base, with a little noise.
        for i in 0..60 {
            let b = 20.0 + (i % 7) as f64 * 10.0;
            m.add(b, 4.0 + 0.03 * b + if i % 2 == 0 { 0.05 } else { -0.05 });
        }
        let f = m.fit().unwrap();
        assert!((f.w_per_pct - 0.03).abs() < 0.005, "{f:?}");
        assert!((f.base_w - 4.0).abs() < 0.3, "{f:?}");
        assert!(f.r2 > 0.8);
    }

    #[test]
    fn ignores_impossible_readings() {
        let mut m = DisplayModel::new();
        m.add(150.0, 5.0);
        m.add(50.0, -1.0);
        assert_eq!(m.n, 0.0);
    }
}
