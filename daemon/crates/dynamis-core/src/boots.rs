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
//!
//! Safe mode doubles as a test. Unclean shutdowns while it is on are counted
//! separately ([`BootCheck::in_safe_mode`]): Dynamis Power changed nothing
//! then, so freezes that continue point elsewhere. Settings changed shortly
//! before the freezes that led to safe mode are kept as *suspects*; if no
//! freeze happens in safe mode, switching it off blocks them
//! ([`suspects_to_block`]).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fs::SysRoot;

pub const UNCLEAN_LIMIT: u32 = 3;
/// Version 2 adds `in_safe_mode` and `suspects` (both default when missing,
/// so version 1 files read as they are).
const FORMAT_VERSION: u32 = 2;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Counter {
    v: u32,
    /// The boot this count was last updated in.
    boot_id: String,
    /// Unclean shutdowns in a row, ending with the previous boot.
    unclean: u32,
    /// Unclean shutdowns while safe mode was on, since it was last off.
    #[serde(default)]
    in_safe_mode: u32,
    /// Settings changed shortly before unclean shutdowns outside safe mode.
    #[serde(default)]
    suspects: Vec<String>,
}

/// What [`record_start`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootCheck {
    pub unclean_in_a_row: u32,
    /// Safe mode was switched on by this check.
    pub safe_mode_now_on: bool,
    /// The previous boot (in this service's view) ended uncleanly.
    pub unclean: bool,
    /// ...and safe mode was on during it.
    pub unclean_in_safe_mode: bool,
    /// Unclean shutdowns while safe mode was on, since it was last off.
    pub in_safe_mode: u32,
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
    let mut unclean = false;
    let mut unclean_in_safe = false;

    // A restart within the same boot (the service crashed and systemd
    // restarted it) is not a new boot and changes nothing.
    if c.boot_id != boot_id || boot_id.is_empty() {
        unclean = marker.exists();
        unclean_in_safe = unclean && state_dir.join(crate::engine::SAFE_MODE_FILE).exists();
        c.unclean = if unclean { c.unclean + 1 } else { 0 };
        if unclean_in_safe {
            c.in_safe_mode += 1;
        }
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
        unclean,
        unclean_in_safe_mode: unclean_in_safe,
        in_safe_mode: c.in_safe_mode,
    })
}

fn read(state_dir: &Path) -> Option<Counter> {
    fs::read_to_string(state_dir.join("unclean-boots.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

fn write(state_dir: &Path, c: &Counter) -> std::io::Result<()> {
    fs::write(
        state_dir.join("unclean-boots.json"),
        serde_json::to_string(c).unwrap_or_default(),
    )
}

/// Note settings changed shortly before an unclean shutdown.
pub fn add_suspects(state_dir: &Path, ids: &[String]) -> std::io::Result<()> {
    let Some(mut c) = read(state_dir) else {
        return Ok(());
    };
    for id in ids {
        if !c.suspects.contains(id) {
            c.suspects.push(id.clone());
        }
    }
    c.v = FORMAT_VERSION;
    write(state_dir, &c)
}

/// The suspects so far, and the unclean shutdowns while safe mode was on.
pub fn suspects(state_dir: &Path) -> (Vec<String>, u32) {
    read(state_dir)
        .map(|c| (c.suspects, c.in_safe_mode))
        .unwrap_or_default()
}

/// What to block when safe mode is switched off: the suspects, but only if
/// no unclean shutdown happened while it was on (otherwise the cause is
/// probably elsewhere and they are let go).
pub fn suspects_to_block(state_dir: &Path) -> Vec<String> {
    match read(state_dir) {
        Some(c) if c.in_safe_mode == 0 => c.suspects,
        _ => Vec::new(),
    }
}

/// Reset the count (used when a person switches safe mode off).
pub fn reset(state_dir: &Path) -> std::io::Result<()> {
    let path = state_dir.join("unclean-boots.json");
    if let Some(mut c) = fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Counter>(&t).ok())
    {
        c.unclean = 0;
        c.in_safe_mode = 0;
        c.suspects.clear();
        c.v = FORMAT_VERSION;
        fs::write(&path, serde_json::to_string(&c).unwrap_or_default())?;
    }
    Ok(())
}
