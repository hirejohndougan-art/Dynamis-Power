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

pub(crate) fn write_all(root: &SysRoot, writes: &[Write]) -> Result<(), String> {
    for w in writes {
        if w.path.contains("..") || !ALLOWED_PREFIXES.iter().any(|p| w.path.starts_with(p)) {
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
