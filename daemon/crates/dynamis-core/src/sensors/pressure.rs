//! Pressure stall information (PSI) from `/proc/pressure`.
//!
//! PSI reports the share of time tasks were stalled waiting for a resource.
//! It is the project's main "is the system lagging" measure.
//! See <https://docs.kernel.org/accounting/psi.html>.

use serde::Serialize;

use crate::fs::SysRoot;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Default)]
pub struct PsiLine {
    /// Percent of time stalled, averaged over 10, 60 and 300 seconds.
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    /// Total stall time since boot, in microseconds.
    pub total_us: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Default)]
pub struct Psi {
    /// Some tasks stalled.
    pub some: PsiLine,
    /// All non-idle tasks stalled at once (absent for system-wide CPU on older kernels).
    pub full: Option<PsiLine>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Default)]
pub struct Pressure {
    pub cpu: Option<Psi>,
    pub memory: Option<Psi>,
    pub io: Option<Psi>,
}

pub fn read(root: &SysRoot) -> Pressure {
    let r = |res: &str| {
        root.read_string(&format!("/proc/pressure/{res}"))
            .ok()
            .and_then(|s| parse(&s))
    };
    Pressure {
        cpu: r("cpu"),
        memory: r("memory"),
        io: r("io"),
    }
}

/// Parse the contents of a PSI file. Returns `None` if no `some` line is found.
pub fn parse(text: &str) -> Option<Psi> {
    let mut some = None;
    let mut full = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let kind = parts.next()?;
        let mut l = PsiLine::default();
        for kv in parts {
            let (k, v) = kv.split_once('=')?;
            match k {
                "avg10" => l.avg10 = v.parse().ok()?,
                "avg60" => l.avg60 = v.parse().ok()?,
                "avg300" => l.avg300 = v.parse().ok()?,
                "total" => l.total_us = v.parse().ok()?,
                _ => {}
            }
        }
        match kind {
            "some" => some = Some(l),
            "full" => full = Some(l),
            _ => {}
        }
    }
    Some(Psi { some: some?, full })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_lines() {
        let p = parse("some avg10=1.50 avg60=0.80 avg300=0.10 total=1234\nfull avg10=0.25 avg60=0.00 avg300=0.00 total=99\n").unwrap();
        assert_eq!(p.some.avg10, 1.5);
        assert_eq!(p.some.total_us, 1234);
        assert_eq!(p.full.unwrap().avg10, 0.25);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("nonsense").is_none());
        assert!(parse("").is_none());
    }
}
