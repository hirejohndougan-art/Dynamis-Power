//! The screen page's settings (decision 0029), acted on in the session.
//!
//! **Screen-off time.** The person's own times (on battery and plugged in)
//! are written to the desktop's own settings, so the desktop keeps turning
//! the screen off itself: KDE Plasma's `powerdevilrc` (the AC, Battery and
//! LowBattery groups, then PowerDevil is asked to reload), GNOME's
//! `idle-delay` (one value, so it is switched with the charger). What was
//! there before is recorded and put back when the person goes back to the
//! desktop's own time, at Restore, and when the agent stops.
//!
//! **Animated dim.** About 15 seconds before the screen turns off, the
//! brightness fades from where it is down to 10%; the desktop then turns the
//! screen off as usual. Any use during the fade puts the brightness straight
//! back. When the person comes back after the screen went off, it starts at
//! 10% and brightens to where it was in about a third of a second. The
//! desktop's own dimming is switched off meanwhile, so the two do not fight.
//! Brightness is set through logind, like the brightness keys.
//!
//! Idle time comes from the desktop (KDE: `org.freedesktop.ScreenSaver`
//! `GetSessionIdleTime`; GNOME: Mutter's idle monitor). The fade runs in its
//! own thread, which sleeps until the fade is due: no wake-ups while the
//! person is working, one reading a second only while the screen is off.
//!
//! None of this has been confirmed on a real desktop yet.

use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use zbus::Connection;

/// The fade starts this long before the screen turns off.
pub const FADE_MS: u64 = 15_000;
/// It fades down to this share of the brightness range.
pub const FADE_TO_PCT: u64 = 10;
/// Coming back: from 10% to where it was, this fast.
const BRIGHTEN_MS: u64 = 320;
const STEP_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desk {
    Kde,
    Gnome,
    Other,
}

/// The brightness a fade step should have: an ease-in curve from `from` to
/// `to`, `frac` of the way through (0 to 1).
pub fn fade_value(from: u64, to: u64, frac: f64) -> u64 {
    let f = frac.clamp(0.0, 1.0);
    let eased = f * f * (3.0 - 2.0 * f);
    let v = from as f64 + (to as f64 - from as f64) * eased;
    v.round().max(1.0) as u64
}

/// The fade's lowest brightness for a backlight with this maximum.
pub fn floor_for(max: u64, from: u64) -> u64 {
    (max * FADE_TO_PCT / 100).max(1).min(from)
}

// --- The desktop's own settings --------------------------------------------

/// What one setting was before Dynamis Power changed it (`None`: it was not
/// set, so putting back deletes it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Was {
    pub group: String,
    pub key: String,
    pub value: Option<String>,
}

fn kde_read(group: &str, key: &str) -> Option<String> {
    for tool in ["kreadconfig6", "kreadconfig5"] {
        if let Ok(o) = Command::new(tool)
            .args([
                "--file",
                "powerdevilrc",
                "--group",
                group,
                "--group",
                "Display",
                "--key",
                key,
            ])
            .output()
        {
            if o.status.success() {
                let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
                return (!v.is_empty()).then_some(v);
            }
        }
    }
    None
}

