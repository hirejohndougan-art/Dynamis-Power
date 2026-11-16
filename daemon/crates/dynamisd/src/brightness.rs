//! The screen brightness the person chose for each mode.
//!
//! Off until the person picks a level for a mode. Then, each time the mode
//! changes to one with a level (by hand, by auto mode, by an app asking or
//! by "Last until"), the screen is set to that level once. Between mode
//! changes the person's own brightness keys win: nothing is set again until
//! the mode changes, so this never fights them. Choosing a level for the
//! mode in use sets it at once, so the person sees what they picked.
//!
//! Writes go through the engine (`Engine::set_brightness`): journaled,
//! refused in safe mode, logged only in dry run. Needs no readings of its
//! own, so it adds no wakeups.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use dynamis_core::profiles::Profile;
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::{open_engine, write_atomic};

const FILE: &str = "brightness.json";
const VERSION: u32 = 1;
/// Lowest level offered: some screens go dark at 0.
pub const MIN_PCT: u8 = 5;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BrightnessPrefs {
    /// Brightness in percent for each mode; a mode not listed is left as it
    /// is.
    #[serde(default)]
    pub levels: BTreeMap<Profile, u8>,
}

impl BrightnessPrefs {
    fn check(&self) -> Result<(), String> {
        match self.levels.values().find(|v| !(MIN_PCT..=100).contains(*v)) {
            Some(v) => Err(format!("brightness {v}% is outside {MIN_PCT}–100%")),
            None => Ok(()),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    v: u32,
    #[serde(flatten)]
    prefs: BrightnessPrefs,
}

#[derive(Default)]
pub struct BrightnessState {
    pub prefs: BrightnessPrefs,
    /// The mode at the last look. Behind a lock because the mode is applied
    /// from `&Core`.
    last: Mutex<Option<Profile>>,
}

impl BrightnessState {
    /// Load the choices. Call [`BrightnessState::prime`] with the mode in
    /// force before the first [`BrightnessState::follow`].
    pub fn load(dir: &Path) -> BrightnessState {
        let prefs = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
            .map(|s| s.prefs)
            .unwrap_or_default();
        BrightnessState {
            prefs,
            last: Mutex::new(None),
        }
    }

    /// The mode in force without a change: nothing is set for it.
    pub fn prime(&self, active: Profile) {
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(active);
    }

    /// The mode may have changed: if it did and the new mode has a level,
    /// set it. Cheap when nothing changed (no reading at all).
    pub fn follow(&self, root: &SysRoot, dir: &Path, active: Profile) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if *last == Some(active) {
            return;
        }
        *last = Some(active);
        if let Some(&pct) = self.prefs.levels.get(&active) {
            set(
                root,
                dir,
                pct,
                &format!("your brightness for {}", name(active)),
            );
        }
    }

    pub fn set_prefs(
        &mut self,
        p: BrightnessPrefs,
        root: &SysRoot,
        dir: &Path,
        active: Profile,
    ) -> Result<(), String> {
        p.check()?;
        let changed_now = p.levels.get(&active) != self.prefs.levels.get(&active);
        write_atomic(
            &dir.join(FILE),
            &serde_json::to_string_pretty(&Saved {
                v: VERSION,
                prefs: p.clone(),
            })
            .map_err(|e| e.to_string())?,
        )?;
        self.prefs = p;
        if changed_now {
            if let Some(&pct) = self.prefs.levels.get(&active) {
                set(
                    root,
                    dir,
                    pct,
                    &format!("you chose it for {}", name(active)),
                );
            }
        }
        Ok(())
    }

    /// Forget every choice (Restore). The screen stays as it is.
    pub fn clear(&mut self, dir: &Path) {
        self.prefs = BrightnessPrefs::default();
        let _ = std::fs::remove_file(dir.join(FILE));
    }

    pub fn json(&self, root: &SysRoot) -> Value {
        let b = dynamis_core::sensors::backlight::read(root);
        json!({
            "supported": b.is_some(),
            "now_pct": b.map(|b| b.percent),
            "min_pct": MIN_PCT,
            "levels": self.prefs.levels,
        })
    }
}

fn name(p: Profile) -> &'static str {
    match p {
        Profile::PowerSaver => "Longer battery",
        Profile::Balanced => "Balanced",
        Profile::Performance => "Faster",
    }
}

fn set(root: &SysRoot, dir: &Path, pct: u8, reason: &str) {
    match open_engine(root, dir).map(|mut e| e.set_brightness(f64::from(pct), reason)) {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => eprintln!("dynamisd: brightness: {e}"),
        Err(e) => eprintln!("dynamisd: brightness: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> (std::path::PathBuf, SysRoot) {
        let dir =
            std::env::temp_dir().join(format!("dynamis-bright-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bl = dir.join("root/sys/class/backlight/intel_backlight");
        std::fs::create_dir_all(&bl).unwrap();
        std::fs::write(bl.join("brightness"), "500\n").unwrap();
        std::fs::write(bl.join("max_brightness"), "1000\n").unwrap();
        std::fs::create_dir_all(dir.join("state")).unwrap();
        (dir.clone(), SysRoot::at(dir.join("root")))
    }

    fn level(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("root/sys/class/backlight/intel_backlight/brightness"))
            .unwrap()
            .trim()
            .to_string()
    }

    #[test]
    fn set_once_per_mode_change_and_never_fights() {
        let (dir, root) = sandbox("follow");
        let state = dir.join("state");
        let mut b = BrightnessState::load(&state);
        b.prime(Profile::Balanced);
        let mut p = BrightnessPrefs::default();
        p.levels.insert(Profile::PowerSaver, 30);
        p.levels.insert(Profile::Balanced, 70);
        // Choosing a level for the mode in use sets it at once.
        b.set_prefs(p, &root, &state, Profile::Balanced).unwrap();
        assert_eq!(level(&dir), "700");
        // The person dims by hand; applying Balanced again changes nothing.
        std::fs::write(
            dir.join("root/sys/class/backlight/intel_backlight/brightness"),
            "400",
        )
        .unwrap();
        b.follow(&root, &state, Profile::Balanced);
        assert_eq!(level(&dir), "400");
        // A mode change sets that mode's level once.
        b.follow(&root, &state, Profile::PowerSaver);
        assert_eq!(level(&dir), "300");
        // A mode without a level leaves the screen alone.
        b.follow(&root, &state, Profile::Performance);
        assert_eq!(level(&dir), "300");
        // Saved and loaded again; a restart sets nothing.
        let b2 = BrightnessState::load(&state);
        b2.prime(Profile::PowerSaver);
        assert_eq!(b2.prefs.levels[&Profile::Balanced], 70);
        b2.follow(&root, &state, Profile::PowerSaver);
        assert_eq!(level(&dir), "300");
        // Out of range is refused.
        let mut bad = BrightnessPrefs::default();
        bad.levels.insert(Profile::Balanced, 2);
        assert!(b.set_prefs(bad, &root, &state, Profile::Balanced).is_err());
        b.clear(&state);
        assert!(b.prefs.levels.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
