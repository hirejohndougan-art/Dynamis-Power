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
