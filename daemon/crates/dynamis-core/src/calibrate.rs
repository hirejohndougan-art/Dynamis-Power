//! Calibration: testing one setting at a time on this machine (phase 4).
//!
//! An experiment alternates the current value (A) and a candidate (B) in
//! equal blocks — A B A B A B A B — while the laptop is in a steady, quiet
//! state. Alternating cancels slow drift (the battery warming, a background
//! task finishing) that would fool a single before/after comparison.
//!
//! For each block it averages whole-system power and stall (time tasks spent
//! waiting: the lag measure). Each B block is compared with the A blocks on
//! either side, giving one paired difference per B block. The verdict:
//!
//! - **Keep** — power is lower by at least the minimum effect, with ~95%
//!   confidence, *and* stall did not rise by more than the limit.
//! - **Reject** — stall rose past the limit (responsiveness is a hard limit,
//!   never traded for watts), or power is not lower.
//! - **Inconclusive** — too noisy to tell. The current value stays.
//!
//! This module is pure: it is told the time and the readings, and answers
//! what to do next. The daemon does the I/O. That is what lets the
//! statistics be tested on simulated laptops with known effects.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::auto::Workload;

/// One reading taken during an experiment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Whole-system power from the battery, watts.
    pub power_w: f64,
    /// Share of time tasks were stalled (CPU + memory + disk), percent.
    pub stall_pct: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub setting: String,
    /// A: the value before the experiment, restored if B is not kept.
    pub baseline: String,
    /// B: the candidate.
    pub value: String,
    pub block_s: f64,
    /// Readings in the first `settle_s` of a block are ignored while the
    /// machine adjusts to the switch.
    pub settle_s: f64,
    /// Even number; alternates starting with A.
    pub blocks: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    /// Smallest saving worth keeping, watts.
    pub min_effect_w: f64,
    /// ... and as a share of baseline power.
    pub min_effect_frac: f64,
    /// Largest stall increase tolerated, percentage points.
    pub max_stall_increase_pct: f64,
    /// t-statistic needed to call a difference real (≈95% with few pairs is
    /// a bit higher; 2.5 is used to stay conservative).
    pub min_t: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            min_effect_w: 0.05,
            min_effect_frac: 0.01,
            max_stall_increase_pct: 0.5,
            min_t: 2.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Keep,
    Reject,
    Inconclusive,
}

/// Mean difference (B − A) with a 95% interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub mean: f64,
    pub ci_low: f64,
    pub ci_high: f64,
    /// 0–1: how sure the sign is right.
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerSource {
    Battery,
    Ac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordContext {
    pub power_source: PowerSource,
    pub workload: Workload,
}

/// One experiment's result. Field names follow the shared results schema
/// (`schema/v1`), plus local-only fields (stall, time, reason).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub setting: String,
    pub value: String,
    pub baseline: String,
    pub context: RecordContext,
    /// Always "system_watts" for now.
    pub metric: String,
    pub effect: Effect,
    pub stall: Effect,
    pub baseline_w: f64,
    /// Paired differences used.
    pub samples: usize,
    pub verdict: Verdict,
    /// Plain-language reason for the verdict.
    pub reason: String,
    pub time: u64,
    /// Phase 5: the values of the settings tuned together (and of the
    /// profile's own settings) while this one was tested. Empty for
    /// settings tested on their own. See [`crate::tune`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub group: BTreeMap<String, String>,
    /// The kernel and firmware it was measured on (everyday problem §6).
    /// Absent in results from before this field: those are re-checked by
    /// age only. Local only, never shared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<crate::sysid::SystemId>,
}

impl Record {
    /// Whether this result should be measured again: it was measured on
    /// another kernel, firmware or processor rest limit, or more than a few
    /// months ago.
    pub fn stale(&self, now: &crate::sysid::SystemId, now_s: u64) -> bool {
        self.system
            .as_ref()
            .is_some_and(|s| now.change_from(s).is_some())
            || now_s.saturating_sub(self.time) > crate::sysid::RECHECK_AFTER_S
    }
}

/// What the daemon should do after a reading.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Keep going.
    Continue,
    /// Switch the setting to this value now (a new block starts).
    Apply(String),
    /// The experiment is over.
    /// Boxed: a record is much bigger than the other variants.
    Finish(Box<Record>),
}

#[derive(Debug, Clone)]
pub struct Experiment {
    pub plan: Plan,
    limits: Limits,
    context: RecordContext,
    block: usize,
    block_start: f64,
    blocks: Vec<Vec<Sample>>,
}

impl Experiment {
    /// Start with block 0 (A, the current value): nothing to apply yet.
    pub fn start(plan: Plan, limits: Limits, context: RecordContext, now: f64) -> Experiment {
        let n = plan.blocks.max(4) & !1; // even, at least 4
        Experiment {
            plan: Plan { blocks: n, ..plan },
            limits,
            context,
            block: 0,
            block_start: now,
            blocks: vec![Vec::new(); n],
        }
    }

