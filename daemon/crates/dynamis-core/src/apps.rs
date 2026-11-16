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
//! Totals are kept per day for a week, on this computer only.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::fs::SysRoot;

const FORMAT_VERSION: u32 = 1;
const KEEP_DAYS: usize = 8;
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

/// Every process with its app and CPU time. Kernel threads are skipped.
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
        out.push(Proc {
            pid,
            app,
            cgroup,
            cpu_ticks: ticks,
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
}

/// One reading's result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Now {
    pub app: String,
    pub cpu_pct: f64,
    /// Estimated watts (on battery only).
    pub watts: Option<f64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Meter {
    v: u32,
    /// Day ("2026-11-16", UTC) → app → usage.
    days: BTreeMap<String, BTreeMap<String, Usage>>,
    #[serde(skip)]
    prev: HashMap<u32, u64>,
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

impl Meter {
    pub fn new() -> Self {
        Meter {
            v: FORMAT_VERSION,
            ..Default::default()
        }
    }

    /// Add one reading. `now` is Unix time; `power_w` is whole-system power
    /// on battery (None on mains); `cores` scales CPU %.
    pub fn add(&mut self, procs: &[Proc], now: f64, power_w: Option<f64>, cores: usize) {
        if let Some(w) = power_w {
            self.base_w = Some(self.base_w.map_or(w, |b| b.min(w)));
        }
        let dt = self.prev_time.map(|t| now - t).filter(|d| *d > 0.0);
        let mut per_app: BTreeMap<String, u64> = BTreeMap::new();
        let mut cgroups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut next = HashMap::with_capacity(procs.len());
        for p in procs {
            next.insert(p.pid, p.cpu_ticks);
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
        self.prev_time = Some(now);
        self.cgroups = cgroups;
        let Some(dt) = dt else {
            self.last.clear();
            return;
        };

        let total: u64 = per_app.values().sum();
        let spare_w = power_w.zip(self.base_w).map(|(w, b)| (w - b).max(0.0));
        let day = day_of(now);
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

    /// Apps with at least one process right now.
    pub fn running(&self) -> Vec<String> {
        self.cgroups.keys().cloned().collect()
    }
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
        }
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
}
