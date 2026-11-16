//! Counting unclean shutdowns, for automatic safe mode.
//!
//! `dynamisd` calls [`record_start`] when it starts. It leaves a `running`
//! marker in the state directory; the systemd unit deletes it when the service
//! is stopped normally (which includes every normal shutdown or reboot). If a
//! new boot finds the marker still there, the previous boot ended without a
//! normal stop: a freeze, a crash of the whole system, or a power cut.
//!
//! After [`UNCLEAN_LIMIT`] such boots in a row, safe mode is switched on and
//! **stays on until a person switches it off** (`dynamis safe-mode off`):
//! if Dynamis Power caused the freezes, turning itself back on after the next
//! clean boot would just start the cycle again.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fs::SysRoot;

pub const UNCLEAN_LIMIT: u32 = 3;
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Counter {
    v: u32,
    /// The boot this count was last updated in.
    boot_id: String,
    /// Unclean shutdowns in a row, ending with the previous boot.
    unclean: u32,
}

/// What [`record_start`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootCheck {
    pub unclean_in_a_row: u32,
    /// Safe mode was switched on by this check.
    pub safe_mode_now_on: bool,
}

/// Call once when the service starts. Updates the count and the marker, and
/// switches safe mode on when the limit is reached.
pub fn record_start(root: &SysRoot, state_dir: &Path) -> std::io::Result<BootCheck> {
    let boot_id = root
        .read_string("/proc/sys/kernel/random/boot_id")
        .unwrap_or_default();
    let counter_path = state_dir.join("unclean-boots.json");
    let mut c: Counter = fs::read_to_string(&counter_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let marker = state_dir.join("running");
    let mut turned_on = false;

    // A restart within the same boot (the service crashed and systemd
    // restarted it) is not a new boot and changes nothing.
    if c.boot_id != boot_id || boot_id.is_empty() {
        c.unclean = if marker.exists() { c.unclean + 1 } else { 0 };
        c.boot_id = boot_id.clone();
        c.v = FORMAT_VERSION;
        fs::write(&counter_path, serde_json::to_string(&c).unwrap_or_default())?;
        if c.unclean >= UNCLEAN_LIMIT && !state_dir.join(crate::engine::SAFE_MODE_FILE).exists() {
            fs::write(
                state_dir.join(crate::engine::SAFE_MODE_FILE),
                format!("{} unclean shutdowns in a row\n", c.unclean),
            )?;
            turned_on = true;
        }
    }
    fs::write(&marker, &boot_id)?;
    Ok(BootCheck {
        unclean_in_a_row: c.unclean,
        safe_mode_now_on: turned_on,
    })
}

/// Reset the count (used when a person switches safe mode off).
pub fn reset(state_dir: &Path) -> std::io::Result<()> {
    let path = state_dir.join("unclean-boots.json");
    if let Some(mut c) = fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Counter>(&t).ok())
    {
        c.unclean = 0;
        fs::write(&path, serde_json::to_string(&c).unwrap_or_default())?;
    }
    Ok(())
}
