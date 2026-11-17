//! Deeper link sleep (PCIe L1 substates): a connection between the
//! processor and a device such as the SSD or Wi-Fi card can drop into
//! deeper sleep states (L1.1, L1.2) between uses, if both ends support it.
//! Many laptops leave these off. Turning them on can save a little power at
//! idle, and on some hardware makes devices drop out or the laptop freeze,
//! so it is Experimental and off by default (decision 0027).
//!
//! The kernel shows `link/l1_*` files only for states the link supports, so
//! a file that exists is a capability; this module only reads. Graphics and
//! devices never let sleep when idle (`crate::devices`) are left alone. The
//! engine writes (`Engine::set_link_sleep`).

use crate::devices::Device;
use crate::fs::SysRoot;

/// The group id used to block this on a laptop, and in trouble reports.
pub const ID: &str = "pcie.l1_substates";

/// The files, in the order they are turned on: the link's own (ASPM)
/// substates, then the ones driven by the device's power state.
pub const FILES: &[&str] = &["l1_1_aspm", "l1_2_aspm", "l1_1_pcipm", "l1_2_pcipm"];

/// The substate files this device has, with their values.
pub fn files(root: &SysRoot, dev: &str) -> Vec<(&'static str, String)> {
    FILES
        .iter()
        .filter_map(|f| {
            root.read_string(&format!("{dev}/link/{f}"))
                .ok()
                .map(|v| (*f, v.trim().to_string()))
        })
        .collect()
}

/// Why a device is left alone, if it is.
pub fn excluded(d: &Device) -> Option<&'static str> {
    if d.what == "graphics" {
        return Some("graphics");
    }
    d.excluded
}

/// Whether every substate the device has is on.
pub fn all_on(files: &[(&'static str, String)]) -> bool {
    !files.is_empty() && files.iter().all(|(_, v)| v == "1")
}
