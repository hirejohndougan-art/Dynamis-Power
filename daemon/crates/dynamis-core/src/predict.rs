//! Prediction (phase 7): battery forecasts from habits, "Last until", and
//! charging that follows when the laptop is usually unplugged.
//!
//! Everything here is pure: the daemon passes the time and the readings.
//!
//! **Time.** The service has no time zone database, so habits are kept by
//! hour of the week in UTC. A routine at 8 am local time is a routine at the
//! same UTC hour (daylight saving moves it by an hour twice a year, which the
//! tolerances absorb). The app turns times into local clock times.
//!
//! **Habits.** Two things are learned, slowly and on this computer only:
//! typical power on battery for each hour of the week, and when the charger
//! is unplugged. Nothing about what the laptop was used for.

use serde::{Deserialize, Serialize};

use crate::profiles::Profile;

const HABITS_VERSION: u32 = 1;
const WEEK_HOURS: usize = 168;
/// Unplug times kept: about two months of daily use.
const KEEP_UNPLUGS: usize = 60;
/// Readings needed before an hour's typical power is trusted.
const MIN_HOUR_SAMPLES: f64 = 6.0;
/// Weight of a new reading in an hour's average (slow, so one odd day does
/// not rewrite the habit).
const HOUR_ALPHA: f64 = 0.05;

/// Seconds since the epoch, as the daemon passes it.
pub type Unix = f64;

/// Hour of the week, 0 = Monday 00:00 UTC.
pub fn hour_of_week(t: Unix) -> usize {
    let hours = (t / 3600.0).floor() as i64;
    // 1970-01-01 was a Thursday: three days after a Monday.
    (hours + 72).rem_euclid(WEEK_HOURS as i64) as usize
}

