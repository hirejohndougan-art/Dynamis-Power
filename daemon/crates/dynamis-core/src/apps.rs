//! Per-app power accounting (phase 6).
//!
//! Desktops start every app in its own systemd "scope" (a cgroup), named
//! after the app: `app-org.kde.dolphin@3f2c.service`,
//! `app-flatpak-org.mozilla.firefox-1234.scope`, `app-gnome-firefox-5678.scope`.
//! That name is how processes are grouped into apps here. Processes outside
//! any app scope count as "System".
//!
//! Each reading adds up CPU time per app since the last reading. On battery,
//! the power above the laptop's idle level is shared out by CPU time to give
//! an **estimate** of each app's watts. These are estimates, and the app says
//! so: CPU time is the biggest part of what an app costs, but not all of it
//! (the screen, Wi-Fi and the GPU are not counted per app).
//!
//! Totals are kept per day for a week, and per month for three months (for
//! "uses more power than last month"), on this computer only.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::fs::SysRoot;

const FORMAT_VERSION: u32 = 1;
const KEEP_DAYS: usize = 8;
const KEEP_MONTHS: usize = 3;
/// Readings further apart than this are a gap, not counted.
const MAX_GAP_S: f64 = 900.0;
/// Hours on battery an app must have run in each month to compare them.
const TREND_MIN_S: f64 = 2.0 * 3600.0;
/// The change that counts as a trend.
const TREND_PCT: f64 = 25.0;
pub const SYSTEM: &str = "System";

/// One process's identity and CPU time.
#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub app: String,
    /// The cgroup directory (under /sys/fs/cgroup) the process is in.
    pub cgroup: String,
    /// utime + stime, in clock ticks.
    pub cpu_ticks: u64,
    /// Times any of its threads has been put on a processor (the third
    /// number of each thread's `schedstat`): for threads that mostly sleep,
    /// how often they woke. Only read for apps, not System processes, to
    /// keep a reading cheap. `None` where the kernel does not keep it.
    pub runs: Option<u64>,
}

/// Total times the process's threads were put on a processor.
fn thread_runs(root: &SysRoot, pid: u32) -> Option<u64> {
    let mut total = 0u64;
    let mut any = false;
    for tid in root.list_dir(&format!("/proc/{pid}/task")) {
        let Ok(s) = root.read_string(&format!("/proc/{pid}/task/{tid}/schedstat")) else {
            continue;
        };
        if let Some(n) = s
            .split_whitespace()
            .nth(2)
            .and_then(|n| n.parse::<u64>().ok())
        {
            total += n;
            any = true;
        }
    }
    any.then_some(total)
}

/// App name from a cgroup path, or `None` when it is not an app scope.
pub fn app_from_cgroup(cgroup: &str) -> Option<String> {
    let last = cgroup.rsplit('/').next()?;
    let name = last.strip_prefix("app-")?;
    let name = name
        .strip_suffix(".scope")
        .or_else(|| name.strip_suffix(".service"))?;
    // "org.kde.dolphin@3f2c" → "org.kde.dolphin"
    let name = name.split('@').next()?;
    // Launcher prefixes some desktops add.
    let name = ["flatpak-", "gnome-", "kde-", "dbus-:1.2-"]
        .iter()
        .fold(name, |n, p| n.strip_prefix(p).unwrap_or(n));
    // A trailing "-1234" or "-a1b2c3" is a random instance number.
    let name = match name.rsplit_once('-') {
        Some((head, tail))
            if !head.is_empty()
                && tail.len() >= 2
                && tail.chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            head
        }
        _ => name,
    };
    // "org.kde.dolphin" stays as is; "\x2d" escapes from systemd unit names.
    let name = name.replace("\\x2d", "-");
    (!name.is_empty()).then_some(name)
}

/// Dynamis Power's own app, panel icon and helpers: never listed in Apps,
/// never counted and never given a different share (it would only be
/// measuring itself). The service runs outside user.slice anyway.
pub fn is_own(app: &str) -> bool {
    let a = app.to_ascii_lowercase();
    a.starts_with("dynamis") || a.starts_with("net.dynamis")
}

