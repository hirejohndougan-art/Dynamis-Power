//! Acting through the desktop (decision 0027): the service decides, the
//! session agent acts. The service keeps the person's choices and says what
//! it wants now (`desktop` in `Status`, and `Desktop()` for the agent); the
//! agent pauses file indexing, lowers the screen's refresh rate or turns on
//! the dark style through the desktop's own interfaces, and puts each back.
//! The agent says what this desktop can do (`ReportDesktop`).

use std::path::Path;

use dynamis_core::profiles::Profile;
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::write_atomic;

const FILE: &str = "desktop-policy.json";
const VERSION: u32 = 1;

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesktopPolicy {
    /// Pause file indexing (KDE Baloo, GNOME LocalSearch/Tracker) on
    /// battery. On by default: fully reversible, and the indexer catches
    /// up when plugged in.
    #[serde(default = "yes")]
    pub pause_indexing: bool,
    /// A fast screen (above 60 Hz) runs at 60 Hz on battery.
    #[serde(default)]
    pub lower_refresh: bool,
    /// The dark style on battery (saves power on OLED screens only).
    #[serde(default)]
    pub dark_style: bool,
    /// Seconds of no use before the screen turns off, on battery and
    /// plugged in (decision 0029). `None` leaves the desktop's own time.
    #[serde(default)]
    pub screen_off_battery_s: Option<u32>,
    #[serde(default)]
    pub screen_off_ac_s: Option<u32>,
    /// Animated dim: the screen fades to 10% over the last 15 seconds
    /// before it turns off, and brightens quickly from 10% when used again.
    /// On by default; the desktop's own dimming is switched off meanwhile.
    #[serde(default = "yes")]
    pub animated_dim: bool,
}

/// The screen-off times offered, in seconds (30 s to 1 hour).
pub const SCREEN_OFF_MIN_S: u32 = 30;
pub const SCREEN_OFF_MAX_S: u32 = 3600;

impl Default for DesktopPolicy {
    fn default() -> Self {
        DesktopPolicy {
            pause_indexing: true,
            lower_refresh: false,
            dark_style: false,
            screen_off_battery_s: None,
            screen_off_ac_s: None,
            animated_dim: true,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    v: u32,
    #[serde(flatten)]
    policy: DesktopPolicy,
}

/// What the agent found this desktop can do. Every field optional: an older
/// or other agent may send less.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Found {
    /// `kde`, `gnome` or `other`.
    #[serde(default)]
    pub desktop: Option<String>,
    /// `baloo`, `localsearch` or null.
    #[serde(default)]
    pub indexer: Option<String>,
    /// The built-in screen's refresh rate now and the slowest it offers at
    /// 60 Hz or more, in Hz.
    #[serde(default)]
    pub refresh_hz: Option<f64>,
    #[serde(default)]
    pub low_hz: Option<f64>,
    /// Whether the agent can change the style here.
    #[serde(default)]
    pub can_style: bool,
}

#[derive(Default)]
pub struct DesktopState {
    pub policy: DesktopPolicy,
    pub found: Option<Found>,
    oled: bool,
}

impl DesktopState {
    pub fn load(dir: &Path, root: &SysRoot) -> Self {
        let policy = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
            .map(|s| s.policy)
            .unwrap_or_default();
        DesktopState {
            policy,
            found: None,
            oled: dynamis_core::edid::builtin_oled(root),
        }
    }

    pub fn set_policy(&mut self, p: DesktopPolicy, dir: &Path) -> Result<(), String> {
        for s in [p.screen_off_battery_s, p.screen_off_ac_s]
            .into_iter()
            .flatten()
        {
            if !(SCREEN_OFF_MIN_S..=SCREEN_OFF_MAX_S).contains(&s) {
                return Err(format!(
                    "the screen-off time must be between {SCREEN_OFF_MIN_S} seconds and an hour"
                ));
            }
        }
        write_atomic(
            &dir.join(FILE),
            &serde_json::to_string_pretty(&Saved {
                v: VERSION,
                policy: p.clone(),
            })
            .map_err(|e| e.to_string())?,
        )?;
        self.policy = p;
        Ok(())
    }

    /// Restore: the person's choices go back to the defaults; the agent
    /// puts the desktop back when it next looks. Restore puts everything
    /// back as it was before Dynamis Power, so animated dim is off too.
    pub fn clear(&mut self, dir: &Path) {
        self.policy = DesktopPolicy {
            pause_indexing: false,
            animated_dim: false,
            ..DesktopPolicy::default()
        };
        let _ = self.set_policy(self.policy.clone(), dir);
    }

    /// What the agent should do now. Nothing on mains; in Faster the screen
    /// keeps its full refresh rate. Nothing in safe mode.
    pub fn want(&self, on_ac: bool, profile: Profile, safe_mode: bool) -> Value {
        let battery = !on_ac && !safe_mode;
        json!({
            "pause_indexing": battery && self.policy.pause_indexing,
            "lower_refresh": battery && self.policy.lower_refresh && profile != Profile::Performance,
            "dark_style": battery && self.policy.dark_style,
            // The person's own screen settings apply in safe mode too: they
            // are the desktop's, not tuning.
            "screen_off_battery_s": self.policy.screen_off_battery_s,
            "screen_off_ac_s": self.policy.screen_off_ac_s,
            "animated_dim": self.policy.animated_dim,
            "on_ac": on_ac,
        })
    }

    pub fn json(&self, on_ac: bool, profile: Profile, safe_mode: bool) -> Value {
        json!({
            "policy": self.policy,
            "want": self.want(on_ac, profile, safe_mode),
            // Null until the session agent has reported (it is not running).
            "found": self.found,
            "oled": self.oled,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wants_only_on_battery_and_keeps_full_speed_in_faster() {
        let dir = std::env::temp_dir().join(format!("dynamisd-desktop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = SysRoot::at(dir.join("root"));
        let mut d = DesktopState::load(&dir, &root);
        assert!(d.policy.pause_indexing, "on by default");
        assert_eq!(
            d.want(true, Profile::Balanced, false)["pause_indexing"],
            false
        );
        assert_eq!(
            d.want(false, Profile::Balanced, false)["pause_indexing"],
            true
        );
        assert_eq!(
            d.want(false, Profile::Balanced, true)["pause_indexing"],
            false,
            "safe mode"
        );
        d.set_policy(
            DesktopPolicy {
                lower_refresh: true,
                ..DesktopPolicy::default()
            },
            &dir,
        )
        .unwrap();
        assert_eq!(
            d.want(false, Profile::PowerSaver, false)["lower_refresh"],
            true
        );
        assert_eq!(
            d.want(false, Profile::Performance, false)["lower_refresh"],
            false
        );
        assert_eq!(
            d.want(true, Profile::Balanced, false)["animated_dim"],
            true,
            "on by default"
        );
        assert!(d
            .set_policy(
                DesktopPolicy {
                    screen_off_battery_s: Some(5),
                    ..DesktopPolicy::default()
                },
                &dir,
            )
            .is_err());
        let again = DesktopState::load(&dir, &root);
        assert!(again.policy.lower_refresh, "saved");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