/// Day of the week, 0 = Monday (UTC).
pub fn weekday(t: Unix) -> usize {
    hour_of_week(t) / 24
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
struct Bucket {
    w: f64,
    n: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Habits {
    v: u32,
    hours: Vec<Bucket>,
    /// When the charger was unplugged, newest last.
    unplugs: Vec<u64>,
    #[serde(skip)]
    last_on_ac: Option<bool>,
}

impl Default for Habits {
    fn default() -> Self {
        Habits {
            v: HABITS_VERSION,
            hours: vec![Bucket::default(); WEEK_HOURS],
            unplugs: Vec::new(),
            last_on_ac: None,
        }
    }
}

/// What a reading changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Unplugged,
    PluggedIn,
}

impl Habits {
    /// Add one reading. Power is counted only on battery.
    pub fn add(&mut self, now: Unix, on_ac: bool, power_w: Option<f64>) -> Option<Event> {
        if self.hours.len() != WEEK_HOURS {
            self.hours = vec![Bucket::default(); WEEK_HOURS];
        }
        let event = match self.last_on_ac {
            Some(true) if !on_ac => Some(Event::Unplugged),
            Some(false) if on_ac => Some(Event::PluggedIn),
            _ => None,
        };
        self.last_on_ac = Some(on_ac);
        if event == Some(Event::Unplugged) {
            self.unplugs.push(now as u64);
            if self.unplugs.len() > KEEP_UNPLUGS {
                self.unplugs.remove(0);
            }
        }
        if let (false, Some(w)) = (on_ac, power_w.filter(|w| *w > 0.1 && *w < 200.0)) {
            let b = &mut self.hours[hour_of_week(now)];
            // Plain mean for the first readings, then a slow moving average.
            let a = (1.0 / (b.n + 1.0)).max(HOUR_ALPHA);
            b.w += (w - b.w) * a;
            b.n += 1.0;
        }
        event
    }

    /// Typical power on battery at this hour of the week, or the average of
    /// every known hour, or `None` before anything is known.
    pub fn typical_w(&self, t: Unix) -> Option<f64> {
        let b = self.hours.get(hour_of_week(t))?;
        if b.n >= MIN_HOUR_SAMPLES {
            return Some(b.w);
        }
        self.overall_w()
    }

    fn overall_w(&self) -> Option<f64> {
        let known: Vec<&Bucket> = self
            .hours
            .iter()
            .filter(|b| b.n >= MIN_HOUR_SAMPLES)
            .collect();
        let n: f64 = known.iter().map(|b| b.n).sum();
        (n > 0.0).then(|| known.iter().map(|b| b.w * b.n).sum::<f64>() / n)
    }

    /// When the battery will run out: hour by hour, using typical power for
    /// each hour ahead, blended with the power right now for the next hour
    /// (what is happening now is the best guess for the near future).
    /// `None` without a usable estimate.
    pub fn forecast_empty(&self, now: Unix, energy_wh: f64, now_w: Option<f64>) -> Option<Unix> {
        let mut left = energy_wh;
        let mut t = now;
        for step in 0..(7 * 24) {
            let typical = self.typical_w(t);
            let w = match (step, now_w, typical) {
                (0, Some(n), Some(ty)) => 0.7 * n + 0.3 * ty,
                (0, Some(n), None) => n,
                (_, _, Some(ty)) => ty,
                (_, Some(n), None) => n,
                _ => return None,
            };
            if w <= 0.05 {
                return None;
            }
            let need = w; // watt-hours for one hour
            if left <= need {
                return Some(t + left / w * 3600.0);
            }
            left -= need;
            t += 3600.0;
        }
        None
    }

    /// The next time, after `after`, the laptop is usually unplugged: for
    /// today and the next six days, the unplug times on that weekday in the
    /// last six weeks, if there are at least three and they agree within
    /// about 45 minutes (median distance from the median).
    pub fn usual_unplug(&self, after: Unix) -> Option<Unix> {
        let day_start = |t: Unix| (t / 86400.0).floor() * 86400.0;
        for d in 0..7 {
            let day = day_start(after) + d as f64 * 86400.0;
            let wd = weekday(day);
            let mut mins: Vec<f64> = self
                .unplugs
                .iter()
                .map(|u| *u as f64)
                .filter(|u| *u > after - 42.0 * 86400.0 && weekday(*u) == wd)
                .map(|u| (u - day_start(u)) / 60.0)
                .collect();
            if mins.len() < 3 {
                continue;
            }
            let median = median(&mut mins);
            let mut dev: Vec<f64> = mins.iter().map(|m| (m - median).abs()).collect();
            if median_of(&mut dev) > 45.0 {
                continue;
            }
            let t = day + median * 60.0;
            if t > after {
                return Some(t);
            }
        }
        None
    }

    pub fn unplugs_seen(&self) -> usize {
        self.unplugs.len()
    }
}

fn median(v: &mut [f64]) -> f64 {
    median_of(v)
}

fn median_of(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

// --- Last until -------------------------------------------------------------

/// How hard "Last until" is saving. Each level includes the ones before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// On track: nothing changes.
    Normal,
    /// No full speed.
    NoBoost,
    /// Longer battery mode.
    Save,
    /// Longer battery mode, and apps in the background slowed.
    SaveMore,
}

impl Level {
    /// The fastest power mode allowed at this level.
    pub fn cap(self) -> Option<Profile> {
        match self {
            Level::Normal => None,
            Level::NoBoost => Some(Profile::Balanced),
            Level::Save | Level::SaveMore => Some(Profile::PowerSaver),
        }
    }

    fn up(self) -> Level {
        match self {
            Level::Normal => Level::NoBoost,
            Level::NoBoost => Level::Save,
            _ => Level::SaveMore,
        }
    }

    fn down(self) -> Level {
        match self {
            Level::SaveMore => Level::Save,
            Level::Save => Level::NoBoost,
            _ => Level::Normal,
        }
    }
}

/// What "Last until" decided.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TargetStatus {
    pub until: Unix,
    pub level: Level,
    /// Watts the battery can afford on average until the target.
    pub budget_w: Option<f64>,
    /// Power use lately (a slow average).
    pub recent_w: Option<f64>,
    /// When the battery would run out at the recent rate.
    pub expected_end: Option<Unix>,
    pub on_track: bool,
    pub reason: String,
}

/// Energy kept in reserve: the target is met with this much to spare.
const RESERVE_FRAC: f64 = 0.05;
/// Stay at least this long at a level before moving again.
const LEVEL_DWELL_S: f64 = 300.0;

/// Steers towards a time target by stepping through [`Level`]s. Feedback,
/// not a model: if recent power use is above what the battery can afford
/// until the target, save harder; if well below, ease off. Moves one level
/// at a time and waits between moves, so the power mode does not flap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub until: Unix,
    #[serde(default = "normal")]
    pub level: Level,
    #[serde(skip)]
    recent_w: Option<f64>,
    #[serde(skip)]
    last_move: Option<Unix>,
}

fn normal() -> Level {
    Level::Normal
}

impl Target {
    pub fn new(until: Unix) -> Target {
        Target {
            until,
            level: Level::Normal,
            recent_w: None,
            last_move: None,
        }
    }

