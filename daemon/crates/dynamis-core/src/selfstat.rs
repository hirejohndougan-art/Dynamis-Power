//! The running process's own CPU and memory use.
//!
//! The project promises the daemon will cost less than it saves (under 0.5%
//! average CPU and 20 MB of memory; see `docs/ARCHITECTURE.md`). This module
//! measures that. It reads the real `/proc/self`, not a [`crate::SysRoot`],
//! because it describes this process rather than the machine under test.

/// Kernel clock ticks per second for `/proc/<pid>/stat` times. This is
/// `USER_HZ`, which is 100 on every mainstream Linux architecture.
const TICKS_PER_SEC: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelfUsage {
    /// User plus system CPU time used so far, in seconds.
    pub cpu_s: f64,
    /// Resident memory, in MB.
    pub rss_mb: Option<f64>,
}

impl SelfUsage {
    pub fn read() -> Option<SelfUsage> {
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        let status = std::fs::read_to_string("/proc/self/status").ok();
        Some(SelfUsage {
            cpu_s: parse_cpu_seconds(&stat)?,
            rss_mb: status.as_deref().and_then(parse_rss_mb),
        })
    }

    /// CPU use between two readings, as a percent of one core.
    pub fn cpu_pct_since(&self, earlier: &SelfUsage, wall_s: f64) -> Option<f64> {
        if wall_s <= 0.0 || self.cpu_s < earlier.cpu_s {
            return None;
        }
        Some(((self.cpu_s - earlier.cpu_s) / wall_s * 100.0 * 1000.0).round() / 1000.0)
    }
}

/// utime and stime are fields 14 and 15. The process name (field 2) is in
/// parentheses and may contain spaces, so count fields after the last `)`.
pub fn parse_cpu_seconds(stat: &str) -> Option<f64> {
    let after = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = after.split_whitespace().collect();
    // After ")": field 3 is index 0, so utime (14) is index 11 and stime (15) is 12.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some((utime + stime) as f64 / TICKS_PER_SEC)
}

pub fn parse_rss_mb(status: &str) -> Option<f64> {
    let kb: f64 = status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some((kb / 1024.0 * 10.0).round() / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stat_with_spaces_in_name() {
        let stat = "1234 (my prog) S 1 1234 1234 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 1 0 100 1000000 300 0";
        assert_eq!(parse_cpu_seconds(stat), Some(3.0));
    }

    #[test]
    fn parses_rss() {
        assert_eq!(parse_rss_mb("Name:\tx\nVmRSS:\t    5120 kB\n"), Some(5.0));
    }

    #[test]
    fn reads_this_process() {
        let u = SelfUsage::read().expect("/proc/self should exist on Linux");
        assert!(u.cpu_s >= 0.0);
    }
}
