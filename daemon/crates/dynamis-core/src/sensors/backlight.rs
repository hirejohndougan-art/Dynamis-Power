//! Screen backlight level from `/sys/class/backlight`.

use serde::Serialize;

use crate::fs::SysRoot;

const BACKLIGHT: &str = "/sys/class/backlight";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Backlight {
    pub name: String,
    pub brightness: u64,
    pub max_brightness: u64,
    pub percent: f64,
}

/// The first backlight device that reports sensible values.
pub fn read(root: &SysRoot) -> Option<Backlight> {
    root.list_dir(BACKLIGHT).into_iter().find_map(|name| {
        let base = format!("{BACKLIGHT}/{name}");
        let brightness = root.read_u64(&format!("{base}/brightness")).ok()?;
        let max_brightness = root.read_u64(&format!("{base}/max_brightness")).ok()?;
        if max_brightness == 0 {
            return None;
        }
        let percent = (brightness as f64 / max_brightness as f64 * 1000.0).round() / 10.0;
        Some(Backlight {
            name,
            brightness,
            max_brightness,
            percent,
        })
    })
}