    /// Which value block `i` uses.
    fn value_of(&self, i: usize) -> &str {
        if i.is_multiple_of(2) {
            &self.plan.baseline
        } else {
            &self.plan.value
        }
    }

    pub fn current_value(&self) -> &str {
        self.value_of(self.block)
    }

    /// Progress for people: (block now, blocks in total).
    pub fn progress(&self) -> (usize, usize) {
        (self.block + 1, self.plan.blocks)
    }

    pub fn observe(&mut self, now: f64, s: Sample) -> Action {
        if now - self.block_start >= self.plan.settle_s {
            self.blocks[self.block].push(s);
        }
        if now - self.block_start < self.plan.block_s {
            return Action::Continue;
        }
        self.block += 1;
        self.block_start = now;
        if self.block == self.plan.blocks {
            return Action::Finish(Box::new(self.analyse(now)));
        }
        Action::Apply(self.value_of(self.block).to_string())
    }

    /// Stop early (conditions changed). The result is inconclusive.
    pub fn abort(&self, why: &str, now: f64) -> Record {
        let mut r = self.analyse(now);
        r.verdict = Verdict::Inconclusive;
        r.reason = format!("stopped early: {why}");
        r
    }

    fn analyse(&self, now: f64) -> Record {
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
        let block_means = |f: fn(&Sample) -> f64| -> Vec<Option<f64>> {
            self.blocks
                .iter()
                .map(|b| (!b.is_empty()).then(|| mean(&b.iter().map(f).collect::<Vec<_>>())))
                .collect()
        };
        let power = block_means(|s| s.power_w);
        let stall = block_means(|s| s.stall_pct);

        // One difference per B block: B minus the mean of its A neighbours.
        let pairs = |m: &[Option<f64>]| -> Vec<f64> {
            (1..m.len())
                .step_by(2)
                .filter_map(|i| {
                    let b = m[i]?;
                    let before = m[i - 1];
                    let after = m.get(i + 1).copied().flatten();
                    let a = match (before, after) {
                        (Some(x), Some(y)) => (x + y) / 2.0,
                        (Some(x), None) | (None, Some(x)) => x,
                        (None, None) => return None,
                    };
                    Some(b - a)
                })
                .collect()
        };
        let dp = pairs(&power);
        let ds = pairs(&stall);
        let ep = effect(&dp);
        let es = effect(&ds);
        let baseline_w = mean(
            &power
                .iter()
                .step_by(2)
                .filter_map(|x| *x)
                .collect::<Vec<_>>(),
        );

        let l = self.limits;
        let needed = l.min_effect_w.max(l.min_effect_frac * baseline_w);
        let t_power = t_stat(&dp);
        let (verdict, reason) = if dp.len() < 3 {
            (Verdict::Inconclusive, "not enough readings".to_string())
        } else if es.mean > l.max_stall_increase_pct {
            (
                Verdict::Reject,
                format!(
                    "made the laptop less responsive (+{:.1} points of stall)",
                    es.mean
                ),
            )
        } else if ep.mean <= -needed && t_power <= -l.min_t {
            (
                Verdict::Keep,
                format!(
                    "saves {:.2} W (about {:.0}%)",
                    -ep.mean,
                    -ep.mean / baseline_w.max(0.01) * 100.0
                ),
            )
        } else if ep.ci_low >= -needed {
            // Even the optimistic end of the interval saves too little.
            (Verdict::Reject, "no useful saving".to_string())
        } else {
            (Verdict::Inconclusive, "too noisy to tell".to_string())
        };

        Record {
            setting: self.plan.setting.clone(),
            value: self.plan.value.clone(),
            baseline: self.plan.baseline.clone(),
            context: self.context,
            metric: "system_watts".into(),
            effect: ep,
            stall: es,
            baseline_w,
            samples: dp.len(),
            verdict,
            reason,
            time: now as u64,
            group: BTreeMap::new(),
            system: None,
        }
    }
}

fn mean_sd(d: &[f64]) -> (f64, f64) {
    let n = d.len() as f64;
    if d.is_empty() {
        return (0.0, 0.0);
    }
    let m = d.iter().sum::<f64>() / n;
    let var = if d.len() > 1 {
        d.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0)
    } else {
        0.0
    };
    (m, var.sqrt())
}

fn t_stat(d: &[f64]) -> f64 {
    let (m, sd) = mean_sd(d);
    let se = sd / (d.len().max(1) as f64).sqrt();
    if se == 0.0 {
        // Identical differences: perfectly consistent, sign decides.
        if m == 0.0 {
            0.0
        } else {
            m.signum() * f64::INFINITY
        }
    } else {
        m / se
    }
}

/// Two-sided 97.5% Student t quantile for small samples.
fn t_crit(df: usize) -> f64 {
    const T: [f64; 10] = [12.71, 4.30, 3.18, 2.78, 2.57, 2.45, 2.36, 2.31, 2.26, 2.23];
    T.get(df.saturating_sub(1)).copied().unwrap_or(2.0)
}

