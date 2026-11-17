//! The three standard power profiles: power-saver, balanced and performance.
//!
//! These are the "safe standard settings" Dynamis Power starts from, and what
//! the desktop's power slider switches between. Each profile is a small set of
//! setting values chosen from what this machine offers (its probe), applied one
//! at a time through [`Engine`], so every change is checked and journaled like
//! any other.
//!
//! Learned settings (phase 4 onwards) will build on top of these; nothing here
//! learns anything.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::engine::{Engine, EngineError, Request};
use crate::fs::SysRoot;
use crate::settings::{self, Allowed, Probe};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    PowerSaver,
    #[default]
    Balanced,
    Performance,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::PowerSaver, Profile::Balanced, Profile::Performance];

    /// The name used on D-Bus and in files. These exact strings are part of
    /// the standard power-profiles interface.
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::PowerSaver => "power-saver",
            Profile::Balanced => "balanced",
            Profile::Performance => "performance",
        }
    }

    pub fn parse(s: &str) -> Option<Profile> {
        Profile::ALL.into_iter().find(|p| p.as_str() == s)
    }

    /// Kernel values to try for each setting, in order of preference. The
    /// first one this machine offers is used; if none is offered the setting
    /// is left alone for this profile.
    fn wishes(self) -> &'static [(&'static str, &'static [&'static str])] {
        match self {
            Profile::PowerSaver => &[
                ("platform.profile", &["low-power", "quiet", "cool"]),
                ("cpu.epp", &["power", "balance_power"]),
                ("display.amd_panel_power_savings", &["3"]),
            ],
            Profile::Balanced => &[
                ("platform.profile", &["balanced"]),
                ("cpu.epp", &["balance_performance"]),
                ("display.amd_panel_power_savings", &["0"]),
            ],
            Profile::Performance => &[
                ("platform.profile", &["performance"]),
                ("cpu.epp", &["performance"]),
                ("display.amd_panel_power_savings", &["0"]),
            ],
        }
    }

    /// For laptops with neither a speed preference nor a firmware power mode
    /// (budget and older processors): turbo, the speed limit or top
    /// frequency, and the governor. `pct:N` means the lowest frequency the
    /// driver lists that is at least N% of the top one.
    fn fallback(self) -> &'static [(&'static str, &'static [&'static str])] {
        // Every mode sets every lever, so switching modes always undoes the
        // last one's choices.
        const CALM: &[&str] = &["powersave", "schedutil", "ondemand", "conservative"];
        match self {
            Profile::PowerSaver => &[
                ("cpu.turbo", &["off"]),
                ("cpu.max_perf_pct", &["60"]),
                ("cpu.max_freq", &["pct:60"]),
                ("cpu.governor", CALM),
                ("display.amd_panel_power_savings", &["3"]),
            ],
            Profile::Balanced => &[
                ("cpu.turbo", &["on"]),
                ("cpu.max_perf_pct", &["100"]),
                ("cpu.max_freq", &["pct:100"]),
                ("cpu.governor", CALM),
                ("display.amd_panel_power_savings", &["0"]),
            ],
            Profile::Performance => &[
                ("cpu.turbo", &["on"]),
                ("cpu.max_perf_pct", &["100"]),
                ("cpu.max_freq", &["pct:100"]),
                ("cpu.governor", &["performance"]),
                ("display.amd_panel_power_savings", &["0"]),
            ],
        }
    }

    /// The settings this profile uses on this machine: the usual two, or the
    /// fallback when neither exists.
    fn levers(self, root: &SysRoot) -> &'static [(&'static str, &'static [&'static str])] {
        if uses_fallback(root) {
            self.fallback()
        } else {
            self.wishes()
        }
    }
}

/// Whether this machine lacks both usual mode controls (the speed
/// preference and the firmware power mode), so modes use the fallback.
pub fn uses_fallback(root: &SysRoot) -> bool {
    ["cpu.epp", "platform.profile"].iter().all(|id| {
        settings::by_id(id).is_none_or(|s| !matches!(s.probe(root), Probe::Supported { .. }))
    })
}

/// Why a mode is not offered here, for the app to say so.
pub fn missing(root: &SysRoot) -> Vec<(Profile, &'static str)> {
    let offered = available(root);
    Profile::ALL
        .into_iter()
        .filter(|p| !offered.contains(p))
        .map(|p| {
            (
                p,
                if plan(root, p).is_empty() {
                    "this laptop has no controls for it"
                } else {
                    "on this laptop it would be the same as Balanced"
                },
            )
        })
        .collect()
}

/// The values a setting offers, as a list (ranges become steps of 10).
fn offered(root: &SysRoot, id: &str) -> Option<Vec<String>> {
    let Probe::Supported { allowed } = settings::by_id(id)?.probe(root) else {
        return None;
    };
    Some(match allowed {
        Allowed::Choices { values } => values,
        Allowed::OnOff => vec!["on".into(), "off".into()],
        Allowed::Range { min, max, .. } => {
            let mut v: Vec<String> = (0..=10)
                .map(|i| i * 10)
                .filter(|n| *n >= min && *n <= max)
                .map(|n| n.to_string())
                .collect();
            if v.is_empty() {
                v.push(max.to_string());
            }
            v
        }
    })
}