    /// One reading. `energy_wh` and `power_w` are from the battery;
    /// `full_wh` sizes the reserve.
    pub fn step(
        &mut self,
        now: Unix,
        energy_wh: Option<f64>,
        power_w: Option<f64>,
        full_wh: Option<f64>,
    ) -> TargetStatus {
        if let Some(w) = power_w.filter(|w| *w > 0.0) {
            // About a ten-minute average at a reading every 15 s.
            self.recent_w = Some(self.recent_w.map_or(w, |r| r + (w - r) * 0.025));
        }
        let hours_left = (self.until - now) / 3600.0;
        let reserve = full_wh.unwrap_or(0.0) * RESERVE_FRAC;
        let budget_w = energy_wh
            .filter(|_| hours_left > 0.0)
            .map(|e| ((e - reserve).max(0.0) / hours_left).max(0.0));
        let expected_end = energy_wh
            .zip(self.recent_w)
            .filter(|(_, w)| *w > 0.05)
            .map(|(e, w)| now + e / w * 3600.0);

        let can_move = self.last_move.is_none_or(|t| now - t >= LEVEL_DWELL_S);
        if let (Some(b), Some(r)) = (budget_w, self.recent_w) {
            if can_move && r > b && self.level != Level::SaveMore {
                self.level = self.level.up();
                self.last_move = Some(now);
            } else if can_move && r < b * 0.75 && self.level != Level::Normal {
                self.level = self.level.down();
                self.last_move = Some(now);
            }
        }
        let on_track = match (budget_w, self.recent_w) {
            (Some(b), Some(r)) => r <= b,
            _ => true,
        };
        let reason = match (on_track, self.level) {
            (true, Level::Normal) => "On track without changing anything".to_string(),
            (true, _) => "On track by saving power".to_string(),
            (false, Level::SaveMore) => {
                "Saving everything it can, but the battery may run out first".to_string()
            }
            (false, _) => "Saving more to reach your time".to_string(),
        };
        TargetStatus {
            until: self.until,
            level: self.level,
            budget_w: budget_w.map(|b| (b * 100.0).round() / 100.0),
            recent_w: self.recent_w.map(|r| (r * 100.0).round() / 100.0),
            expected_end,
            on_track,
            reason,
        }
    }
}

// --- Charging that follows habits ---------------------------------------------

/// Hold the battery here overnight.
pub const HOLD_PCT: u8 = 80;
/// Start charging fully this long before the usual unplug.
pub const TOP_UP_S: f64 = 2.5 * 3600.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChargePlan {
    pub limit_pct: u8,
    /// When it next changes (the app shows it as a local time).
    pub until: Option<Unix>,
    pub reason: String,
}

