//! Local time, for "the rest of the day". The service runs as root with the
//! system's time zone (`/etc/localtime`), which is the person's.

/// The next local midnight after `now` (Unix seconds). Falls back to 12
/// hours from now if the local time cannot be worked out.
pub fn next_midnight(now: f64) -> f64 {
    let t = now as libc::time_t;
    // SAFETY: `tm` is plain data; localtime_r fills it and is thread-safe;
    // mktime only reads and normalises it. Both results are checked.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return now + 12.0 * 3600.0;
        }
        tm.tm_hour = 0;
        tm.tm_min = 0;
        tm.tm_sec = 0;
        tm.tm_mday += 1;
        tm.tm_isdst = -1;
        let m = libc::mktime(&mut tm);
        if m == -1 || (m as f64) <= now {
            now + 12.0 * 3600.0
        } else {
            m as f64
        }
    }
}

/// The local hour (0-23) at `now`, if the local time can be worked out.
pub fn local_hour(now: f64) -> Option<u8> {
    local(now).map(|tm| tm.tm_hour as u8)
}

/// The local day number since 1970 (for "on different days").
pub fn local_day(now: f64) -> Option<i64> {
    local(now).map(|tm| {
        // Days from the local calendar date (same algorithm as heat.rs).
        let (y, m, d) = (
            tm.tm_year as i64 + 1900,
            tm.tm_mon as i64 + 1,
            tm.tm_mday as i64,
        );
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    })
}

fn local(now: f64) -> Option<libc::tm> {
    let t = now as libc::time_t;
    // SAFETY: `tm` is plain data that localtime_r fills; the result is
    // checked; localtime_r is thread-safe.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        (!libc::localtime_r(&t, &mut tm).is_null()).then_some(tm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midnight_is_within_a_day() {
        let now = 1_795_000_000.0;
        let m = next_midnight(now);
        assert!(m > now && m <= now + 25.0 * 3600.0, "{m}");
    }
}
