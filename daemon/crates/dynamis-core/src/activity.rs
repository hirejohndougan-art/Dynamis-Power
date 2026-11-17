//! Learning per activity (decision 0026).
//!
//! The settings whose cost is speed (turbo and the speed limit, tuned
//! together, and the graphics speed limits) are learned separately for
//! everyday use (idle and light use, which tests already move between),
//! for video, and, only when the person allows it, for heavy work. Every
//! other setting keeps one result for all activities.
//!
//! During calls and games, and during heavy work unless learning for it is
//! on, speed-related settings use the power mode's own values: learning
//! never slows real work it has not measured.

use serde::{Deserialize, Serialize};

use crate::auto::Workload;
use crate::calibrate::Record;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Idle and light use: the quiet moments where tests have always run.
    Everyday,
    Video,
    Heavy,
}

impl Activity {
    /// The activity for a kind of use; `None` for calls and games, which
    /// learning leaves alone.
    pub fn of(w: Workload) -> Option<Activity> {
        match w {
            Workload::Idle
            | Workload::LightInteractive
            | Workload::BurstyInteractive
            | Workload::BackgroundTransfer => Some(Activity::Everyday),
            Workload::Video => Some(Activity::Video),
            Workload::SustainedCompute => Some(Activity::Heavy),
            Workload::Call | Workload::Gaming => None,
        }
    }

    /// The activity a result was measured in (older results: everyday).
    pub fn of_record(r: &Record) -> Activity {
        Activity::of(r.context.workload).unwrap_or(Activity::Everyday)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Activity::Everyday => "everyday",
            Activity::Video => "video",
            Activity::Heavy => "heavy",
        }
    }
}

/// Graphics speed settings learned per activity: for everyday use and for
/// video, judged by whether the graphics chip keeps up
/// ([`crate::sensors::gpu`]); heavy work keeps the driver's own speed.
pub const GRAPHICS: &[&str] = &["gpu.max_freq", "gpu.amd_power_level"];

/// Whether a setting is learned per activity.
pub fn per_activity(id: &str) -> bool {
    crate::tune::in_group(id) || GRAPHICS.contains(&id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activities() {
        assert_eq!(Activity::of(Workload::Idle), Some(Activity::Everyday));
        assert_eq!(
            Activity::of(Workload::BurstyInteractive),
            Some(Activity::Everyday)
        );
        assert_eq!(Activity::of(Workload::Video), Some(Activity::Video));
        assert_eq!(
            Activity::of(Workload::SustainedCompute),
            Some(Activity::Heavy)
        );
        assert_eq!(Activity::of(Workload::Call), None);
        assert!(per_activity("cpu.turbo"));
        assert!(per_activity("gpu.max_freq"));
        assert!(!per_activity("wifi.power_save"));
    }
}