/// The charge limit for now. Holds at [`HOLD_PCT`] while plugged in, and
/// lets it charge fully [`TOP_UP_S`] before the laptop is usually unplugged.
/// Without a regular pattern it charges fully, as a laptop normally does.
pub fn charge_plan(habits: &Habits, now: Unix) -> ChargePlan {
    match habits.usual_unplug(now) {
        Some(t) if t - now <= TOP_UP_S => ChargePlan {
            limit_pct: 100,
            until: Some(t),
            reason: "Charging fully: you usually unplug soon".into(),
        },
        Some(t) => ChargePlan {
            limit_pct: HOLD_PCT,
            until: Some(t - TOP_UP_S),
            reason: format!(
                "Holding at {HOLD_PCT}% to spare the battery; it charges fully before you usually unplug"
            ),
        },
        None => ChargePlan {
            limit_pct: 100,
            until: None,
            reason: "Charging fully: no regular unplug time yet".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 2026-11-16 00:00 UTC.
    const MONDAY: Unix = 1_794_787_200.0;

    #[test]
    fn weeks_start_on_monday() {
        assert_eq!(hour_of_week(MONDAY), 0);
        assert_eq!(weekday(MONDAY + 86400.0 * 2.0 + 3600.0), 2);
        assert_eq!(hour_of_week(MONDAY - 3600.0), 167);
    }

    #[test]
    fn learns_when_the_charger_comes_out() {
        let mut h = Habits::default();
        // Four weeks of weekday unplugs around 8:00 (±10 min), then a
        // Saturday at 11:00 once.
        for week in 0..4 {
            for d in 0..5 {
                let day = MONDAY + (week * 7 + d) as f64 * 86400.0;
                let jitter = ((week * 5 + d) % 5) as f64 * 5.0 - 10.0;
                h.add(day + 6.0 * 3600.0, true, None);
                assert_eq!(
                    h.add(day + 8.0 * 3600.0 + jitter * 60.0, false, Some(5.0)),
                    Some(Event::Unplugged)
                );
                h.add(day + 18.0 * 3600.0, true, None);
            }
        }
        let now = MONDAY + 28.0 * 86400.0 + 2.0 * 3600.0; // Monday 02:00
        let next = h.usual_unplug(now).unwrap();
        assert!(
            (next - (now + 6.0 * 3600.0)).abs() <= 600.0,
            "{}",
            next - now
        );
        // Friday evening: the next usual unplug is Monday morning.
        let fri = MONDAY + 32.0 * 86400.0 + 20.0 * 3600.0;
        let next = h.usual_unplug(fri).unwrap();
        assert_eq!(weekday(next), 0);

        // Charging: hold overnight, charge fully before.
        let night = MONDAY + 28.0 * 86400.0 + 1.0 * 3600.0;
        assert_eq!(charge_plan(&h, night).limit_pct, HOLD_PCT);
        let early = MONDAY + 28.0 * 86400.0 + 6.0 * 3600.0;
        assert_eq!(charge_plan(&h, early).limit_pct, 100);
        assert_eq!(charge_plan(&Habits::default(), night).limit_pct, 100);
    }

    #[test]
    fn irregular_unplugs_are_not_a_habit() {
        let mut h = Habits::default();
        for (week, hour) in [(0, 7.0), (1, 13.0), (2, 9.5), (3, 18.0)] {
            let day = MONDAY + (week * 7) as f64 * 86400.0;
            h.add(day, true, None);
            h.add(day + hour * 3600.0, false, None);
        }
        assert!(h.usual_unplug(MONDAY + 28.0 * 86400.0).is_none());
    }

    #[test]
    fn forecasts_from_typical_power() {
        let mut h = Habits::default();
        assert!(h.forecast_empty(MONDAY, 40.0, None).is_none());
        // Every hour of the week: 5 W, except 4 W between 12:00 and 18:00.
        for week in 0..2 {
            for hw in 0..168 {
                let t = MONDAY + (week * 168 + hw) as f64 * 3600.0;
                let w = if (12..18).contains(&(hw % 24)) {
                    4.0
                } else {
                    5.0
                };
                for k in 0..4 {
                    h.add(t + k as f64 * 600.0, false, Some(w));
                }
            }
        }
        let now = MONDAY + 14.0 * 86400.0 + 10.0 * 3600.0; // 10:00
                                                           // 10–12 at 5 W (10 Wh), 12–18 at 4 W (24 Wh), then 5 W: 40 Wh lasts
                                                           // until 19:12.
        let end = h.forecast_empty(now, 40.0, Some(5.0)).unwrap();
        assert!(
            ((end - now) / 3600.0 - 9.2).abs() < 0.05,
            "{}",
            (end - now) / 3600.0
        );
    }

    #[test]
    fn last_until_saves_only_as_hard_as_needed() {
        // 30 Wh left; must last 5 h: 5.7 W budget with a 2 Wh reserve.
        let now = MONDAY;
        let mut t = Target::new(now + 5.0 * 3600.0);
        let full = Some(40.0);
        // Using 8 W: steps up one level per dwell, up to the most.
        let mut levels = Vec::new();
        for i in 0..200 {
            let s = t.step(now + i as f64 * 15.0, Some(30.0), Some(8.0), full);
            levels.push(s.level);
        }
        assert_eq!(*levels.last().unwrap(), Level::SaveMore);
        assert_eq!(
            levels[0],
            Level::NoBoost,
            "the first reading may act at once"
        );
        assert_eq!(levels.windows(2).filter(|w| w[0] != w[1]).count(), 2);
        // ...and then waits five minutes between moves.
        assert_eq!(levels[19], Level::NoBoost);
        assert_eq!(levels[20], Level::Save);
        // Use drops to 3 W: eases off, one level at a time.
        let mut last = t.level;
        for i in 0..400 {
            let s = t.step(now + 3000.0 + i as f64 * 15.0, Some(30.0), Some(3.0), full);
            assert!(s.level <= last);
            last = s.level;
        }
        assert_eq!(last, Level::Normal);
        let s = t.step(now + 9000.0, Some(30.0), Some(3.0), full);
        assert!(s.on_track);
        assert_eq!(Level::Save.cap(), Some(Profile::PowerSaver));
    }
}
