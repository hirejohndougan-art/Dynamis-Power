//! The system half of auto mode's readings: what the root service can see
//! without help from the desktop session. The session half (media, screen
//! lock, games) comes from the session agent. See [`crate::auto::Context`].
//!
//! Everything here reads through [`SysRoot`], so tests use fixtures.

use crate::auto::Context;
use crate::fs::SysRoot;
use crate::sensors::{backlight, power, pressure, thermal};

/// Keeps the previous reading of the counters that need two readings
/// (CPU time, network bytes).
#[derive(Debug, Default)]
pub struct Sampler {
    cpu: Option<(u64, u64)>,
    net: Option<(u64, f64)>,
    /// Previous PSI "some" totals for CPU, memory and disk, and when.
    stall: Option<([Option<u64>; 3], f64)>,
    /// For batteries that report energy but not power: the last energy
    /// reading that changed, and when; and the power worked out from it.
    energy: Option<(f64, f64)>,
    energy_power: Option<f64>,
}

impl Sampler {
    /// Read the system now. `now` is seconds (any fixed origin) and is only
    /// used to turn byte counts into rates. The first call reports 0% CPU
    /// and no traffic, since it has nothing to compare with.
    pub fn read(&mut self, root: &SysRoot, now: f64) -> Context {
        let ps = power::read(root);
        // No battery at all: a desktop, or a laptop with its battery removed.
        let on_ac = ps.on_ac() || ps.batteries.is_empty();
        let battery_pct = {
            let v: Vec<f64> = ps
                .batteries
                .iter()
                .filter_map(|b| b.capacity_pct)
                .map(f64::from)
                .collect();
            (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
        };

        let cpu_busy_pct = match (cpu_times(root), self.cpu) {
            (Some((busy, total)), Some((pb, pt))) if total > pt => {
                self.cpu = Some((busy, total));
                (busy.saturating_sub(pb)) as f64 / (total - pt) as f64 * 100.0
            }
            (now_times, _) => {
                self.cpu = now_times;
                0.0
            }
        };

        let net_mb_s = match (net_bytes(root), self.net) {
            (Some(b), Some((pb, pt))) if now > pt => {
                self.net = Some((b, now));
                b.saturating_sub(pb) as f64 / (now - pt) / 1_000_000.0
            }
            (b, _) => {
                self.net = b.map(|b| (b, now));
                0.0
            }
        };

        // Stall: the growth of the PSI "some" counters for CPU, memory and
        // disk, as a share of the time since the last reading.
        // Each counter is compared only with its own previous value, so a
        // counter missing from one reading cannot make the next look huge.
        let p = pressure::read(root);
        let totals = [p.cpu, p.memory, p.io].map(|x| x.map(|x| x.some.total_us));
        let stall_pct = match self.stall {
            Some((prev, pt)) if now > pt => {
                let grown: u64 = totals
                    .iter()
                    .zip(prev.iter())
                    .filter_map(|(cur, before)| {
                        Some(cur.as_ref()?.saturating_sub(*before.as_ref()?))
                    })
                    .sum();
                grown as f64 / ((now - pt) * 1e6) * 100.0
            }
            _ => 0.0,
        };
        self.stall = Some((totals, now));

        let energy_wh = {
            let e: Vec<f64> = ps
                .batteries
                .iter()
                .filter_map(|b| b.energy_now_wh)
                .collect();
            (!e.is_empty()).then(|| e.iter().sum::<f64>())
        };
        let reported = if on_ac {
            None
        } else {
            ps.total_battery_power_w()
        };
        // No power reading on battery: work it out from the fall in energy
        // between two readings that differ (coarse, so learning lengthens
        // its tests to match how rarely it changes).
        let power_w = match (on_ac, reported, energy_wh) {
            (true, _, _) => {
                self.energy = None;
                self.energy_power = None;
                None
            }
            (false, Some(w), _) => Some(w),
            (false, None, Some(e)) => {
                match self.energy {
                    Some((pe, pt)) if (pe - e).abs() > 1e-9 && now > pt => {
                        if pe > e {
                            self.energy_power = Some((pe - e) / (now - pt) * 3600.0);
                        }
                        self.energy = Some((e, now));
                    }
                    None => self.energy = Some((e, now)),
                    _ => {}
                }
                self.energy_power
            }
            (false, None, None) => None,
        };

        Context {
            on_ac,
            battery_pct,
            cpu_busy_pct,
            net_mb_s,
            power_w,
            stall_pct,
            brightness_pct: backlight::read(root).map(|b| b.percent),
            max_temp_c: thermal::max_c(&thermal::read(root)),
            mic_in_use: mic_in_use(root),
            energy_wh,
            ..Context::default()
        }
    }
}

/// (busy, total) CPU time since boot from the first line of /proc/stat.
fn cpu_times(root: &SysRoot) -> Option<(u64, u64)> {
    let stat = root.read_string("/proc/stat").ok()?;
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() < 4 {
        return None;
    }
    // user nice system idle iowait irq softirq steal [guest guest_nice]; guest
    // time is already counted in user, so only the first eight are summed.
    let total: u64 = v.iter().take(8).sum();
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    Some((total - idle, total))
}

/// Bytes received plus sent on every interface except loopback.
fn net_bytes(root: &SysRoot) -> Option<u64> {
    let text = root.read_string("/proc/net/dev").ok()?;
    let mut sum = 0u64;
    for line in text.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let f: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        // Field 0 is bytes received, field 8 bytes sent.
        sum += f.first().copied().unwrap_or(0) + f.get(8).copied().unwrap_or(0);
    }
    Some(sum)
}

