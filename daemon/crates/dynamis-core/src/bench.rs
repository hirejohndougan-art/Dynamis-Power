//! The head-to-head benchmark (`dynamis bench`): the same workloads, on
//! battery, under different power managers (stock settings, TLP, Dynamis
//! Power), measured the same way so the results can be compared.
//!
//! This module holds the measuring and the arithmetic; the workloads run in
//! the `dynamis` command. Read-only: nothing here changes a setting.
//!
//! Power is taken from the drop in the battery's stored energy over a run
//! when the gauge reports it (the most honest figure: it is what the battery
//! actually lost), and otherwise from the average of the power readings.

use serde::{Deserialize, Serialize};

use crate::fs::SysRoot;
use crate::sensors;

/// Version of the saved results file.
pub const VERSION: u32 = 1;
/// The first readings of each workload are not counted, so a burst from
/// starting it (or from the last workload) does not skew it.
pub const SETTLE_S: f64 = 60.0;
/// Brightness that moves more than this many points during a run makes it
/// suspect (the screen dimmed or someone pressed the keys).
const BRIGHTNESS_DRIFT: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Workload {
    /// Nothing running, screen on.
    Idle,
    /// A small steady load, like reading and light browsing.
    Light,
    /// A video playing.
    Video,
    /// A fixed amount of heavy work on every core: how long it takes and
    /// how much energy it uses.
    Heavy,
}

impl Workload {
    pub const ALL: [Workload; 4] = [
        Workload::Idle,
        Workload::Light,
        Workload::Video,
        Workload::Heavy,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Workload::Idle => "idle",
            Workload::Light => "light",
            Workload::Video => "video",
            Workload::Heavy => "heavy",
        }
    }
}

/// One reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Seconds since the workload started.
    pub t: f64,
    pub watts: Option<f64>,
    pub energy_wh: Option<f64>,
    pub battery_pct: Option<f64>,
    pub temp_c: Option<f64>,
    pub brightness_pct: Option<f64>,
    pub on_ac: bool,
}

/// Read the battery, temperature and screen now.
pub fn sample(root: &SysRoot, t: f64) -> Sample {
    let ps = sensors::power::read(root);
    let bat = ps.batteries.first();
    let energy = {
        let all: Vec<f64> = ps
            .batteries
            .iter()
            .filter_map(|b| b.energy_now_wh)
            .collect();
        (!all.is_empty()).then(|| all.iter().sum())
    };
    Sample {
        t,
        watts: ps.total_battery_power_w(),
        energy_wh: energy,
        battery_pct: bat.and_then(|b| b.capacity_pct).map(f64::from),
        temp_c: sensors::thermal::max_c(&sensors::thermal::read(root)),
        brightness_pct: sensors::backlight::read(root).map(|b| b.percent),
        on_ac: ps.on_ac(),
    }
}

