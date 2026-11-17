//! The screen brightness slider in the quick panel.
//!
//! The person moves it, so this is their own brightness, not a setting
//! Dynamis Power tunes: it is set through logind (`SetBrightness` on the
//! person's own session), the same way the desktop's brightness keys do it,
//! with no root and nothing journaled. The desktop notices the change and
//! moves its own slider to match.

use std::path::{Path, PathBuf};

use zbus::Connection;

const DIR: &str = "/sys/class/backlight";

/// Lowest the slider sets, so the screen never goes fully dark by accident.
pub const MIN_PCT: u32 = 5;

/// The screen's backlight: the first that reports sensible values, preferring
/// the graphics driver's own (`raw`, `platform`) over the firmware's.
fn device(dir: &Path) -> Option<(String, u64, u64)> {
    let mut found: Vec<(u8, String, u64, u64)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let p = e.path();
            let read = |f: &str| std::fs::read_to_string(p.join(f)).ok();
            let max: u64 = read("max_brightness")?
                .trim()
                .parse()
                .ok()
                .filter(|m| *m > 0)?;
            let now: u64 = read("brightness")?.trim().parse().ok()?;
            let rank = match read("type").as_deref().map(str::trim) {
                Some("raw") => 0,
                Some("platform") => 1,
                _ => 2,
            };
            Some((rank, name, now, max))
        })
        .collect();
    found.sort();
    found
        .into_iter()
        .next()
        .map(|(_, n, now, max)| (n, now, max))
}

/// The backlight's name, raw brightness now and its maximum.
pub fn raw() -> Option<(String, u64, u64)> {
    device(Path::new(DIR))
}

/// Set a raw brightness through logind (for the session agent's animated
/// dim, which needs finer steps than percent).
pub async fn set_raw(conn: &Connection, name: &str, value: u32) -> Result<(), String> {
    conn.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1/session/auto",
        Some("org.freedesktop.login1.Session"),
        "SetBrightness",
        &("backlight", name, value),
    )
    .await
    .map(|_| ())
    .map_err(|e| format!("The brightness could not be set: {e}"))
}

/// The screen brightness now, in percent (None without a backlight).
pub fn percent() -> Option<u32> {
    percent_in(Path::new(DIR))
}

fn percent_in(dir: &Path) -> Option<u32> {
    let (_, now, max) = device(dir)?;
    Some(((now as f64 / max as f64) * 100.0).round() as u32)
}

/// The raw value for a percentage, never below `MIN_PCT`.
pub fn raw_for(pct: u32, max: u64) -> u32 {
    let pct = pct.clamp(MIN_PCT, 100) as f64;
    ((max as f64 * pct / 100.0).round() as u64).clamp(1, max) as u32
}

/// Set the screen brightness to a percentage through logind.
pub async fn set_percent(pct: u32) -> Result<(), String> {
    set_percent_in(PathBuf::from(DIR).as_path(), pct).await
}

async fn set_percent_in(dir: &Path, pct: u32) -> Result<(), String> {
    let (name, _, max) = device(dir).ok_or("This screen's brightness cannot be set here.")?;
    let conn = Connection::system().await.map_err(|e| e.to_string())?;
    set_raw(&conn, &name, raw_for(pct, max)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_graphics_backlight_and_converts() {
        let dir = std::env::temp_dir().join(format!("dynamis-client-bl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, kind, now, max) in [
            ("acpi_video0", "firmware", "50", "100"),
            ("intel_backlight", "raw", "9600", "19200"),
        ] {
            let d = dir.join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("type"), kind).unwrap();
            std::fs::write(d.join("brightness"), now).unwrap();
            std::fs::write(d.join("max_brightness"), max).unwrap();
        }
        assert_eq!(device(&dir).unwrap().0, "intel_backlight");
        assert_eq!(percent_in(&dir), Some(50));
        assert_eq!(raw_for(40, 19200), 7680);
        assert_eq!(raw_for(0, 19200), 960, "never below 5%");
        assert_eq!(raw_for(100, 7), 7);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
