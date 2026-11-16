//! Tuning processor settings that affect each other (phase 5).
//!
//! Phase 4 tests each setting on its own. Some processor settings interact:
//! turning turbo boost off can make a laptop feel slow at full speed, yet
//! cost nothing once the speed limit is already lower, and how much either
//! saves depends on the speed preference the power mode sets. Testing each
//! once, from wherever the others happened to be, misses that.
//!
//! The method is **coordinate descent with re-testing**, built on the same
//! experiments as phase 4:
//!
//! - The settings tuned together are [`GROUP`]. The power mode's own settings
//!   ([`CONTEXT`]) are not tuned here, but every result records them, and
//!   results only count for the same power mode settings.
//! - From the current values, try one step for one setting (turbo the other
//!   way; the speed limit one step up or down on [`grid`]). Each step is an
//!   ordinary experiment: one change, a trial, A/B blocks, kept only if it
//!   saves power with confidence and adds no lag.
//! - A result is tied to the values of every other setting when it was
//!   measured. When one setting moves, results for the others no longer
//!   apply, so their steps are tried again from the new point.
//! - It stops when no single step from the current values helps. Every kept
//!   step lowers measured power by at least the minimum effect, so it cannot
//!   go round in circles; [`MAX_KEEPS`] caps it anyway.
//!
//! Only one setting changes at a time, so every safety rail of a single
//! change (journal, trial, health monitor, crash recovery) still applies.
//! Changing two at once could find a few more combinations, but would mean a
//! trial that changes several settings together; that is left out on
//! purpose (decision 0014).
//!
//! This module is pure: the daemon passes the records and current values.

use std::collections::BTreeMap;

use crate::calibrate::{Record, Verdict};

/// Settings tuned together.
pub const GROUP: &[&str] = &["cpu.turbo", "cpu.max_perf_pct"];
/// The power mode's settings: recorded with every result, never changed here.
/// `mode` is the mode itself, recorded on laptops where the mode sets turbo
/// and the speed limit directly (no speed preference or firmware mode), so
/// what is learned stays per mode there too.
pub const CONTEXT: &[&str] = &["cpu.epp", "platform.profile", "mode"];
/// Kept steps allowed per power-mode context: a backstop, not a target.
pub const MAX_KEEPS: usize = 12;

/// Setting → value.
pub type State = BTreeMap<String, String>;

pub fn in_group(id: &str) -> bool {
    GROUP.contains(&id)
}

/// Values to try, in order.
pub fn grid(id: &str) -> &'static [&'static str] {
    match id {
        "cpu.turbo" => &["on", "off"],
        "cpu.max_perf_pct" => &["100", "90", "80", "70", "60", "50"],
        _ => &[],
    }
}

/// One step from `current`: the other turbo value, or the nearest speed
/// limits above and below. Lower first, since lower is what saves power.
pub fn neighbours(id: &str, current: &str) -> Vec<String> {
    let g = grid(id);
    match current.parse::<f64>() {
        Ok(c) if g.iter().all(|v| v.parse::<f64>().is_ok()) => {
            let nums: Vec<f64> = g.iter().filter_map(|v| v.parse().ok()).collect();
            let below = nums
                .iter()
                .filter(|v| **v < c)
                .fold(None, |m: Option<f64>, v| Some(m.map_or(*v, |m| m.max(*v))));
            let above = nums
                .iter()
                .filter(|v| **v > c)
                .fold(None, |m: Option<f64>, v| Some(m.map_or(*v, |m| m.min(*v))));
            [below, above]
                .into_iter()
                .flatten()
                .map(|v| format!("{v:.0}"))
                .collect()
        }
        _ => g
            .iter()
            .filter(|v| **v != current)
            .map(|v| v.to_string())
            .collect(),
    }
}