/// Every process with its app and CPU time. Kernel threads and Dynamis
/// Power's own processes are skipped.
pub fn scan(root: &SysRoot) -> Vec<Proc> {
    let mut out = Vec::new();
    for entry in root.list_dir("/proc") {
        let Ok(pid) = entry.parse::<u32>() else {
            continue;
        };
        let Ok(stat) = root.read_string(&format!("/proc/{pid}/stat")) else {
            continue;
        };
        // Fields after the command name (which may contain spaces): field 3
        // is index 0, so utime (14) and stime (15) are 11 and 12, ppid (4)
        // is 1.
        let Some(rest) = stat.rfind(')').map(|i| &stat[i + 1..]) else {
            continue;
        };
        let f: Vec<&str> = rest.split_whitespace().collect();
        let (Some(ppid), Some(u), Some(s)) = (f.get(1), f.get(11), f.get(12)) else {
            continue;
        };
        if *ppid == "2" || pid == 2 {
            continue; // kernel threads
        }
        let ticks = u.parse::<u64>().unwrap_or(0) + s.parse::<u64>().unwrap_or(0);
        let cgroup = root
            .read_string(&format!("/proc/{pid}/cgroup"))
            .ok()
            .and_then(|c| {
                c.lines()
                    .find_map(|l| l.strip_prefix("0::").map(String::from))
            })
            .unwrap_or_default();
        let app = app_from_cgroup(&cgroup).unwrap_or_else(|| SYSTEM.to_string());
        if is_own(&app) {
            continue;
        }
        let runs = if app == SYSTEM {
            None
        } else {
            thread_runs(root, pid)
        };
        out.push(Proc {
            pid,
            app,
            cgroup,
            cpu_ticks: ticks,
            runs,
        });
    }
    out
}

/// What one app used in a period.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub cpu_s: f64,
    /// Estimated energy on battery, watt-hours.
    pub wh: f64,
    /// Seconds the app was running while on battery (for average watts).
    #[serde(default)]
    pub bat_s: f64,
}

/// An app that costs more (or less) than last month while running on
/// battery.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Trend {
    pub app: String,
    pub last_w: f64,
    pub this_w: f64,
    pub change_pct: f64,
}

/// One reading's result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Now {
    pub app: String,
    pub cpu_pct: f64,
    /// Estimated watts (on battery only).
    pub watts: Option<f64>,
    /// How often the app's threads woke, per second (None when unknown).
    pub wakeups_per_s: Option<f64>,
}

/// Wake-ups per second above which an app is said to wake the processor a
/// lot: each wake-up stops the processor from resting in its deeper idle
/// states, and a quiet desktop wakes it a few hundred times a second in all.
pub const MANY_WAKEUPS_PER_S: f64 = 100.0;

#[derive(Debug, Serialize, Deserialize)]
pub struct Meter {
    v: u32,
    /// Day ("2026-11-16", UTC) → app → usage.
    days: BTreeMap<String, BTreeMap<String, Usage>>,
    /// Month ("2026-11") → app → usage. Added later: older files have none.
    #[serde(default)]
    months: BTreeMap<String, BTreeMap<String, Usage>>,
    #[serde(skip)]
    prev: HashMap<u32, u64>,
    #[serde(skip)]
    prev_runs: HashMap<u32, u64>,
    #[serde(skip)]
    prev_time: Option<f64>,
    /// Lowest whole-system power seen on battery: the idle level that is
    /// not shared out among apps.
    #[serde(default)]
    base_w: Option<f64>,
    #[serde(skip)]
    pub last: Vec<Now>,
    /// App → cgroup directories, from the last reading (for slowing down).
    #[serde(skip)]
    pub cgroups: BTreeMap<String, Vec<String>>,
}

/// USER_HZ on Linux is 100 on every architecture we run on.
const TICKS_PER_S: f64 = 100.0;

impl Default for Meter {
    fn default() -> Self {
        Meter {
            v: FORMAT_VERSION,
            days: BTreeMap::new(),
            months: BTreeMap::new(),
            prev: HashMap::new(),
            prev_runs: HashMap::new(),
            prev_time: None,
            base_w: None,
            last: Vec::new(),
            cgroups: BTreeMap::new(),
        }
    }
}

