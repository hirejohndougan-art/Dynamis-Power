//! The daemon's state, with no D-Bus in it so it can be tested directly.
//!
//! - `chosen` is the profile the user picked (saved across reboots).
//! - Holds are temporary requests from apps ("performance while I compile",
//!   "power-saver while the battery is low"). A power-saver hold wins over a
//!   performance hold, and any hold wins over the user's choice.
//! - The active profile is what is applied to the machine.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use dynamis_core::engine::{Engine, EngineError};
use dynamis_core::profiles::{self, Profile};
use dynamis_core::SysRoot;
use serde::{Deserialize, Serialize};

/// Version of `profile.json`.
const PROFILE_FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct Hold {
    pub profile: Profile,
    pub reason: String,
    pub application_id: String,
    /// D-Bus unique name of the caller; the hold ends when it disconnects.
    pub owner: String,
}

pub struct Core {
    root: SysRoot,
    state_dir: PathBuf,
    available: Vec<Profile>,
    chosen: Profile,
    holds: BTreeMap<u32, Hold>,
    next_cookie: u32,
}

#[derive(Serialize, Deserialize)]
struct SavedProfile {
    v: u32,
    profile: String,
}

impl Core {
    /// Load state and recover from anything an earlier run left unfinished.
    /// Does not apply anything yet; call [`Core::apply_active`] for that.
    pub fn new(root: SysRoot, state_dir: PathBuf) -> Core {
        let available = profiles::available(&root);
        let saved = fs::read_to_string(state_dir.join("profile.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<SavedProfile>(&t).ok())
            .and_then(|s| Profile::parse(&s.profile))
            .filter(|p| available.contains(p));
        Core {
            root,
            state_dir,
            available,
            chosen: saved.unwrap_or(Profile::Balanced),
            holds: BTreeMap::new(),
            next_cookie: 1,
        }
    }

    pub fn available(&self) -> &[Profile] {
        &self.available
    }

    pub fn root(&self) -> &SysRoot {
        &self.root
    }

    pub fn holds(&self) -> &BTreeMap<u32, Hold> {
        &self.holds
    }

    pub fn active(&self) -> Profile {
        let held = |p| self.holds.values().any(|h| h.profile == p);
        if held(Profile::PowerSaver) {
            Profile::PowerSaver
        } else if held(Profile::Performance) {
            Profile::Performance
        } else {
            self.chosen
        }
    }

    /// The user picked a profile. Releases every hold (as the standard
    /// interface requires) and returns their cookies.
    pub fn choose(&mut self, p: Profile) -> Result<Vec<u32>, String> {
        if !self.available.contains(&p) {
            return Err(format!("'{}' is not available on this machine", p.as_str()));
        }
        let before = (self.chosen, std::mem::take(&mut self.holds));
        self.chosen = p;
        if let Err(e) = self.apply_active(&format!("profile {} chosen", p.as_str())) {
            // Nothing applied: keep the old state so what we report stays true.
            (self.chosen, self.holds) = before;
            return Err(e);
        }
        self.save()?;
        Ok(before.1.into_keys().collect())
    }

    pub fn hold(
        &mut self,
        p: Profile,
        reason: &str,
        app: &str,
        owner: &str,
    ) -> Result<u32, String> {
        if p == Profile::Balanced {
            return Err("only power-saver and performance can be held".into());
        }
        if !self.available.contains(&p) {
            return Err(format!("'{}' is not available on this machine", p.as_str()));
        }
        let cookie = self.next_cookie;
        self.next_cookie = self.next_cookie.wrapping_add(1).max(1);
        self.holds.insert(
            cookie,
            Hold {
                profile: p,
                reason: reason.into(),
                application_id: app.into(),
                owner: owner.into(),
            },
        );
        // A failed apply still records the hold: the app asked, and the next
        // successful apply honours it. The error is logged.
        if let Err(e) = self.apply_active(&format!("{app} asked for {}: {reason}", p.as_str())) {
            eprintln!("dynamisd: hold {cookie}: {e}");
        }
        Ok(cookie)
    }

    pub fn release(&mut self, cookie: u32, caller: &str) -> Result<(), String> {
        match self.holds.get(&cookie) {
            None => Err(format!("no hold with cookie {cookie}")),
            Some(h) if h.owner != caller => {
                Err("only the app that made a hold can release it".into())
            }
            Some(_) => {
                self.holds.remove(&cookie);
                self.reapply_after_release();
                Ok(())
            }
        }
    }

    /// An app left the bus: drop its holds. Returns their cookies.
    pub fn release_owner(&mut self, owner: &str) -> Vec<u32> {
        let gone: Vec<u32> = self
            .holds
            .iter()
            .filter(|(_, h)| h.owner == owner)
            .map(|(c, _)| *c)
            .collect();
        if !gone.is_empty() {
            self.holds.retain(|_, h| h.owner != owner);
            self.reapply_after_release();
        }
        gone
    }

    fn reapply_after_release(&mut self) {
        if let Err(e) = self.apply_active("hold released") {
            eprintln!("dynamisd: {e}");
        }
    }

    /// Apply the active profile. Settings already at the right value are
    /// left untouched (the engine reads first and writes nothing), so this is
    /// cheap to repeat and stays correct after `dynamis restore`. Fails without
    /// changing anything in safe mode, or when every step fails.
    pub fn apply_active(&self, reason: &str) -> Result<(), String> {
        let target = self.active();
        let mut engine = open_engine(&self.root, &self.state_dir)?;
        if let Some(why) = engine.safe_mode() {
            return Err(format!("safe mode is on ({why}); profiles are not applied"));
        }
        let results = profiles::apply(&mut engine, &self.root, target, reason);
        let failures: Vec<String> = results
            .iter()
            .filter_map(|a| {
                a.result
                    .as_ref()
                    .err()
                    .map(|e| format!("{}: {e}", a.step.id))
            })
            .collect();
        for f in &failures {
            eprintln!("dynamisd: {} profile, {f}", target.as_str());
        }
        if !results.is_empty() && failures.len() == results.len() {
            return Err(format!(
                "could not apply {}: {}",
                target.as_str(),
                failures.join("; ")
            ));
        }
        Ok(())
    }

    fn save(&self) -> Result<(), String> {
        let path = self.state_dir.join("profile.json");
        let tmp = path.with_extension("tmp");
        let text = serde_json::to_string(&SavedProfile {
            v: PROFILE_FILE_VERSION,
            profile: self.chosen.as_str().into(),
        })
        .map_err(|e| e.to_string())?;
        let write = || -> std::io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            fs::rename(&tmp, &path)
        };
        write().map_err(|e| format!("could not save the chosen profile: {e}"))
    }
}

/// Open the engine, waiting briefly if `dynamis` on the command line is
/// changing a setting at the same moment.
pub fn open_engine(root: &SysRoot, state_dir: &Path) -> Result<Engine, String> {
    for _ in 0..40 {
        match Engine::open(root.clone(), state_dir) {
            Ok(engine) => {
                if let Some(r) = engine.recovered() {
                    eprintln!(
                        "dynamisd: a change to {} did not finish last time; put back to {} ({}) and blocked",
                        r.id,
                        r.restored_to,
                        if r.ok { "ok" } else { "FAILED" }
                    );
                }
                return Ok(engine);
            }
            Err(EngineError::Locked(_)) => sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("another Dynamis Power process kept the settings lock for 2 seconds".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A copy of the fixture with a firmware profile added, so every profile
    /// changes something.
    fn sandbox(name: &str) -> (PathBuf, Core) {
        let dir = std::env::temp_dir().join(format!("dynamisd-core-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/sysfs/synthetic-laptop");
        let ok = std::process::Command::new("cp")
            .arg("-a")
            .arg(&src)
            .arg(dir.join("root"))
            .status()
            .unwrap()
            .success();
        assert!(ok, "copying the fixture failed");
        let core = Core::new(SysRoot::at(dir.join("root")), dir.join("state"));
        (dir, core)
    }

    fn epp(dir: &Path) -> String {
        fs::read_to_string(
            dir.join("root/sys/devices/system/cpu/cpufreq/policy0/energy_performance_preference"),
        )
        .unwrap()
        .trim()
        .to_string()
    }

    #[test]
    fn choose_hold_release() {
        let (dir, mut c) = sandbox("chr");
        assert_eq!(c.active(), Profile::Balanced);

        assert!(c.choose(Profile::PowerSaver).unwrap().is_empty());
        assert_eq!(epp(&dir), "power");

        // Any hold outranks the user's choice.
        let perf = c
            .hold(Profile::Performance, "compiling", "ide", ":1.5")
            .unwrap();
        assert_eq!(c.active(), Profile::Performance);
        assert_eq!(epp(&dir), "performance");

        // A power-saver hold outranks a performance hold.
        let saver = c
            .hold(Profile::PowerSaver, "battery low", "shell", ":1.9")
            .unwrap();
        assert_eq!(c.active(), Profile::PowerSaver);

        assert!(
            c.release(saver, ":1.5").is_err(),
            "only the owner may release"
        );
        c.release(saver, ":1.9").unwrap();
        assert_eq!(c.active(), Profile::Performance);

        // The app disconnects: its hold ends and the user's choice returns.
        assert_eq!(c.release_owner(":1.5"), vec![perf]);
        assert_eq!(c.active(), Profile::PowerSaver);
        assert_eq!(epp(&dir), "power");

        assert!(c.hold(Profile::Balanced, "", "x", ":1.1").is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn choosing_releases_holds_and_is_saved() {
        let (dir, mut c) = sandbox("save");
        let h = c
            .hold(Profile::Performance, "game", "steam", ":1.2")
            .unwrap();
        assert_eq!(c.choose(Profile::PowerSaver).unwrap(), vec![h]);
        assert!(c.holds().is_empty());

        let again = Core::new(SysRoot::at(dir.join("root")), dir.join("state"));
        assert_eq!(again.active(), Profile::PowerSaver);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn safe_mode_refuses_and_keeps_state() {
        let (dir, mut c) = sandbox("safe");
        fs::create_dir_all(dir.join("root/etc/dynamis-power")).unwrap();
        fs::write(dir.join("root/etc/dynamis-power/disable"), "").unwrap();
        assert!(c
            .choose(Profile::Performance)
            .unwrap_err()
            .contains("safe mode"));
        assert_eq!(c.active(), Profile::Balanced);
        assert_eq!(epp(&dir), "balance_performance");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reapplies_after_an_outside_restore() {
        let (dir, mut c) = sandbox("restore");
        c.choose(Profile::Performance).unwrap();
        open_engine(c.root(), &dir.join("state"))
            .unwrap()
            .restore()
            .unwrap();
        assert_eq!(epp(&dir), "balance_performance");
        c.choose(Profile::Performance).unwrap();
        assert_eq!(epp(&dir), "performance");
        fs::remove_dir_all(&dir).ok();
    }
}