/// The first wish this machine accepts.
fn pick(root: &SysRoot, id: &str, wanted: &[&str]) -> Option<String> {
    let Probe::Supported { allowed } = settings::by_id(id)?.probe(root) else {
        return None;
    };
    for w in wanted {
        if let Some(pct) = w.strip_prefix("pct:").and_then(|p| p.parse::<u64>().ok()) {
            // Numeric choices only: the lowest one at least pct% of the top.
            let Allowed::Choices { values } = &allowed else {
                continue;
            };
            let mut nums: Vec<u64> = values.iter().filter_map(|v| v.parse().ok()).collect();
            nums.sort_unstable();
            let Some(top) = nums.last().copied() else {
                continue;
            };
            if let Some(n) = nums.iter().find(|n| **n * 100 >= top * pct) {
                return Some(n.to_string());
            }
            continue;
        }
        if let Ok(v) = allowed.check(w) {
            return Some(v);
        }
    }
    None
}

/// A person's own choices for what a profile sets (Advanced → Profiles):
/// profile → setting → value. Only the profile's own settings, and only
/// values this machine offers; anything else is ignored when planning, so a
/// stale or hand-edited file can never make a profile write something odd.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Overrides {
    pub v: u32,
    #[serde(default)]
    pub profiles: BTreeMap<Profile, BTreeMap<String, String>>,
}

impl Overrides {
    pub const VERSION: u32 = 1;

    /// Check a set of overrides against this machine, with a sentence for
    /// the first problem.
    pub fn check(&self, root: &SysRoot) -> Result<(), String> {
        for (p, settings) in &self.profiles {
            for (id, value) in settings {
                let Some(choices) = choices(root, *p, id) else {
                    return Err(format!(
                        "{} does not set '{id}' on this machine",
                        p.as_str()
                    ));
                };
                if !choices.contains(value) {
                    return Err(format!(
                        "'{value}' is not offered for '{id}' (choices: {})",
                        choices.join(", ")
                    ));
                }
            }
        }
        Ok(())
    }
}

/// The settings a profile may set here, with the values offered for each.
pub fn editable(root: &SysRoot, profile: Profile) -> Vec<(&'static str, Vec<String>)> {
    profile
        .levers(root)
        .iter()
        .filter_map(|(id, _)| choices(root, profile, id).map(|c| (*id, c)))
        .collect()
}

fn choices(root: &SysRoot, profile: Profile, id: &str) -> Option<Vec<String>> {
    if !profile.levers(root).iter().any(|(w, _)| *w == id) {
        return None;
    }
    offered(root, id)
}

/// One setting value a profile would apply on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub id: &'static str,
    pub value: String,
}

/// What `profile` would change on this machine. Empty if it changes nothing.
pub fn plan(root: &SysRoot, profile: Profile) -> Vec<Step> {
    plan_with(root, profile, &Overrides::default())
}

/// [`plan`], with the person's own choices where they are valid here.
pub fn plan_with(root: &SysRoot, profile: Profile, ov: &Overrides) -> Vec<Step> {
    let mine = ov.profiles.get(&profile);
    let mut steps = Vec::new();
    for (id, wanted) in profile.levers(root) {
        let Some(values) = offered(root, id) else {
            continue;
        };
        if let Some(v) = mine.and_then(|m| m.get(*id)).filter(|v| values.contains(v)) {
            steps.push(Step {
                id,
                value: v.clone(),
            });
        } else if let Some(v) = pick(root, id, wanted) {
            steps.push(Step { id, value: v });
        }
    }
    steps
}

/// Profiles worth offering here. Balanced is always offered (it is the
/// default and "do nothing special" is a valid balanced). The others only
/// when they would actually change something, and performance only when
/// something differs from balanced.
pub fn available(root: &SysRoot) -> Vec<Profile> {
    let balanced = plan(root, Profile::Balanced);
    Profile::ALL
        .into_iter()
        .filter(|p| match p {
            Profile::Balanced => true,
            other => {
                let steps = plan(root, *other);
                !steps.is_empty() && steps != balanced
            }
        })
        .collect()
}

/// Which kinds of driver a profile uses here, for the `CpuDriver` and
/// `PlatformDriver` fields of the standard interface.
pub fn drivers(root: &SysRoot) -> (Option<String>, Option<String>) {
    let all: Vec<Step> = Profile::ALL
        .into_iter()
        .flat_map(|p| plan(root, p))
        .collect();
    let cpu = all.iter().any(|s| s.id.starts_with("cpu.")).then(|| {
        root.read_string("/sys/devices/system/cpu/cpufreq/policy0/scaling_driver")
            .unwrap_or_else(|_| "cpufreq".into())
    });
    let platform = all
        .iter()
        .any(|s| s.id == "platform.profile")
        .then(|| "platform_profile".to_string());
    (cpu, platform)
}

/// Result of applying one step.
#[derive(Debug)]
pub struct Applied {
    pub step: Step,
    pub result: Result<(), EngineError>,
}

/// Apply every step of `profile`, one change at a time. A step that fails
/// (blocked, a trial in progress, a write that did not stick) is reported and
/// the rest still run: half a profile is better than none, and each step is
/// independently journaled and restorable.
pub fn apply(engine: &mut Engine, root: &SysRoot, profile: Profile, reason: &str) -> Vec<Applied> {
    apply_with(engine, root, profile, &Overrides::default(), reason)
}

/// [`apply`], with the person's own choices.
pub fn apply_with(
    engine: &mut Engine,
    root: &SysRoot,
    profile: Profile,
    ov: &Overrides,
    reason: &str,
) -> Vec<Applied> {
    plan_with(root, profile, ov)
        .into_iter()
        .map(|step| {
            let result = engine
                .apply(&Request {
                    id: step.id,
                    value: &step.value,
                    allow_risky: false,
                    trial_secs: None,
                    reason,
                })
                .map(|_| ());
            Applied { step, result }
        })
        .collect()
}
