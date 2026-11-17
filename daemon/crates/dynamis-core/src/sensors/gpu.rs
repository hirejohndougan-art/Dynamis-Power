//! Whether the graphics chip keeps up (decision 0026).
//!
//! Counting dropped frames needs each app or the compositor to report them,
//! and there is no common way to ask. What the kernel does show is whether
//! the graphics chip is **fully busy at its speed limit**: busy all of a
//! reading with no idle gaps between frames, while running at the highest
//! speed allowed. A chip that keeps up with a video has idle time between
//! frames; one that does not is busy throughout. That share of readings is
//! used as the graphics "lag" when testing graphics speed.
//!
//! - **Intel (i915):** busy from the change in RC6 (idle) residency between
//!   readings; at the limit when the actual frequency is within 50 MHz of
//!   the maximum allowed.
//! - **AMD (amdgpu):** `device/gpu_busy_percent` (the driver keeps to the
//!   power level itself).
//!
//! Read-only.

use crate::fs::SysRoot;

/// Busy share above which a reading counts as "fully busy".
const BUSY_PCT: f64 = 95.0;

fn cards(root: &SysRoot) -> Vec<String> {
    let mut v: Vec<String> = root
        .list_dir("/sys/class/drm")
        .into_iter()
        .filter(|c| {
            c.strip_prefix("card")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(|c| format!("/sys/class/drm/{c}"))
        .collect();
    v.sort();
    v
}

fn rc6_ms(root: &SysRoot, base: &str) -> Option<u64> {
    ["gt/gt0/rc6_residency_ms", "power/rc6_residency_ms"]
        .iter()
        .find_map(|f| root.read_u64(&format!("{base}/{f}")).ok())
}

/// Whether this laptop's graphics can be measured this way.
pub fn available(root: &SysRoot) -> bool {
    cards(root).iter().any(|b| {
        (root.exists(&format!("{b}/gt_act_freq_mhz")) && rc6_ms(root, b).is_some())
            || root.exists(&format!("{b}/device/gpu_busy_percent"))
    })
}

/// Readings over time: each [`GpuMeter::sample`] says whether the chip was
/// fully busy at its limit since the previous one (100) or not (0).
#[derive(Debug, Default)]
pub struct GpuMeter {
    last: Option<(f64, u64)>,
}

impl GpuMeter {
    pub fn sample(&mut self, root: &SysRoot, now: f64) -> Option<f64> {
        for b in cards(root) {
            if let Ok(busy) = root.read_u64(&format!("{b}/device/gpu_busy_percent")) {
                return Some(if busy as f64 >= BUSY_PCT { 100.0 } else { 0.0 });
            }
            let (Ok(act), Ok(max), Some(rc6)) = (
                root.read_u64(&format!("{b}/gt_act_freq_mhz")),
                root.read_u64(&format!("{b}/gt_max_freq_mhz")),
                rc6_ms(root, &b),
            ) else {
                continue;
            };
            let prev = self.last.replace((now, rc6));
            let (t0, r0) = prev?;
            let dt_ms = (now - t0) * 1000.0;
            if dt_ms <= 0.0 {
                return None;
            }
            let idle_pct = (rc6.saturating_sub(r0) as f64 / dt_ms * 100.0).clamp(0.0, 100.0);
            let at_limit = act + 50 >= max;
            return Some(if at_limit && 100.0 - idle_pct >= BUSY_PCT {
                100.0
            } else {
                0.0
            });
        }
        None
    }
}

/// How busy the graphics chip was since the previous reading, 0–100
/// (Intel: from RC6 idle residency; AMD: `gpu_busy_percent`). Used for the
/// video decoding tip.
#[derive(Debug, Default)]
pub struct BusyMeter {
    last: Option<(f64, u64)>,
}

impl BusyMeter {
    pub fn sample(&mut self, root: &SysRoot, now: f64) -> Option<f64> {
        for b in cards(root) {
            if let Ok(busy) = root.read_u64(&format!("{b}/device/gpu_busy_percent")) {
                return Some((busy as f64).clamp(0.0, 100.0));
            }
            let Some(rc6) = rc6_ms(root, &b) else {
                continue;
            };
            let (t0, r0) = self.last.replace((now, rc6))?;
            let dt_ms = (now - t0) * 1000.0;
            if dt_ms <= 0.0 {
                return None;
            }
            let idle = (rc6.saturating_sub(r0) as f64 / dt_ms * 100.0).clamp(0.0, 100.0);
            return Some(100.0 - idle);
        }
        None
    }

    /// Start again (after a gap, so the next reading is not over the gap).
    pub fn reset(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_at_the_limit_counts() {
        let dir = std::env::temp_dir().join(format!("dynamis-gpu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let card = dir.join("sys/class/drm/card0");
        std::fs::create_dir_all(card.join("gt/gt0")).unwrap();
        let w = |f: &str, v: u64| std::fs::write(card.join(f), format!("{v}\n")).unwrap();
        w("gt_max_freq_mhz", 900);
        w("gt_act_freq_mhz", 900);
        w("gt/gt0/rc6_residency_ms", 1000);
        let root = SysRoot::at(&dir);
        assert!(available(&root));
        let mut m = GpuMeter::default();
        assert_eq!(m.sample(&root, 0.0), None, "needs two readings");
        // 10 s at the limit with only 0.1 s idle: fully busy.
        w("gt/gt0/rc6_residency_ms", 1100);
        assert_eq!(m.sample(&root, 10.0), Some(100.0));
        // Idle half the time: keeps up.
        w("gt/gt0/rc6_residency_ms", 6100);
        assert_eq!(m.sample(&root, 20.0), Some(0.0));
        // Busy but below its limit: keeps up (it could go faster).
        w("gt_act_freq_mhz", 600);
        w("gt/gt0/rc6_residency_ms", 6150);
        assert_eq!(m.sample(&root, 30.0), Some(0.0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
