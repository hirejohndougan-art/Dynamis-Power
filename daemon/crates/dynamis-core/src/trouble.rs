//! "Something stopped working" (everyday problem §1): which settings could
//! make each kind of device misbehave.
//!
//! The person says what stopped working; the settings below for that kind,
//! if Dynamis Power changed them, are put back and blocked on this laptop.
//! A kind with none of its settings changed is most likely not caused by
//! Dynamis Power, and nothing is blocked: good settings are not blocked for
//! a fault they cannot cause.
//!
//! Besides setting ids, two ids stand for groups the engine manages:
//! `usb.autosuspend` and `pci.runtime_pm` (devices let sleep when idle) and
//! `radios` (radios switched off by rule).

/// The kinds of trouble the app asks about, in the order shown.
pub const KINDS: &[&str] = &[
    "wifi",
    "bluetooth",
    "sound",
    "input",
    "screen",
    "storage",
    "sleep",
    "freeze",
    "other",
];

/// The settings that could cause this kind of trouble. `other` has none:
/// for it, what was just undone is blocked.
pub fn suspects(kind: &str) -> &'static [&'static str] {
    match kind {
        "wifi" => &[
            "wifi.power_save",
            "pcie.aspm",
            "pci.runtime_pm",
            "usb.autosuspend",
            "radios",
        ],
        "bluetooth" => &["usb.autosuspend", "pci.runtime_pm", "radios"],
        "sound" => &[
            "audio.power_save",
            "audio.controller_power",
            "pcie.aspm",
            "pci.runtime_pm",
        ],
        "input" => &["usb.autosuspend", "pci.runtime_pm"],
        "screen" => &[
            "gpu.max_freq",
            "gpu.min_freq",
            "gpu.amd_power_level",
            "gpu.dgpu_runtime_pm",
            "pcie.aspm",
            "pci.runtime_pm",
        ],
        "storage" => &[
            "storage.sata_alpm",
            "storage.disk_apm",
            "storage.io_scheduler",
            "usb.autosuspend",
            "pcie.aspm",
            "pci.runtime_pm",
        ],
        "sleep" => &[
            "system.mem_sleep",
            "net.wake_on_lan",
            "usb.autosuspend",
            "pci.runtime_pm",
        ],
        "freeze" => &[
            "pcie.aspm",
            "pci.runtime_pm",
            "system.mem_sleep",
            "storage.sata_alpm",
            "gpu.max_freq",
            "gpu.min_freq",
            "gpu.amd_power_level",
            "gpu.dgpu_runtime_pm",
            "cpu.min_freq",
            "cpu.max_freq",
        ],
        _ => &[],
    }
}

/// A plain name for an id, including the group ids.
pub fn label(id: &str) -> String {
    match id {
        "usb.autosuspend" => "USB devices sleeping when idle".into(),
        "pci.runtime_pm" => "Built-in devices sleeping when idle".into(),
        "devices" => "Devices sleeping when idle".into(),
        "radios" => "Radios switched off by rule".into(),
        _ if crate::radios::valid_id(id) => format!("Radio switch {id}"),
        _ => crate::settings::by_id(id)
            .map(|s| s.label().to_string())
            .unwrap_or_else(|| id.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_suspect_is_a_known_setting_or_group() {
        for kind in KINDS {
            for id in suspects(kind) {
                assert!(
                    crate::settings::by_id(id).is_some()
                        || matches!(*id, "usb.autosuspend" | "pci.runtime_pm" | "radios"),
                    "{kind}: {id}"
                );
            }
        }
        assert!(suspects("other").is_empty());
        assert_eq!(label("radios"), "Radios switched off by rule");
    }
}