impl Meter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one reading. `now` is Unix time; `power_w` is whole-system power
    /// on battery (None on mains); `cores` scales CPU %.
    pub fn add(&mut self, procs: &[Proc], now: f64, power_w: Option<f64>, cores: usize) {
        if let Some(w) = power_w {
            self.base_w = Some(self.base_w.map_or(w, |b| b.min(w)));
        }
        // A long gap (sleep, a pause in readings) is not counted: the CPU
        // time in it is unknown, and the power reading is from now.
        let dt = self
            .prev_time
            .map(|t| now - t)
            .filter(|d| *d > 0.0 && *d <= MAX_GAP_S);
        let mut per_app: BTreeMap<String, u64> = BTreeMap::new();
        let mut cgroups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut next = HashMap::with_capacity(procs.len());
        let mut next_runs = HashMap::new();
        let mut runs_per_app: BTreeMap<String, u64> = BTreeMap::new();
        for p in procs {
            next.insert(p.pid, p.cpu_ticks);
            if let Some(r) = p.runs {
                next_runs.insert(p.pid, r);
                if let Some(before) = self.prev_runs.get(&p.pid) {
                    // Threads that ended take their counts with them, so a
                    // total can fall: count that as no wake-ups.
                    *runs_per_app.entry(p.app.clone()).or_default() += r.saturating_sub(*before);
                }
            }
            if p.app != SYSTEM && !p.cgroup.is_empty() {
                let list = cgroups.entry(p.app.clone()).or_default();
                if !list.contains(&p.cgroup) {
                    list.push(p.cgroup.clone());
                }
            }
            // A new process counts from its next reading.
            let Some(before) = self.prev.get(&p.pid) else {
                continue;
            };
            *per_app.entry(p.app.clone()).or_default() += p.cpu_ticks.saturating_sub(*before);
        }
        self.prev = next;
        self.prev_runs = next_runs;
        self.prev_time = Some(now);
        self.cgroups = cgroups;
        let Some(dt) = dt else {
            self.last.clear();
            return;
        };

        let total: u64 = per_app.values().sum();
        let spare_w = power_w.zip(self.base_w).map(|(w, b)| (w - b).max(0.0));
        let day = day_of(now);
        let month = self
            .months
            .entry(day.get(..7).unwrap_or_default().to_string())
            .or_default();
        // Time running on battery, for average watts per app.
        if spare_w.is_some() {
            for app in cgroups_running(&self.cgroups, &per_app) {
                month.entry(app).or_default().bat_s += dt;
            }
        }
        for (app, t) in per_app.iter().filter(|(_, t)| **t > 0) {
            let m = month.entry(app.clone()).or_default();
            m.cpu_s += *t as f64 / TICKS_PER_S;
            if let Some(s) = spare_w {
                if total > 0 {
                    m.wh += s * *t as f64 / total as f64 * dt / 3600.0;
                }
            }
        }
        while self.months.len() > KEEP_MONTHS {
            let oldest = self.months.keys().next().cloned();
            if let Some(k) = oldest {
                self.months.remove(&k);
            }
        }
        let bucket = self.days.entry(day).or_default();
        self.last = per_app
            .iter()
            .filter(|(_, t)| **t > 0)
            .map(|(app, t)| {
                let cpu_s = *t as f64 / TICKS_PER_S;
                let watts = spare_w.map(|s| {
                    if total > 0 {
                        s * *t as f64 / total as f64
                    } else {
                        0.0
                    }
                });
                let u = bucket.entry(app.clone()).or_default();
                u.cpu_s += cpu_s;
                u.wh += watts.unwrap_or(0.0) * dt / 3600.0;
                Now {
                    app: app.clone(),
                    cpu_pct: cpu_s / dt / cores.max(1) as f64 * 100.0,
                    watts,
                    wakeups_per_s: runs_per_app.get(app).map(|r| *r as f64 / dt),
                }
            })
            .collect();
        self.last.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
        while self.days.len() > KEEP_DAYS {
            let oldest = self.days.keys().next().cloned();
            if let Some(k) = oldest {
                self.days.remove(&k);
            }
        }
    }

    /// Totals for the last `days` days (1 = today), biggest energy first.
    pub fn totals(&self, days: usize, now: f64) -> Vec<(String, Usage)> {
        let from = day_of(now - (days.max(1) - 1) as f64 * 86400.0);
        let mut sum: BTreeMap<String, Usage> = BTreeMap::new();
        for (_, apps) in self.days.range(from..) {
            for (app, u) in apps {
                let s = sum.entry(app.clone()).or_default();
                s.cpu_s += u.cpu_s;
                s.wh += u.wh;
            }
        }
        let mut v: Vec<(String, Usage)> = sum.into_iter().collect();
        v.sort_by(|a, b| {
            b.1.wh
                .total_cmp(&a.1.wh)
                .then(b.1.cpu_s.total_cmp(&a.1.cpu_s))
        });
        v
    }

    /// Drop every per-day total (counting was switched off, or the person
    /// asked to forget). The idle level is kept: it describes the laptop.
    pub fn forget(&mut self) {
        self.days.clear();
        self.months.clear();
    }

    /// Apps whose average power while running on battery changed by a
    /// quarter or more since last month, biggest rise first. Both months
    /// need two hours of the app on battery.
    pub fn trends(&self, now: f64) -> Vec<Trend> {
        let this_key = day_of(now).get(..7).unwrap_or_default().to_string();
        let Some(this) = self.months.get(&this_key) else {
            return Vec::new();
        };
        let Some((_, last)) = self.months.range(..this_key.clone()).next_back() else {
            return Vec::new();
        };
        let avg = |u: &Usage| (u.bat_s >= TREND_MIN_S).then(|| u.wh * 3600.0 / u.bat_s);
        let mut out: Vec<Trend> = this
            .iter()
            .filter(|(app, _)| app.as_str() != SYSTEM)
            .filter_map(|(app, u)| {
                let this_w = avg(u)?;
                let last_w = avg(last.get(app)?)?;
                if last_w <= 0.05 {
                    return None;
                }
                let change_pct = (this_w / last_w - 1.0) * 100.0;
                (change_pct.abs() >= TREND_PCT).then(|| Trend {
                    app: app.clone(),
                    last_w,
                    this_w,
                    change_pct,
                })
            })
            .collect();
        out.sort_by(|a, b| b.change_pct.total_cmp(&a.change_pct));
        out
    }

    /// Apps with at least one process right now.
    pub fn running(&self) -> Vec<String> {
        self.cgroups.keys().cloned().collect()
    }
}