/// True when any sound card is recording. Reads the ALSA status files; with
/// PipeWire or PulseAudio the hardware capture stream shows here too while a
/// microphone is open.
fn mic_in_use(root: &SysRoot) -> bool {
    for card in root
        .list_dir("/proc/asound")
        .into_iter()
        .filter(|c| c.starts_with("card"))
    {
        let card_dir = format!("/proc/asound/{card}");
        // Capture devices end in "c", e.g. pcm0c.
        for pcm in root
            .list_dir(&card_dir)
            .into_iter()
            .filter(|p| p.starts_with("pcm") && p.ends_with('c'))
        {
            let pcm_dir = format!("{card_dir}/{pcm}");
            for sub in root
                .list_dir(&pcm_dir)
                .into_iter()
                .filter(|s| s.starts_with("sub"))
            {
                if root
                    .read_string(&format!("{pcm_dir}/{sub}/status"))
                    .is_ok_and(|s| s.contains("state: RUNNING"))
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sandbox(name: &str) -> (std::path::PathBuf, SysRoot) {
        let dir = std::env::temp_dir().join(format!("dynamis-ctx-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/sysfs/synthetic-laptop");
        assert!(std::process::Command::new("cp")
            .arg("-a")
            .arg(&src)
            .arg(&dir)
            .status()
            .unwrap()
            .success());
        (dir.clone(), SysRoot::at(dir))
    }

    #[test]
    fn reads_the_fixture() {
        let (dir, root) = sandbox("read");
        let mut s = Sampler::default();
        let first = s.read(&root, 0.0);
        assert!(!first.on_ac);
        assert_eq!(first.battery_pct, Some(79.0));
        assert_eq!(first.cpu_busy_pct, 0.0);
        assert!(!first.mic_in_use);

        fs::write(dir.join("proc/stat"), "cpu  100 0 100 800 0 0 0 0 0 0\n").unwrap();
        fs::create_dir_all(dir.join("proc/net")).unwrap();
        fs::write(dir.join("proc/net/dev"), "h1\nh2\n    lo: 999999999 0 0 0 0 0 0 0 999999999 0\n  wlan0: 1000000 0 0 0 0 0 0 0 0 0\n").unwrap();
        s.read(&root, 10.0);
        fs::write(dir.join("proc/stat"), "cpu  250 0 250 900 0 0 0 0 0 0\n").unwrap();
        fs::write(
            dir.join("proc/net/dev"),
            "h1\nh2\n    lo: 0 0 0 0 0 0 0 0 0 0\n  wlan0: 21000000 0 0 0 0 0 0 0 0 0\n",
        )
        .unwrap();
        let c = s.read(&root, 20.0);
        // 300 busy out of 400 total.
        assert_eq!(c.cpu_busy_pct, 75.0);
        // 20 MB in 10 s, loopback ignored.
        assert_eq!(c.net_mb_s, 2.0);

        let sub = dir.join("proc/asound/card0/pcm0c/sub0");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("status"), "state: RUNNING\nowner_pid   : 1234\n").unwrap();
        assert!(s.read(&root, 30.0).mic_in_use);
        fs::write(sub.join("status"), "closed\n").unwrap();
        assert!(!s.read(&root, 40.0).mic_in_use);

        // A pressure counter missing from one reading is not counted as
        // stall when it comes back.
        let cpu = dir.join("proc/pressure/cpu");
        let original = fs::read_to_string(&cpu).unwrap();
        fs::write(&cpu, "").unwrap();
        s.read(&root, 50.0);
        fs::write(&cpu, original).unwrap();
        assert_eq!(s.read(&root, 60.0).stall_pct, 0.0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn power_from_energy_when_power_is_missing() {
        let (dir, root) = sandbox("energy");
        let bat = dir.join("sys/class/power_supply/BAT0");
        fs::remove_file(bat.join("power_now")).unwrap();
        let _ = fs::remove_file(bat.join("current_now"));
        fs::write(bat.join("energy_now"), "30000000\n").unwrap();
        let mut s = Sampler::default();
        assert_eq!(s.read(&root, 0.0).power_w, None);
        // Unchanged energy: still nothing to go on.
        assert_eq!(s.read(&root, 30.0).power_w, None);
        // 0.1 Wh in 60 s is 6 W.
        fs::write(bat.join("energy_now"), "29900000\n").unwrap();
        let w = s.read(&root, 60.0).power_w.unwrap();
        assert!((w - 6.0).abs() < 1e-6, "{w}");
        // Kept until the next change.
        assert!((s.read(&root, 90.0).power_w.unwrap() - 6.0).abs() < 1e-6);
        fs::write(bat.join("energy_now"), "29850000\n").unwrap();
        let w = s.read(&root, 120.0).power_w.unwrap();
        assert!((w - 3.0).abs() < 1e-6, "{w}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_battery_means_mains() {
        let (dir, root) = sandbox("desktop");
        fs::remove_dir_all(dir.join("sys/class/power_supply")).unwrap();
        let c = Sampler::default().read(&root, 0.0);
        assert!(c.on_ac);
        assert_eq!(c.battery_pct, None);
        fs::remove_dir_all(&dir).ok();
    }
}