/// The energy the batteries hold when full, watt-hours.
pub fn full_wh(root: &SysRoot) -> Option<f64> {
    let all: Vec<f64> = sensors::power::read(root)
        .batteries
        .iter()
        .filter_map(|b| b.energy_full_wh)
        .collect();
    (!all.is_empty()).then(|| all.iter().sum())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkloadResult {
    pub workload: Workload,
    /// Seconds counted (after settling).
    pub seconds: f64,
    /// Average power from the battery, watts.
    pub avg_w: Option<f64>,
    /// `energy` (the battery's drop in stored energy) or `readings` (the
    /// average of the power readings).
    pub avg_from: Option<String>,
    /// Energy used in the counted part, watt-hours.
    pub energy_wh: Option<f64>,
    /// How long a full battery would last at this power, hours.
    pub hours_on_full: Option<f64>,
    pub max_temp_c: Option<f64>,
    /// Heavy work only: seconds to finish the fixed amount of work.
    pub work_seconds: Option<f64>,
    /// Why this result should not be trusted, in plain words. Empty when
    /// nothing went wrong.
    pub problems: Vec<String>,
    pub samples: Vec<Sample>,
}

/// Summarise one workload's readings.
pub fn summarise(
    workload: Workload,
    samples: Vec<Sample>,
    full_wh: Option<f64>,
    work_seconds: Option<f64>,
    settle_s: f64,
) -> WorkloadResult {
    let counted: Vec<&Sample> = samples.iter().filter(|s| s.t >= settle_s).collect();
    let mut problems = Vec::new();
    if counted.len() < 3 {
        problems.push("too few readings".to_string());
    }
    if samples.iter().any(|s| s.on_ac) {
        problems.push("the charger was plugged in during the run".to_string());
    }
    let bright: Vec<f64> = counted.iter().filter_map(|s| s.brightness_pct).collect();
    if let (Some(lo), Some(hi)) = (
        bright.iter().copied().reduce(f64::min),
        bright.iter().copied().reduce(f64::max),
    ) {
        if hi - lo > BRIGHTNESS_DRIFT {
            problems.push(format!(
                "the screen brightness changed during the run ({lo:.0}% to {hi:.0}%)"
            ));
        }
    }
    let (t0, t1) = match (counted.first(), counted.last()) {
        (Some(a), Some(b)) => (a.t, b.t),
        _ => (0.0, 0.0),
    };
    let seconds = (t1 - t0).max(0.0);
    let energy = match (
        counted.first().and_then(|s| s.energy_wh),
        counted.last().and_then(|s| s.energy_wh),
    ) {
        // A gauge that did not move is not a measurement of zero.
        (Some(a), Some(b)) if a - b > 0.01 && seconds > 0.0 => Some(a - b),
        _ => None,
    };
    let readings: Vec<f64> = counted
        .iter()
        .filter_map(|s| s.watts)
        .filter(|w| *w > 0.0)
        .collect();
    let (avg_w, avg_from) = if let Some(e) = energy {
        (Some(e / (seconds / 3600.0)), Some("energy".to_string()))
    } else if !readings.is_empty() {
        (
            Some(readings.iter().sum::<f64>() / readings.len() as f64),
            Some("readings".to_string()),
        )
    } else {
        problems.push("the battery reported no power".to_string());
        (None, None)
    };
    let energy_wh = energy.or_else(|| avg_w.map(|w| w * seconds / 3600.0));
    WorkloadResult {
        workload,
        seconds,
        avg_w,
        avg_from,
        energy_wh,
        hours_on_full: avg_w
            .zip(full_wh)
            .filter(|(w, _)| *w > 0.0)
            .map(|(w, f)| f / w),
        max_temp_c: counted.iter().filter_map(|s| s.temp_c).reduce(f64::max),
        work_seconds,
        problems,
        samples,
    }
}

/// The machine and settings a run was made on: enough to compare like with
/// like, nothing personal.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub model: Option<String>,
    pub cpu: Option<String>,
    pub kernel: Option<String>,
    /// The firmware power mode and the processor's speed preference at the
    /// start (what the power manager chose).
    pub power_mode: Option<String>,
    pub speed_preference: Option<String>,
}

pub fn machine(root: &SysRoot) -> Machine {
    let inv = crate::inventory::read(root);
    let read = |id: &str| crate::settings::by_id(id).and_then(|s| s.read(root).ok());
    Machine {
        model: inv.machine.model,
        cpu: inv.cpu.model,
        kernel: inv.software.kernel,
        power_mode: read("platform.profile"),
        speed_preference: read("cpu.epp"),
    }
}

/// One `dynamis bench run`: a label for what was being compared, and each
/// workload's result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub v: u32,
    /// What ran the laptop: `stock`, `tlp`, `dynamis`, or anything else.
    pub label: String,
    /// The day it was made (UTC), day precision only.
    pub day: String,
    pub machine: Machine,
    pub results: Vec<WorkloadResult>,
}

/// Mean and spread (half the range) of some numbers.
fn mean_spread(v: &[f64]) -> Option<(f64, f64)> {
    if v.is_empty() {
        return None;
    }
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let lo = v.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Some((mean, (hi - lo) / 2.0))
}

fn fmt_ms(x: Option<(f64, f64)>, digits: usize, unit: &str) -> String {
    match x {
        Some((m, s)) if s > 0.0 => format!("{m:.digits$} ± {s:.digits$}{unit}"),
        Some((m, _)) => format!("{m:.digits$}{unit}"),
        None => "–".into(),
    }
}

