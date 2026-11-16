//! Read-only summaries for front ends (the app, `dynamis --json`): what each
//! setting is, what it is now, and whether anything restricts it.

use serde::Serialize;

use crate::engine::Engine;
use crate::fs::SysRoot;
use crate::settings::{self, Probe, RiskTier};

#[derive(Debug, Clone, Serialize)]
pub struct SettingRow {
    pub id: &'static str,
    pub label: &'static str,
    pub risk: RiskTier,
    pub risk_label: &'static str,
    pub available: bool,
    /// Accepted values, or why the setting is not available.
    pub allowed: String,
    pub current: Option<String>,
    /// Value before Dynamis Power first changed it.
    pub original: Option<String>,
    pub blocked: Option<String>,
    /// Values a quirk forbids here, with the reason.
    pub not_allowed: Vec<(Vec<String>, String)>,
}

pub fn settings(root: &SysRoot, engine: &Engine) -> Vec<SettingRow> {
    let blocked = engine.blocked();
    let baseline = engine.baseline();
    settings::all()
        .iter()
        .map(|s| {
            let (available, allowed) = match s.probe(root) {
                Probe::Supported { allowed } => (true, allowed.describe()),
                Probe::Unsupported { reason } => (false, reason),
            };
            SettingRow {
                id: s.id(),
                label: s.label(),
                risk: s.risk(),
                risk_label: s.risk().label(),
                available,
                allowed,
                current: s.read(root).ok(),
                original: baseline.get(s.id()).cloned(),
                blocked: blocked.get(s.id()).cloned(),
                not_allowed: engine
                    .quirks()
                    .quirks
                    .iter()
                    .filter(|q| q.setting == s.id())
                    .map(|q| (q.values.clone(), q.reason.clone()))
                    .collect(),
            }
        })
        .collect()
}
