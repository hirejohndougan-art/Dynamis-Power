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
            "display.amd_panel_power_savings",
            "pcie.aspm",
            "pci.runtime_pm",
        ],
        "storage" => &[
            "storage.nvme_power_states",
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
            "storage.nvme_power_states",
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

/// The kind of trouble a kernel log record shows, if it is one the service
/// watches for all the time (not only during tests): from the kernel itself
/// (facility 0), matching a known sign of trouble, from a part of the
/// laptop that a setting could affect. Plain errors without such a sign are
/// left alone: many laptops log harmless errors all day.
pub fn from_kernel(prio: u32, message: &str) -> Option<&'static str> {
    if prio >> 3 != 0 || !crate::health::is_trouble(message) {
        return None;
    }
    let m = message.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| m.contains(w));
    // Order matters: the most specific first.
    if has(&[
        "soft lockup",
        "hard lockup",
        "hung task",
        "blocked for more than",
        "detected stall",
    ]) {
        Some("freeze")
    } else if has(&[
        "iwlwifi", "ath9k", "ath10k", "ath11k", "ath12k", "rtw88", "rtw89", "rtw_", "rtl8", "mt76",
        "mt79", "brcmf", "wlan", "wlp", "wlo",
    ]) {
        Some("wifi")
    } else if has(&["bluetooth", "btusb", "hci0", "hci1"]) {
        Some("bluetooth")
    } else if has(&[
        "snd_hda",
        "snd-hda",
        "hda_intel",
        "hda-intel",
        "snd_sof",
        "sof-audio",
        "azx_",
    ]) {
        Some("sound")
    } else if has(&[
        "i915", "xe ", "amdgpu", "radeon", "nouveau", "[drm]", "gpu hang",
    ]) {
        Some("screen")
    } else if has(&[
        "nvme",
        "mmc",
        "sdhci",
        "scsi",
        "blk_update_request",
        "i/o error",
    ]) || m.starts_with("ata")
        || m.starts_with("sd ")
    {
        Some("storage")
    } else if has(&["usb", "xhci", "i2c_hid", "i2c-hid", "psmouse", "atkbd"]) {
        Some("input")
    } else if has(&["aer:", "pcie bus error"]) {
        // PCIe link errors: the link power saving and runtime power.
        Some("freeze")
    } else {
        None
    }
}

/// A plain name for an id, including the group ids.
pub fn label(id: &str) -> String {
    match id {
        "usb.autosuspend" => "USB devices sleeping when idle".into(),
        "pci.runtime_pm" => "Built-in devices sleeping when idle".into(),
        "devices" => "Devices sleeping when idle".into(),
        "radios" => "Radios switched off by rule".into(),
        "keyboard.light" => "Keyboard light off on battery".into(),
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
        // Kernel records: facility 0 and a sign of trouble.
        assert_eq!(
            from_kernel(3, "iwlwifi 0000:00:14.3: Microcode SW error detected"),
            Some("wifi")
        );
        assert_eq!(
            from_kernel(
                6,
                "usb 1-2: reset high-speed USB device number 3 using xhci_hcd"
            ),
            Some("input")
        );
        assert_eq!(
            from_kernel(3, "i915 0000:00:02.0: [drm] GPU HANG: ecode 9:1:85dffffb"),
            Some("screen")
        );
        assert_eq!(
            from_kernel(3, "snd_hda_intel 0000:00:1f.3: azx_get_response timeout"),
            Some("sound")
        );
        assert_eq!(from_kernel(3, "ata1.00: exception Emask 0x0 SAct 0x0 SErr 0x0 action 0x6 frozen; failed command: READ, timeout"), Some("storage"));
        assert_eq!(
            from_kernel(0, "watchdog: BUG: soft lockup - CPU#1 stuck for 22s!"),
            Some("freeze")
        );
        assert_eq!(
            from_kernel(3, "pcieport 0000:00:1c.0: AER: Corrected error received"),
            Some("freeze")
        );
        // Plain errors, programs writing to the log, and ordinary messages.
        assert_eq!(
            from_kernel(3, "ACPI Error: AE_NOT_FOUND, Evaluating _DSM"),
            None
        );
        assert_eq!(from_kernel(27, "systemd[1]: foo.service: timeout"), None);
        assert_eq!(from_kernel(6, "wlan0: associated"), None);
        assert_eq!(label("radios"), "Radios switched off by rule");
    }
}
