//! Spinning hard disks: Advanced Power Management level (TLP's
//! `DISK_APM_LEVEL_ON_AC/BAT`).
//!
//! Not a file: read with `HDIO_GET_IDENTITY` (IDENTIFY DEVICE words 83, 86
//! and 91) and set with `HDIO_DRIVE_CMD` (ATA SET FEATURES 0x05 with the
//! level, or 0x85 to switch APM off), as `hdparm -B` does. Only disks the
//! kernel calls rotational. Levels below 128 let the disk spin down by
//! itself and wear it with frequent head parking; they are never offered.
//!
//! The spin-down timer (`hdparm -S`) cannot be read back, so it cannot be
//! put back on Restore: it is not offered.
//!
//! Tests never reach a real disk (safety rule 8): under a fixture root the
//! level lives in `/run/dynamis-test/ata/<disk>/apm` (`off` or a number;
//! no file means the disk has no APM).

use std::path::Path;

use crate::fs::SysRoot;

const FIXTURE: &str = "/run/dynamis-test/ata";

/// A disk name like `sda`: only SATA/SCSI disks, whole disks.
pub fn valid_disk(name: &str) -> bool {
    name.strip_prefix("sd")
        .is_some_and(|r| !r.is_empty() && r.len() <= 3 && r.bytes().all(|b| b.is_ascii_lowercase()))
}

/// Spinning disks (`queue/rotational` = 1).
pub fn rotational_disks(root: &SysRoot) -> Vec<String> {
    let mut out: Vec<String> = root
        .list_dir("/sys/block")
        .into_iter()
        .filter(|d| valid_disk(d))
        .filter(|d| {
            root.read_string(&format!("/sys/block/{d}/queue/rotational"))
                .is_ok_and(|r| r.trim() == "1")
        })
        .collect();
    out.sort();
    out
}

fn is_real(root: &SysRoot) -> bool {
    root.root() == Path::new("/")
}

/// The APM level: `Ok(Some(level))`, `Ok(None)` if APM is switched off,
/// `Err` if the disk has no APM or cannot be asked.
pub fn get_apm(root: &SysRoot, disk: &str) -> Result<Option<u8>, String> {
    if !valid_disk(disk) {
        return Err(format!("not a disk name: {disk}"));
    }
    if !is_real(root) {
        let v = root
            .read_string(&format!("{FIXTURE}/{disk}/apm"))
            .map_err(|_| "this disk has no power management level".to_string())?;
        let v = v.trim();
        return if v == "off" {
            Ok(None)
        } else {
            v.parse().map(Some).map_err(|_| format!("odd value {v}"))
        };
    }
    ioctl::get_apm(disk)
}

/// Set the level (`None` switches APM off). Only the writer calls this,
/// after its allow-list check.
pub(crate) fn set_apm(root: &SysRoot, disk: &str, level: Option<u8>) -> Result<(), String> {
    if !valid_disk(disk) {
        return Err(format!("not a disk name: {disk}"));
    }
    if level.is_some_and(|l| l < 128) {
        return Err("levels below 128 are not allowed".into());
    }
    if !is_real(root) {
        let p = root.path(&format!("{FIXTURE}/{disk}/apm"));
        if !p.exists() {
            return Err(format!("{}: no such file", p.display()));
        }
        return std::fs::write(p, level.map_or("off".to_string(), |l| l.to_string()))
            .map_err(|e| e.to_string());
    }
    ioctl::set_apm(disk, level)
}

/// For tests: a spinning disk in a fixture (`apm`: `None` = no APM).
pub mod fixture {
    use std::path::Path;

    pub fn disk(root: &Path, name: &str, apm: Option<&str>) {
        let q = root.join("sys/block").join(name).join("queue");
        std::fs::create_dir_all(&q).unwrap();
        std::fs::write(q.join("rotational"), "1\n").unwrap();
        if let Some(a) = apm {
            let d = root.join("run/dynamis-test/ata").join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("apm"), a).unwrap();
        }
    }
}

mod ioctl {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    const HDIO_GET_IDENTITY: libc::c_ulong = 0x030d;
    const HDIO_DRIVE_CMD: libc::c_ulong = 0x031f;
    const ATA_OP_SETFEATURES: u8 = 0xef;
    const SETFEATURES_APM_ON: u8 = 0x05;
    const SETFEATURES_APM_OFF: u8 = 0x85;

    fn open(disk: &str) -> Result<std::fs::File, String> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(format!("/dev/{disk}"))
            .map_err(|e| format!("/dev/{disk}: {e}"))
    }

    pub fn get_apm(disk: &str) -> Result<Option<u8>, String> {
        let f = open(disk)?;
        let mut id = [0u16; 256];
        // SAFETY: HDIO_GET_IDENTITY fills exactly 512 bytes; `id` is 512
        // bytes, lives across the call, and the result is checked.
        let r = unsafe { libc::ioctl(f.as_raw_fd(), HDIO_GET_IDENTITY as _, id.as_mut_ptr()) };
        if r != 0 {
            return Err(format!(
                "{disk}: cannot read its identity: {}",
                std::io::Error::last_os_error()
            ));
        }
        let word = |n: usize| u16::from_le(id[n]);
        if word(83) & (1 << 3) == 0 {
            return Err("this disk has no power management level".into());
        }
        if word(86) & (1 << 3) == 0 {
            return Ok(None);
        }
        Ok(Some((word(91) & 0xff) as u8))
    }

    pub fn set_apm(disk: &str, level: Option<u8>) -> Result<(), String> {
        let f = open(disk)?;
        // command, sector count, feature, (unused)
        let mut args: [u8; 4] = match level {
            Some(l) => [ATA_OP_SETFEATURES, l, SETFEATURES_APM_ON, 0],
            None => [ATA_OP_SETFEATURES, 0, SETFEATURES_APM_OFF, 0],
        };
        // SAFETY: HDIO_DRIVE_CMD takes a 4-byte argument block, which
        // `args` is; it lives across the call and the result is checked.
        let r = unsafe { libc::ioctl(f.as_raw_fd(), HDIO_DRIVE_CMD as _, args.as_mut_ptr()) };
        if r != 0 {
            return Err(format!(
                "{disk}: the disk refused: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}