/// A plain-text table comparing runs: one block per workload, one row per
/// label (in the order first seen), repeats of a label averaged with their
/// spread, and each label's power against the first label's. Results with
/// problems are left out and counted.
pub fn compare(runs: &[Run]) -> String {
    let mut labels: Vec<&str> = Vec::new();
    for r in runs {
        if !labels.contains(&r.label.as_str()) {
            labels.push(&r.label);
        }
    }
    let mut out = String::new();
    let models: Vec<&str> = runs
        .iter()
        .filter_map(|r| r.machine.model.as_deref())
        .collect();
    if models.windows(2).any(|w| w[0] != w[1]) {
        out.push_str("Warning: these runs come from different laptop models.\n\n");
    }
    for w in Workload::ALL {
        let mut rows = Vec::new();
        let mut base: Option<f64> = None;
        for label in &labels {
            let res: Vec<&WorkloadResult> = runs
                .iter()
                .filter(|r| r.label == *label)
                .flat_map(|r| r.results.iter())
                .filter(|x| x.workload == w)
                .collect();
            if res.is_empty() {
                continue;
            }
            let ok: Vec<&&WorkloadResult> = res.iter().filter(|x| x.problems.is_empty()).collect();
            let watts: Vec<f64> = ok.iter().filter_map(|x| x.avg_w).collect();
            let hours: Vec<f64> = ok.iter().filter_map(|x| x.hours_on_full).collect();
            let work: Vec<f64> = ok.iter().filter_map(|x| x.work_seconds).collect();
            let wh: Vec<f64> = ok.iter().filter_map(|x| x.energy_wh).collect();
            let w_ms = mean_spread(&watts);
            let vs = match (base, w_ms) {
                (None, Some((m, _))) => {
                    base = Some(m);
                    "(baseline)".to_string()
                }
                (Some(b), Some((m, _))) if b > 0.0 => format!("{:+.0}% power", (m - b) / b * 100.0),
                _ => String::new(),
            };
            let skipped = res.len() - ok.len();
            let mut row = format!(
                "  {:<10} {:>14}  {:>14} on a full battery",
                label,
                fmt_ms(w_ms, 2, " W"),
                fmt_ms(mean_spread(&hours), 1, " h"),
            );
            if w == Workload::Heavy {
                row.push_str(&format!(
                    "  {:>12} to finish, {}",
                    fmt_ms(mean_spread(&work), 0, " s"),
                    fmt_ms(mean_spread(&wh), 2, " Wh")
                ));
            }
            row.push_str(&format!("  {vs}"));
            if skipped > 0 {
                row.push_str(&format!("  ({skipped} run(s) left out: problems)"));
            }
            rows.push(row);
        }
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("{}\n", w.name()));
        for r in rows {
            out.push_str(r.trim_end());
            out.push('\n');
        }
        out.push('\n');
    }
    if out.is_empty() {
        out.push_str("No results to compare.\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: f64, w: f64, e: f64) -> Sample {
        Sample {
            t,
            watts: Some(w),
            energy_wh: Some(e),
            battery_pct: Some(80.0),
            temp_c: Some(45.0),
            brightness_pct: Some(50.0),
            on_ac: false,
        }
    }

    #[test]
    fn power_from_the_battery_energy_drop() {
        // 10 minutes counted, 1 Wh lost: 6 W. Readings say 5 W but the
        // stored energy wins.
        let samples: Vec<Sample> = (0..=70)
            .map(|i| {
                s(
                    i as f64 * 10.0,
                    5.0,
                    50.0 - (i as f64 * 10.0 - 60.0).max(0.0) / 600.0,
                )
            })
            .collect();
        let r = summarise(Workload::Idle, samples, Some(42.0), None, 60.0);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!((r.avg_w.unwrap() - 6.0).abs() < 1e-6, "{:?}", r.avg_w);
        assert_eq!(r.avg_from.as_deref(), Some("energy"));
        assert!((r.hours_on_full.unwrap() - 7.0).abs() < 1e-6);
    }

    #[test]
    fn falls_back_to_readings_and_flags_problems() {
        let mut samples: Vec<Sample> = (0..=20).map(|i| s(i as f64 * 10.0, 4.0, 50.0)).collect();
        samples[15].on_ac = true;
        samples[18].brightness_pct = Some(80.0);
        let r = summarise(Workload::Light, samples, Some(40.0), None, 60.0);
        assert_eq!(r.avg_from.as_deref(), Some("readings"));
        assert_eq!(r.avg_w, Some(4.0));
        assert_eq!(r.problems.len(), 2, "{:?}", r.problems);
    }

    #[test]
    fn compares_labels_against_the_first() {
        let run = |label: &str, w: f64, heavy_s: f64| Run {
            v: VERSION,
            label: label.into(),
            day: "2026-11-16".into(),
            machine: Machine {
                model: Some("E410MA".into()),
                ..Machine::default()
            },
            results: vec![
                summarise(
                    Workload::Idle,
                    (0..=20).map(|i| s(i as f64 * 10.0, w, 50.0)).collect(),
                    Some(42.0),
                    None,
                    60.0,
                ),
                summarise(
                    Workload::Heavy,
                    (0..=20)
                        .map(|i| s(i as f64 * 10.0, w * 3.0, 50.0))
                        .collect(),
                    Some(42.0),
                    Some(heavy_s),
                    60.0,
                ),
            ],
        };
        let t = compare(&[
            run("stock", 5.0, 200.0),
            run("tlp", 4.5, 230.0),
            run("dynamis", 4.0, 205.0),
            run("dynamis", 4.2, 207.0),
        ]);
        assert!(t.contains("stock") && t.contains("(baseline)"), "{t}");
        assert!(t.contains("-10% power"), "{t}");
        assert!(t.contains("4.10 ± 0.10 W"), "{t}");
        assert!(t.contains("206 ± 1 s to finish"), "{t}");
    }
}