fn effect(d: &[f64]) -> Effect {
    let (m, sd) = mean_sd(d);
    let n = d.len();
    let half = if n > 1 {
        t_crit(n - 1) * sd / (n as f64).sqrt()
    } else {
        f64::INFINITY
    };
    let t = t_stat(d).abs();
    // A rough, monotone mapping from |t| to confidence, for people; the
    // verdict itself uses the t threshold.
    let confidence = if t.is_infinite() {
        1.0
    } else {
        1.0 - (-0.5 * t * t).exp()
    };
    let r = |x: f64| (x * 1000.0).round() / 1000.0;
    Effect {
        mean: r(m),
        ci_low: if half.is_finite() {
            r(m - half)
        } else {
            f64::MIN
        },
        ci_high: if half.is_finite() {
            r(m + half)
        } else {
            f64::MAX
        },
        confidence: r(confidence),
    }
}

/// Values worth trying for each setting, best guess first, for the battery
/// goal. Settings the standard profiles manage (`cpu.epp`,
/// `platform.profile`) are left to phase 5, where settings that interact are
/// tuned together.
pub fn candidates(setting: &str) -> &'static [&'static str] {
    match setting {
        "kernel.nmi_watchdog" => &["off"],
        "audio.power_save" => &["1", "10"],
        "vm.dirty_writeback" => &["1500", "3000"],
        "pcie.aspm" => &["powersave"],
        "storage.sata_alpm" => &["med_power_with_dipm"],
        "wifi.power_save" => &["on"],
        "gpu.dgpu_runtime_pm" => &["on"],
        "storage.nvme_power_states" => &["on"],
        // cpu.turbo and cpu.max_perf_pct are tuned together: crate::tune.
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny deterministic noise source (no dependency needed for tests).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
        }
    }

    /// Simulate an experiment on a laptop where B changes power by `dp` W and
    /// stall by `ds` points, with noise and slow drift.
    fn simulate(dp: f64, ds: f64, noise: f64, seed: u64) -> Record {
        let plan = Plan {
            setting: "kernel.nmi_watchdog".into(),
            baseline: "on".into(),
            value: "off".into(),
            block_s: 60.0,
            settle_s: 10.0,
            blocks: 8,
        };
        let ctx = RecordContext {
            power_source: PowerSource::Battery,
            workload: Workload::LightInteractive,
        };
        let mut e = Experiment::start(plan, Limits::default(), ctx, 0.0);
        let mut rng = Lcg(seed);
        let mut t = 0.0;
        loop {
            t += 5.0;
            let b = e.current_value() == "off";
            let drift = t / 480.0 * 0.4; // battery warming: +0.4 W over the test
            let s = Sample {
                power_w: 6.0 + drift + if b { dp } else { 0.0 } + noise * rng.next(),
                stall_pct: 1.0 + if b { ds } else { 0.0 } + 0.2 * rng.next(),
            };
            match e.observe(t, s) {
                Action::Finish(r) => return *r,
                Action::Apply(_) | Action::Continue => {}
            }
        }
    }

    #[test]
    fn keeps_a_real_saving() {
        let r = simulate(-0.3, 0.0, 0.3, 1);
        assert_eq!(r.verdict, Verdict::Keep, "{r:?}");
        assert!(r.effect.ci_high < 0.0);
        assert_eq!(r.samples, 4);
    }

    #[test]
    fn rejects_no_effect_despite_drift() {
        // Drift alone would look like a 0.4 W change in a before/after test.
        let r = simulate(0.0, 0.0, 0.1, 2);
        assert_ne!(r.verdict, Verdict::Keep, "{r:?}");
        assert!(
            r.effect.mean.abs() < 0.1,
            "alternating cancels drift: {:?}",
            r.effect
        );
    }

    #[test]
    fn responsiveness_is_a_hard_limit() {
        // Saves a whole watt but adds 2 points of stall: rejected anyway.
        let r = simulate(-1.0, 2.0, 0.2, 3);
        assert_eq!(r.verdict, Verdict::Reject);
        assert!(r.reason.contains("less responsive"));
    }

    #[test]
    fn noisy_gauge_is_inconclusive_not_keep() {
        let r = simulate(-0.1, 0.0, 3.0, 4);
        assert_ne!(r.verdict, Verdict::Keep, "{r:?}");
    }

    #[test]
    fn many_seeds_never_keep_a_useless_change() {
        // False keeps would slowly accumulate junk settings.
        let keeps = (0..200)
            .filter(|s| simulate(0.0, 0.0, 0.3, *s + 100).verdict == Verdict::Keep)
            .count();
        eprintln!("false keeps: {keeps} of 200");
        assert!(keeps <= 6, "{keeps} false keeps out of 200");
    }

    #[test]
    fn many_seeds_usually_keep_a_real_saving() {
        let keeps = (0..200)
            .filter(|s| simulate(-0.3, 0.0, 0.3, *s + 500).verdict == Verdict::Keep)
            .count();
        eprintln!("real 0.3 W saving kept: {keeps} of 200");
        assert!(keeps >= 150, "only {keeps} of 200");
    }
}
