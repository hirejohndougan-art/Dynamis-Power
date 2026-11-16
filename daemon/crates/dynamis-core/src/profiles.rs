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
            ],
            Profile::Balanced => &[
                ("platform.profile", &["balanced"]),
                ("cpu.epp", &["balance_performance"]),
            ],
            Profile::Performance => &[
                ("platform.profile", &["performance"]),
                ("cpu.epp", &["performance"]),
            ],
        }
    }
}

/// One setting value a profile would apply on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub id: &'static str,
    pub value: String,
}

/// What `profile` would change on this machine. Empty if it changes nothing.
pub fn plan(root: &SysRoot, profile: Profile) -> Vec<Step> {
    let mut steps = Vec::new();
    for (id, wanted) in profile.wishes() {
        let Some(s) = settings::by_id(id) else {
            continue;
        };
        let Probe::Supported {
            allowed: Allowed::Choices { values },
        } = s.probe(root)
        else {
            continue;
        };
        if let Some(v) = wanted.iter().find(|w| values.iter().any(|v| v == *w)) {
            steps.push(Step {
                id,
                value: (*v).to_string(),
            });
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
    let cpu = all.iter().any(|s| s.id == "cpu.epp").then(|| {
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
    plan(root, profile)
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