/// The power-mode part of a state.
pub fn context_of(state: &State) -> State {
    state
        .iter()
        .filter(|(k, _)| CONTEXT.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn group_part(state: &State) -> State {
    state
        .iter()
        .filter(|(k, _)| in_group(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// What to store in a record when testing `id` from `current`: every other
/// tuned and context value.
pub fn record_group(current: &State, id: &str) -> State {
    current
        .iter()
        .filter(|(k, _)| k.as_str() != id && (in_group(k) || CONTEXT.contains(&k.as_str())))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn tuned(r: &Record) -> bool {
    in_group(&r.setting) && !r.group.is_empty()
}

/// The state a record started from, and the one it would move to.
fn before(r: &Record) -> State {
    let mut s = r.group.clone();
    s.insert(r.setting.clone(), r.baseline.clone());
    s
}

fn after(r: &Record) -> State {
    let mut s = r.group.clone();
    s.insert(r.setting.clone(), r.value.clone());
    s
}

/// The best values found so far for this power-mode context (tuned settings
/// only), or `None` if nothing was kept there.
pub fn learned(records: &[Record], context: &State) -> Option<State> {
    records
        .iter()
        .rev()
        .filter(|r| tuned(r) && r.verdict == Verdict::Keep && context_of(&r.group) == *context)
        .map(|r| group_part(&after(r)))
        .next()
}

/// The values before any tuning: where the first tuned test started.
pub fn original(records: &[Record]) -> Option<State> {
    records
        .iter()
        .find(|r| tuned(r))
        .map(|r| group_part(&before(r)))
}

/// Kept steps in this context, each with the setting it moved. Used to say
/// what each learned value saves.
pub fn kept_steps<'a>(records: &'a [Record], context: &State) -> Vec<&'a Record> {
    records
        .iter()
        .filter(|r| tuned(r) && r.verdict == Verdict::Keep && context_of(&r.group) == *context)
        .collect()
}

/// Lag is a hard limit for the whole set, not only for each step: small
/// steps that each add a little lag must not add up to a slow laptop. This
/// is the stall the kept steps for this power mode added so far (steps that
/// seemed to reduce it count as zero, so noise never earns credit), plus
/// `step_stall` for the step being judged.
pub fn total_stall(records: &[Record], context: &State, step_stall: f64) -> f64 {
    kept_steps(records, context)
        .iter()
        .map(|r| r.stall.mean.max(0.0))
        .sum::<f64>()
        + step_stall.max(0.0)
}

/// The next step to test from `current` (tuned and context values), or
/// `None` when no single step is left to try. `ok` says whether a value is
/// allowed on this machine (probe limits, quirks).
pub fn next(
    records: &[Record],
    current: &State,
    max_tries: usize,
    ok: impl Fn(&str, &str) -> bool,
) -> Option<(String, String)> {
    remaining(records, current, max_tries, ok)
        .into_iter()
        .next()
}

/// Every step still to test from `current`, in the order [`next`] takes
/// them. Used for progress: when one is kept, the steps from the new values
/// take their place, so this can grow again by a step or two.
pub fn remaining(
    records: &[Record],
    current: &State,
    max_tries: usize,
    ok: impl Fn(&str, &str) -> bool,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let context = context_of(current);
    if kept_steps(records, &context).len() >= MAX_KEEPS {
        return out;
    }
    for id in GROUP {
        let Some(now) = current.get(*id) else {
            continue;
        };
        let from = record_group(current, id);
        for v in neighbours(id, now) {
            if !ok(id, &v) {
                continue;
            }
            let tries: Vec<&Record> = records
                .iter()
                .filter(|r| {
                    r.setting == *id && r.value == v && r.baseline == *now && r.group == from
                })
                .collect();
            let settled = tries.iter().any(|r| r.verdict != Verdict::Inconclusive);
            if !settled && tries.len() < max_tries {
                out.push((id.to_string(), v));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto::Workload;
    use crate::calibrate::{Effect, PowerSource, RecordContext};

    fn state(pairs: &[(&str, &str)]) -> State {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn rec(current: &State, id: &str, v: &str, verdict: Verdict, saves: f64) -> Record {
        let e = Effect {
            mean: -saves,
            ci_low: -saves,
            ci_high: -saves,
            confidence: 1.0,
        };
        Record {
            setting: id.into(),
            value: v.into(),
            baseline: current[id].clone(),
            context: RecordContext {
                power_source: PowerSource::Battery,
                workload: Workload::LightInteractive,
            },
            metric: "system_watts".into(),
            effect: e,
            stall: e,
            baseline_w: 6.0,
            samples: 4,
            verdict,
            reason: String::new(),
            time: 0,
            group: record_group(current, id),
        }
    }

    #[test]
    fn steps_one_notch_at_a_time() {
        assert_eq!(neighbours("cpu.max_perf_pct", "100"), ["90"]);
        assert_eq!(neighbours("cpu.max_perf_pct", "80"), ["70", "90"]);
        assert_eq!(neighbours("cpu.max_perf_pct", "85"), ["80", "90"]);
        assert_eq!(neighbours("cpu.max_perf_pct", "50"), ["60"]);
        assert_eq!(neighbours("cpu.turbo", "on"), ["off"]);
    }

    /// A laptop where turbo off on its own adds lag (it is rejected), but
    /// once the speed limit is at 80% or below, turbo barely matters for
    /// speed and switching it off saves power. Testing each setting once
    /// would never find that; descent with re-testing does.
    fn laptop(s: &State) -> (f64, f64) {
        let perf: f64 = s["cpu.max_perf_pct"].parse().unwrap();
        let turbo = s["cpu.turbo"] == "on";
        let mut w = 4.0 + (perf - 50.0) * 0.02;
        let mut stall = if perf < 70.0 {
            (70.0 - perf) * 0.1
        } else {
            0.0
        };
        if turbo {
            w += 0.4;
        } else if perf > 80.0 {
            stall += 1.5;
        }
        if s["cpu.epp"] == "power" {
            w -= 0.5;
        }
        (w, stall)
    }

    fn run(records: &mut Vec<Record>, start: &State) -> State {
        let mut cur = start.clone();
        for _ in 0..50 {
            let Some((id, v)) = next(records, &cur, 3, |_, _| true) else {
                return cur;
            };
            let mut cand = cur.clone();
            cand.insert(id.clone(), v.clone());
            let ((w0, s0), (w1, s1)) = (laptop(&cur), laptop(&cand));
            let keep = w0 - w1 >= 0.05 && s1 - s0 <= 0.5;
            let verdict = if keep { Verdict::Keep } else { Verdict::Reject };
            records.push(rec(&cur, &id, &v, verdict, w0 - w1));
            if keep {
                cur = cand;
            }
        }
        panic!("did not settle");
    }

    #[test]
    fn finds_combinations_single_tests_miss() {
        let start = state(&[
            ("cpu.turbo", "on"),
            ("cpu.max_perf_pct", "100"),
            ("cpu.epp", "balance_performance"),
        ]);
        let mut records = Vec::new();
        let end = run(&mut records, &start);
        assert_eq!(end["cpu.turbo"], "off", "{end:?}");
        assert_eq!(end["cpu.max_perf_pct"], "70", "{end:?}");
        // The very first turbo test was rejected (lag), and later re-tested.
        let turbo: Vec<&Record> = records
            .iter()
            .filter(|r| r.setting == "cpu.turbo")
            .collect();
        assert_eq!(turbo[0].verdict, Verdict::Reject);
        assert!(turbo.iter().any(|r| r.verdict == Verdict::Keep));

        let ctx = context_of(&start);
        let best = learned(&records, &ctx).unwrap();
        assert_eq!(best["cpu.max_perf_pct"], "70");
        assert_eq!(original(&records).unwrap()["cpu.turbo"], "on");
        assert!(next(&records, &end, 3, |_, _| true).is_none());
    }

    #[test]
    fn results_belong_to_their_power_mode() {
        let start = state(&[
            ("cpu.turbo", "on"),
            ("cpu.max_perf_pct", "100"),
            ("cpu.epp", "balance_performance"),
        ]);
        let mut records = Vec::new();
        run(&mut records, &start);
        let other = state(&[
            ("cpu.turbo", "on"),
            ("cpu.max_perf_pct", "100"),
            ("cpu.epp", "power"),
        ]);
        assert!(learned(&records, &context_of(&other)).is_none());
        assert!(
            next(&records, &other, 3, |_, _| true).is_some(),
            "a new power mode is tuned on its own"
        );
    }

    #[test]
    fn lag_adds_up_across_steps() {
        let cur = state(&[
            ("cpu.turbo", "on"),
            ("cpu.max_perf_pct", "100"),
            ("cpu.epp", "power"),
        ]);
        let mut r = rec(&cur, "cpu.max_perf_pct", "90", Verdict::Keep, 0.2);
        r.stall.mean = 0.4;
        let mut noisy = r.clone();
        noisy.stall.mean = -0.3;
        let ctx = context_of(&cur);
        assert!((total_stall(&[r.clone()], &ctx, 0.4) - 0.8).abs() < 1e-9);
        assert!((total_stall(&[noisy], &ctx, 0.4) - 0.4).abs() < 1e-9);
        assert!((total_stall(&[r], &state(&[]), 0.4) - 0.4).abs() < 1e-9);
    }

    #[test]
    fn respects_limits_and_tries() {
        let cur = state(&[("cpu.turbo", "on"), ("cpu.max_perf_pct", "100")]);
        assert_eq!(
            next(&[], &cur, 3, |id, _| id != "cpu.turbo"),
            Some(("cpu.max_perf_pct".into(), "90".into()))
        );
        let noisy: Vec<Record> = (0..3)
            .map(|_| rec(&cur, "cpu.turbo", "off", Verdict::Inconclusive, 0.0))
            .collect();
        assert_eq!(
            next(&noisy, &cur, 3, |_, _| true),
            Some(("cpu.max_perf_pct".into(), "90".into())),
            "three noisy tries are enough"
        );
        assert!(next(&noisy, &cur, 3, |id, _| id == "cpu.turbo").is_none());
    }
}
