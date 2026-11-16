//! The only code in Dynamis Power that writes to kernel settings.
//!
//! It is `pub(crate)`: nothing outside this crate can call it, and inside the
//! crate only [`crate::engine::Engine`] does, after journaling. Keeping every
//! write in one small file makes the write path easy to audit.

use std::fs::OpenOptions;
use std::io::Write as _;

use crate::fs::SysRoot;
use crate::settings::Write;

/// Kernel paths a setting may write under. Anything else is refused, even if a
/// plugin asks for it — a second line of defence against a buggy plugin.
const ALLOWED_PREFIXES: &[&str] = &[
    "/sys/devices/system/cpu/",
    "/sys/firmware/acpi/platform_profile",
    "/sys/module/snd_hda_intel/parameters/",
    "/sys/module/pcie_aspm/parameters/",
    "/sys/class/power_supply/",
    "/proc/sys/vm/",
    "/proc/sys/kernel/nmi_watchdog",
];

/// Per-app CPU weights (phase 6): only this one file, only inside user
/// sessions. Never system services, never other cgroup controls.
const CGROUP_PREFIX: &str = "/sys/fs/cgroup/user.slice/";
const CGROUP_FILE: &str = "/cpu.weight";

/// Directories where only one kind of file may be written: (prefix, the
/// only file name allowed at the end).
const ONE_FILE_ONLY: &[(&str, &str)] = &[
    ("/sys/block/", "/queue/scheduler"),
    (
        "/sys/class/drm/",
        "/device/power_dpm_force_performance_level",
    ),
    ("/sys/class/scsi_host/", "/link_power_management_policy"),
    // The wake-up alarm, for the sleep test only.
    ("/sys/class/rtc/", "/wakealarm"),
    // Screen brightness, for the brightness the person chose for each mode.
    ("/sys/class/backlight/", "/brightness"),
    // Runtime power management of one USB device or PCI function.
    ("/sys/bus/usb/devices/", "/power/control"),
    ("/sys/bus/pci/devices/", "/power/control"),
];

fn allowed(path: &str) -> bool {
    if path.contains("..") {
        return false;
    }
    if path.starts_with(CGROUP_PREFIX) {
        return path.ends_with(CGROUP_FILE);
    }
    if let Some((_, end)) = ONE_FILE_ONLY.iter().find(|(p, _)| path.starts_with(p)) {
        return path.ends_with(end);
    }
    if path == "/sys/power/mem_sleep" {
        return true;
    }
    ALLOWED_PREFIXES.iter().any(|p| path.starts_with(p))
}

/// The one write that is not a file: Wi-Fi power saving through nl80211
/// (decision 0018), `nl80211:<interface>/power_save` with `on` or `off`.
fn wifi_power_save(path: &str) -> Option<&str> {
    let ifname = path.strip_prefix("nl80211:")?.strip_suffix("/power_save")?;
    crate::wifi::valid_name(ifname).then_some(ifname)
}

pub(crate) fn write_all(root: &SysRoot, writes: &[Write]) -> Result<(), String> {
    for w in writes {
        if w.path.starts_with("nl80211:") {
            let ifname = wifi_power_save(&w.path)
                .ok_or_else(|| format!("refused to write outside the allowed paths: {}", w.path))?;
            let on = match w.contents.as_str() {
                "on" => true,
                "off" => false,
                v => return Err(format!("{}: {v} is not on or off", w.path)),
            };
            crate::wifi::set(root, ifname, on).map_err(|e| format!("{}: {e}", w.path))?;
            continue;
        }
        if !allowed(&w.path) {
            return Err(format!(
                "refused to write outside the allowed paths: {}",
                w.path
            ));
        }
        // No `create`: if the file does not exist, the kernel does not offer
        // this setting and we must not invent it.
        let mut f = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(root.path(&w.path))
            .map_err(|e| format!("{}: {e}", w.path))?;
        f.write_all(w.contents.as_bytes())
            .map_err(|e| format!("{}: {e}", w.path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_paths_outside_allow_list() {
        let root = SysRoot::at("/nonexistent");
        for path in [
            "/etc/passwd",
            "/sys/firmware/efi/x",
            "/proc/sys/vm/../kernel/x",
            "/sys/fs/cgroup/user.slice/app.scope/cgroup.freeze",
            "/sys/fs/cgroup/user.slice/app.scope/cpu.max",
            "/sys/fs/cgroup/system.slice/sshd.service/cpu.weight",
            "/sys/block/sda/queue/read_ahead_kb",
            "/sys/class/drm/card0/device/power_state",
            "/sys/power/state",
            "/sys/bus/usb/devices/1-2/authorized",
            "/sys/bus/usb/devices/1-2/power/autosuspend_delay_ms",
            "/sys/bus/pci/devices/0000:00:14.0/remove",
            "/sys/bus/pci/devices/0000:00:14.0/power/control/../../remove",
            "/sys/class/backlight/intel_backlight/bl_power",
            "nl80211:wlan0/ps",
            "nl80211:../x/power_save",
            "nl80211:wlan0/../../power_save",
            "/sys/class/backlight/intel_backlight/max_brightness",
        ] {
            let err = write_all(
                &root,
                &[Write {
                    path: path.into(),
                    contents: "1".into(),
                }],
            )
            .unwrap_err();
            assert!(err.contains("refused"), "{path}: {err}");
        }
    }

    #[test]
    fn never_creates_files() {
        let dir = std::env::temp_dir().join(format!("dynamis-writer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = SysRoot::at(&dir);
        let err = write_all(
            &root,
            &[Write {
                path: "/proc/sys/vm/swappiness".into(),
                contents: "10".into(),
            }],
        )
        .unwrap_err();
        assert!(err.contains("swappiness"));
        assert!(!root.exists("/proc/sys/vm/swappiness"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