/// Apps with a process in this reading (seen in cgroups) or CPU time.
fn cgroups_running(
    cgroups: &BTreeMap<String, Vec<String>>,
    per_app: &BTreeMap<String, u64>,
) -> Vec<String> {
    let mut v: Vec<String> = cgroups.keys().cloned().collect();
    for (app, t) in per_app {
        if *t > 0 && !v.contains(app) {
            v.push(app.clone());
        }
    }
    v
}

/// "YYYY-MM-DD" in UTC, without a date library.
pub fn day_of(unix: f64) -> String {
    let days = (unix / 86400.0).floor() as i64;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_apps_from_their_scopes() {
        let c = |s: &str| {
            app_from_cgroup(&format!(
                "/user.slice/user-1000.slice/user@1000.service/app.slice/{s}"
            ))
        };
        assert_eq!(
            c("app-org.kde.dolphin@3f2c9a.service").as_deref(),
            Some("org.kde.dolphin")
        );
        assert_eq!(
            c("app-flatpak-org.mozilla.firefox-12345.scope").as_deref(),
            Some("org.mozilla.firefox")
        );
        assert_eq!(
            c("app-gnome-firefox-5678.scope").as_deref(),
            Some("firefox")
        );
        assert_eq!(c("app-steam-ab12cd.scope").as_deref(), Some("steam"));
        assert_eq!(c("app-code-oss-4321.scope").as_deref(), Some("code-oss"));
        assert_eq!(c("session-2.scope"), None);
        // Dynamis Power's own app and panel icon are never listed.
        let own = c("app-net.dynamis.power@1a2b.service").unwrap();
        assert!(is_own(&own) && is_own(&c("app-dynamis\\x2dagent@3c.service").unwrap()));
        assert!(!is_own("org.mozilla.firefox"));
        assert_eq!(
            app_from_cgroup("/system.slice/NetworkManager.service"),
            None
        );
    }

    #[test]
    fn dates() {
        assert_eq!(day_of(0.0), "1970-01-01");
        assert_eq!(day_of(1_794_816_000.0), "2026-11-16");
        assert_eq!(day_of(951_782_400.0), "2000-02-29");
    }

    fn p(pid: u32, app: &str, ticks: u64) -> Proc {
        Proc {
            pid,
            app: app.into(),
            cgroup: format!("/user.slice/app-{app}-1.scope"),
            cpu_ticks: ticks,
            runs: None,
        }
    }

    #[test]
    fn counts_wake_ups_per_app_from_every_thread() {
        let dir = std::env::temp_dir().join(format!("dynamis-wakes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let w = |rel: &str, v: &str| {
            let f = dir.join(rel);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, v).unwrap();
        };
        w("proc/40/stat", "40 (chat) S 1 0 0 0 0 0 0 0 0 0 10 5 0 0\n");
        w(
            "proc/40/cgroup",
            "0::/user.slice/user-1000.slice/app-chat.scope\n",
        );
        w("proc/40/task/40/schedstat", "1000 200 300\n");
        w("proc/40/task/41/schedstat", "1000 200 700\n");
        let root = SysRoot::at(&dir);
        let procs = scan(&root);
        assert_eq!(procs[0].runs, Some(1000));
        let mut m = Meter::new();
        m.add(&procs, 0.0, None, 1);
        w("proc/40/stat", "40 (chat) S 1 0 0 0 0 0 0 0 0 0 20 5 0 0\n");
        w("proc/40/task/41/schedstat", "1000 200 2700\n");
        m.add(&scan(&root), 10.0, None, 1);
        assert_eq!(m.last[0].wakeups_per_s, Some(200.0));
        assert!(m.last[0].wakeups_per_s.unwrap() > MANY_WAKEUPS_PER_S);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shares_power_above_idle_by_cpu_time() {
        let mut m = Meter::new();
        let t0 = 1_794_816_000.0;
        m.add(&[p(1, "firefox", 0), p(2, "konsole", 0)], t0, Some(4.0), 2);
        // 10 s later: firefox used 3 s of CPU, konsole 1 s; power 6 W, idle 4 W.
        m.add(
            &[p(1, "firefox", 300), p(2, "konsole", 100)],
            t0 + 10.0,
            Some(6.0),
            2,
        );
        let ff = m.last.iter().find(|n| n.app == "firefox").unwrap();
        assert!((ff.watts.unwrap() - 1.5).abs() < 1e-9, "{ff:?}");
        assert!((ff.cpu_pct - 15.0).abs() < 1e-9, "3 s of 20 core-seconds");
        let totals = m.totals(1, t0 + 10.0);
        assert_eq!(totals[0].0, "firefox");
        assert!((totals[0].1.wh - 1.5 * 10.0 / 3600.0).abs() < 1e-12);
        assert_eq!(m.running(), ["firefox", "konsole"]);
    }

    #[test]
    fn mains_power_counts_cpu_but_no_watts() {
        let mut m = Meter::new();
        m.add(&[p(1, "steam", 0)], 0.0, None, 1);
        m.add(&[p(1, "steam", 500)], 10.0, None, 1);
        assert_eq!(m.last[0].watts, None);
        assert_eq!(m.totals(1, 10.0)[0].1.cpu_s, 5.0);
        assert_eq!(m.totals(1, 10.0)[0].1.wh, 0.0);
    }

    #[test]
    fn keeps_a_week() {
        let mut m = Meter::new();
        for d in 0..12 {
            let t = d as f64 * 86400.0;
            m.add(&[p(1, "a", d * 100)], t, None, 1);
            m.add(&[p(1, "a", d * 100 + 100)], t + 10.0, None, 1);
        }
        assert_eq!(m.days.len(), KEEP_DAYS);
        assert_eq!(m.totals(7, 11.0 * 86400.0 + 10.0)[0].1.cpu_s, 7.0);
    }

    #[test]
    fn an_app_costing_more_than_last_month_is_a_trend() {
        let mut m = Meter::new();
        // A month of 30 days, then the next. Each "day": 3 hours on battery
        // at 10 W, with the app taking all the spare CPU; the idle level is
        // 5 W (seen first), so the app is given 5 W, then 8 W next month.
        let run = |m: &mut Meter, start: f64, watts: f64, ticks: &mut u64| {
            m.add(&[p(1, "firefox", *ticks)], start, Some(5.0), 1);
            for i in 1..=36 {
                *ticks += 30_000;
                m.add(
                    &[p(1, "firefox", *ticks)],
                    start + i as f64 * 300.0,
                    Some(watts),
                    1,
                );
            }
        };
        let mut ticks = 0;
        let oct = 1_759_276_800.0; // 2025-10-01
        let nov = 1_761_955_200.0; // 2025-11-01
        run(&mut m, oct, 10.0, &mut ticks);
        assert!(m.trends(oct + 3.0 * 3600.0).is_empty(), "no earlier month");
        run(&mut m, nov, 13.0, &mut ticks);
        let t = m.trends(nov + 3.0 * 3600.0);
        assert_eq!(t.len(), 1, "{t:?}");
        assert_eq!(t[0].app, "firefox");
        assert!((t[0].last_w - 5.0).abs() < 0.2, "{t:?}");
        assert!((t[0].this_w - 8.0).abs() < 0.2, "{t:?}");
        assert!(t[0].change_pct > 50.0);
        m.forget();
        assert!(m.trends(nov + 3.0 * 3600.0).is_empty());
    }
}