fn kde_write(group: &str, key: &str, value: Option<&str>) -> bool {
    let mut args = vec![
        "--file",
        "powerdevilrc",
        "--group",
        group,
        "--group",
        "Display",
        "--key",
        key,
    ];
    match value {
        Some(v) => args.push(v),
        None => args.push("--delete"),
    }
    ["kwriteconfig6", "kwriteconfig5"].iter().any(|t| {
        Command::new(t)
            .args(&args)
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    let o = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn gsettings_set(schema: &str, key: &str, value: &str) -> bool {
    Command::new("gsettings")
        .args(["set", schema, key, value])
        .output()
        .is_ok_and(|o| o.status.success())
}

const GNOME_IDLE: (&str, &str) = ("org.gnome.desktop.session", "idle-delay");
const GNOME_DIM: (&str, &str) = ("org.gnome.settings-daemon.plugins.power", "idle-dim");

/// "uint32 300" or "300" → 300.
fn gnome_number(v: &str) -> Option<u32> {
    v.split_whitespace().last()?.parse().ok()
}

/// Ask PowerDevil to read its settings again.
async fn kde_reload(session: &Connection) {
    let _ = session
        .call_method(
            Some("org.kde.Solid.PowerManagement"),
            "/org/kde/Solid/PowerManagement",
            Some("org.kde.Solid.PowerManagement"),
            "refreshStatus",
            &(),
        )
        .await;
}

/// The screen-off time the desktop uses now, in seconds (None: never).
pub fn desktop_timeout(desk: Desk, on_ac: bool) -> Option<u32> {
    match desk {
        Desk::Kde => {
            let g = if on_ac { "AC" } else { "Battery" };
            if kde_read(g, "TurnOffDisplayWhenIdle").as_deref() == Some("false") {
                return None;
            }
            // PowerDevil's own default when nothing is written: 5 minutes on
            // battery, 10 plugged in.
            Some(
                kde_read(g, "TurnOffDisplayIdleTimeoutSec")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(if on_ac { 600 } else { 300 }),
            )
        }
        Desk::Gnome => gsettings_get(GNOME_IDLE.0, GNOME_IDLE.1)
            .and_then(|v| gnome_number(&v))
            .filter(|s| *s > 0),
        Desk::Other => None,
    }
}

/// Write the person's screen-off times (and, with animated dim, switch the
/// desktop's own dimming off), recording what was there first in `was`
/// (only the first time each setting is touched).
pub async fn apply(
    session: &Connection,
    desk: Desk,
    battery_s: Option<u32>,
    ac_s: Option<u32>,
    animated: bool,
    on_ac: bool,
    was: &mut Vec<Was>,
) {
    let mut remember = |group: &str, key: &str, value: Option<String>| {
        if !was.iter().any(|w| w.group == group && w.key == key) {
            was.push(Was {
                group: group.into(),
                key: key.into(),
                value,
            });
        }
    };
    match desk {
        Desk::Kde => {
            for (group, secs) in [
                ("AC", ac_s),
                ("Battery", battery_s),
                ("LowBattery", battery_s),
            ] {
                if let Some(s) = secs {
                    for key in ["TurnOffDisplayWhenIdle", "TurnOffDisplayIdleTimeoutSec"] {
                        remember(group, key, kde_read(group, key));
                    }
                    kde_write(group, "TurnOffDisplayWhenIdle", Some("true"));
                    kde_write(group, "TurnOffDisplayIdleTimeoutSec", Some(&s.to_string()));
                }
                if animated {
                    remember(
                        group,
                        "DimDisplayWhenIdle",
                        kde_read(group, "DimDisplayWhenIdle"),
                    );
                    kde_write(group, "DimDisplayWhenIdle", Some("false"));
                }
            }
            kde_reload(session).await;
        }
        Desk::Gnome => {
            if let Some(s) = if on_ac { ac_s } else { battery_s } {
                remember(
                    GNOME_IDLE.0,
                    GNOME_IDLE.1,
                    gsettings_get(GNOME_IDLE.0, GNOME_IDLE.1),
                );
                gsettings_set(GNOME_IDLE.0, GNOME_IDLE.1, &s.to_string());
            }
            if animated {
                remember(
                    GNOME_DIM.0,
                    GNOME_DIM.1,
                    gsettings_get(GNOME_DIM.0, GNOME_DIM.1),
                );
                gsettings_set(GNOME_DIM.0, GNOME_DIM.1, "false");
            }
        }
        Desk::Other => {}
    }
}

/// Put back everything in `was` and forget it.
pub async fn put_back(session: &Connection, desk: Desk, was: &mut Vec<Was>) {
    if was.is_empty() {
        return;
    }
    for w in was.drain(..) {
        match desk {
            Desk::Kde => {
                kde_write(&w.group, &w.key, w.value.as_deref());
            }
            Desk::Gnome => match &w.value {
                Some(v) => {
                    gsettings_set(&w.group, &w.key, v);
                }
                None => {
                    let _ = Command::new("gsettings")
                        .args(["reset", &w.group, &w.key])
                        .output();
                }
            },
            Desk::Other => {}
        }
    }
    if desk == Desk::Kde {
        kde_reload(session).await;
    }
}

// --- Animated dim -----------------------------------------------------------

/// What the fade thread needs to know; changed by the agent's main loop.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DimConfig {
    pub enabled: bool,
    /// The screen-off time in force now, seconds.
    pub timeout_s: Option<u32>,
}

/// Shared between the main loop and the fade thread. The `Condvar` wakes
/// the thread at once when the settings change, instead of it checking on a
/// timer.
pub type DimShared = Arc<(Mutex<DimConfig>, Condvar)>;

pub fn set_config(shared: &DimShared, c: DimConfig) {
    let (m, cv) = &**shared;
    if let Ok(mut g) = m.lock() {
        if *g != c {
            *g = c;
            cv.notify_all();
        }
    }
}

fn idle_ms(session: &Connection, desk: Desk) -> Option<u64> {
    zbus::block_on(async {
        match desk {
            Desk::Kde => session
                .call_method(
                    Some("org.freedesktop.ScreenSaver"),
                    "/ScreenSaver",
                    Some("org.freedesktop.ScreenSaver"),
                    "GetSessionIdleTime",
                    &(),
                )
                .await
                .ok()?
                .body()
                .deserialize::<u32>()
                .ok()
                .map(u64::from),
            Desk::Gnome => session
                .call_method(
                    Some("org.gnome.Mutter.IdleMonitor"),
                    "/org/gnome/Mutter/IdleMonitor/Core",
                    Some("org.gnome.Mutter.IdleMonitor"),
                    "GetIdletime",
                    &(),
                )
                .await
                .ok()?
                .body()
                .deserialize::<u64>()
                .ok(),
            Desk::Other => None,
        }
    })
}

fn set_brightness(system: &Connection, name: &str, value: u64) {
    let _ = zbus::block_on(dynamis_client::backlight::set_raw(
        system,
        name,
        value as u32,
    ));
}

/// Wait up to `ms`, or until the settings change. Returns the settings.
fn wait(shared: &DimShared, ms: u64) -> DimConfig {
    let (m, cv) = &**shared;
    let Ok(g) = m.lock() else {
        return DimConfig::default();
    };
    let before = *g;
    match cv.wait_timeout_while(g, Duration::from_millis(ms), |c| *c == before) {
        Ok((g, _)) => *g,
        Err(_) => before,
    }
}

/// Start the fade thread. It does nothing until animated dim is on and a
/// screen-off time is known.
pub fn start(desk: Desk, shared: DimShared, verbose: bool) {
    if desk == Desk::Other {
        return;
    }
    std::thread::spawn(move || {
        let (Ok(session), Ok(system)) = (
            zbus::block_on(Connection::session()),
            zbus::block_on(Connection::system()),
        ) else {
            return;
        };
        let mut cfg = wait(&shared, 0);
        loop {
            let (true, Some(t)) = (cfg.enabled, cfg.timeout_s) else {
                cfg = wait(&shared, u64::MAX / 4);
                continue;
            };
            let t_ms = u64::from(t) * 1000;
            let fade_at = t_ms.saturating_sub(FADE_MS.min(t_ms / 2));
            let Some(idle) = idle_ms(&session, desk) else {
                cfg = wait(&shared, 60_000);
                continue;
            };
            if idle < fade_at {
                // Sleep until the fade is due (plus a little), unless the
                // settings change.
                cfg = wait(&shared, fade_at - idle + 50);
                continue;
            }
            if zbus::block_on(crate::screen_inhibited(&session)) {
                // A video or a presentation keeps the screen on: no fade.
                cfg = wait(&shared, 10_000);
                continue;
            }
            let Some((name, from, max)) = dynamis_client::backlight::raw() else {
                cfg = wait(&shared, 60_000);
                continue;
            };
            let floor = floor_for(max, from);
            let mut last_idle = idle;
            let mut used = false;
            if idle < t_ms {
                if verbose {
                    eprintln!("dynamis-agent: animated dim: fading from {from} to {floor}");
                }
                // The fade, until the screen-off time or until used.
                while let Some(i) = idle_ms(&session, desk) {
                    if i < last_idle {
                        used = true;
                        break;
                    }
                    last_idle = i;
                    if i >= t_ms {
                        break;
                    }
                    let frac = (i - fade_at) as f64 / (t_ms - fade_at) as f64;
                    set_brightness(&system, &name, fade_value(from, floor, frac));
                    std::thread::sleep(Duration::from_millis(STEP_MS));
                }
                if used {
                    // Used during the fade: straight back.
                    set_brightness(&system, &name, from);
                    cfg = wait(&shared, 0);
                    continue;
                }
                set_brightness(&system, &name, floor);
            }
            // The desktop turns the screen off now. Wait for the person to
            // come back (one reading a second while the screen is off).
            let off_since = Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(1000));
                match idle_ms(&session, desk) {
                    Some(i) if i < last_idle || i < 2000 => break,
                    Some(i) => last_idle = i,
                    None => break,
                }
                if off_since.elapsed() > Duration::from_secs(24 * 3600) {
                    break;
                }
            }
            // Back: from 10% to where it was, quickly.
            let steps = 8;
            for k in 1..=steps {
                set_brightness(
                    &system,
                    &name,
                    fade_value(floor, from, k as f64 / steps as f64),
                );
                std::thread::sleep(Duration::from_millis(BRIGHTEN_MS / steps));
            }
            set_brightness(&system, &name, from);
            cfg = wait(&shared, 0);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_curve_and_floor() {
        assert_eq!(fade_value(1000, 100, 0.0), 1000);
        assert_eq!(fade_value(1000, 100, 1.0), 100);
        assert_eq!(fade_value(1000, 100, 0.5), 550);
        // Gentle at the start: a quarter of the way in, under a fifth down.
        assert!(fade_value(1000, 100, 0.25) > 1000 - 180);
        assert_eq!(floor_for(19200, 9600), 1920);
        assert_eq!(floor_for(19200, 500), 500, "already below 10%: stays");
        assert_eq!(floor_for(7, 7), 1);
        assert_eq!(gnome_number("uint32 300"), Some(300));
        assert_eq!(gnome_number("0"), Some(0));
    }

    #[test]
    fn settings_changes_wake_the_thread() {
        let shared: DimShared = Arc::new((Mutex::new(DimConfig::default()), Condvar::new()));
        let s2 = shared.clone();
        let h = std::thread::spawn(move || wait(&s2, 10_000));
        std::thread::sleep(Duration::from_millis(50));
        set_config(
            &shared,
            DimConfig {
                enabled: true,
                timeout_s: Some(120),
            },
        );
        let got = h.join().unwrap();
        assert_eq!(got.timeout_s, Some(120));
    }
}
