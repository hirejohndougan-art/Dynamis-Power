//! Setting plugins: one per lever.
//!
//! A plugin only *describes* a setting: how to probe it, read it, check a
//! value, and which kernel files a change would write. It never writes
//! anything itself. All writes go through [`crate::engine::Engine`], which
//! journals first and verifies afterwards (see `docs/SAFETY.md`).
//!
//! Setting IDs are permanent. See `docs/SETTINGS.md`.

mod plugins;

use serde::Serialize;

use crate::fs::{ReadResult, SysRoot};

pub use plugins::all;

/// How a setting is handled by default. See `docs/SAFETY.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    /// Cannot cause data loss or instability.
    Safe,
    /// Fine on most hardware, with known exceptions.
    UsuallySafe,
    /// Can misbehave on some hardware; needs an explicit opt-in.
    Risky,
    /// Benefit unproven; needs an explicit opt-in.
    Experimental,
    /// Never applied automatically (needs a reboot or another program).
    Advisory,
}

impl RiskTier {
    pub fn label(self) -> &'static str {
        match self {
            RiskTier::Safe => "Safe",
            RiskTier::UsuallySafe => "Usually safe",
            RiskTier::Risky => "Risky",
            RiskTier::Experimental => "Experimental",
            RiskTier::Advisory => "Advisory",
        }
    }

    /// Whether a change at this tier needs an explicit opt-in.
    pub fn needs_opt_in(self) -> bool {
        matches!(self, RiskTier::Risky | RiskTier::Experimental)
    }
}

/// The values a setting accepts on this machine.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Allowed {
    /// One of a fixed list of words.
    Choices { values: Vec<String> },
    /// A whole number within an inclusive range.
    Range {
        min: i64,
        max: i64,
        unit: &'static str,
    },
    /// "on" or "off".
    OnOff,
}

impl Allowed {
    /// Check a requested value. Returns the value in canonical form.
    pub fn check(&self, value: &str) -> Result<String, String> {
        let v = value.trim();
        match self {
            Allowed::Choices { values } => values
                .iter()
                .find(|c| c.as_str() == v)
                .cloned()
                .ok_or_else(|| format!("'{v}' is not one of: {}", values.join(", "))),
            Allowed::Range { min, max, unit } => {
                let n: i64 = v
                    .parse()
                    .map_err(|_| format!("'{v}' is not a whole number"))?;
                if n < *min || n > *max {
                    Err(format!("{n} is outside {min}–{max}{unit}"))
                } else {
                    Ok(n.to_string())
                }
            }
            Allowed::OnOff => match v.to_ascii_lowercase().as_str() {
                "on" | "1" | "true" | "yes" => Ok("on".into()),
                "off" | "0" | "false" | "no" => Ok("off".into()),
                _ => Err(format!("'{v}' should be on or off")),
            },
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Allowed::Choices { values } => values.join(" | "),
            Allowed::Range { min, max, unit } => format!("{min}–{max}{unit}"),
            Allowed::OnOff => "on | off".into(),
        }
    }
}

/// Result of probing a setting on this machine.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Probe {
    Supported { allowed: Allowed },
    Unsupported { reason: String },
}

/// One kernel file write that a change consists of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Write {
    pub path: String,
    pub contents: String,
}

pub trait Setting: Sync {
    /// Permanent ID from `docs/SETTINGS.md`.
    fn id(&self) -> &'static str;
    /// Plain-language name for people.
    fn label(&self) -> &'static str;
    fn risk(&self) -> RiskTier;
    /// Whether this machine offers the setting, and which values it accepts.
    fn probe(&self, root: &SysRoot) -> Probe;
    /// Current value, in the same form [`Allowed::check`] returns. Settings
    /// spread over several files (one per CPU, say) return `mixed` when they
    /// disagree.
    fn read(&self, root: &SysRoot) -> ReadResult<String>;
    /// The kernel writes that would set `value`. `value` has already passed
    /// [`Allowed::check`]. Must not write anything.
    fn plan(&self, root: &SysRoot, value: &str) -> Result<Vec<Write>, String>;
}

/// Find a plugin by its ID.
pub fn by_id(id: &str) -> Option<&'static dyn Setting> {
    all().iter().copied().find(|s| s.id() == id)
}
