//! Other power tools that fight over the same settings.
//!
//! Asked of systemd over D-Bus (no shelling out). Only the tools listed in
//! [`KNOWN`] can ever be switched off from here, never an arbitrary unit,
//! and switching one off is recorded so that removing Dynamis Power turns it
//! back on.

use std::io::Write as _;
use std::path::Path;

use serde::Serialize;
use zbus::Connection;

/// (unit, plain name, whether it is started by D-Bus on demand and so must be
/// masked, not just disabled).
pub const KNOWN: &[(&str, &str, bool)] = &[
    ("tlp.service", "TLP", false),
    ("auto-cpufreq.service", "auto-cpufreq", false),
    ("laptop-mode.service", "Laptop Mode Tools", false),
    ("tuned.service", "TuneD", false),
    ("system76-power.service", "System76 Power", false),
    ("powertop.service", "PowerTOP auto-tune", false),
    (
        "power-profiles-daemon.service",
        "power-profiles-daemon",
        true,
    ),
    ("tuned-ppd.service", "tuned-ppd", true),
    // They set the processor's speed preference, governor and limits
    // themselves (decision 0022).
    ("tccd.service", "TUXEDO Control Center", false),
    ("slimbookbattery.service", "Slimbook Battery", false),
];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Conflict {
    pub unit: String,
    pub name: String,
    pub active: bool,
    /// systemd's file state: enabled, disabled, masked, static, …
    pub state: String,
}

const SD: &str = "org.freedesktop.systemd1";
const SD_PATH: &str = "/org/freedesktop/systemd1";
const SD_MGR: &str = "org.freedesktop.systemd1.Manager";

async fn unit_file_state(conn: &Connection, unit: &str) -> Option<String> {
    let m = conn
        .call_method(
            Some(SD),
            SD_PATH,
            Some(SD_MGR),
            "GetUnitFileState",
            &(unit,),
        )
        .await
        .ok()?;
    m.body().deserialize::<String>().ok()
}

async fn active_state(conn: &Connection, unit: &str) -> Option<String> {
    let m = conn
        .call_method(Some(SD), SD_PATH, Some(SD_MGR), "GetUnit", &(unit,))
        .await
        .ok()?;
    let path: zbus::OwnedObjectPath = m.body().deserialize().ok()?;
    let m = conn
        .call_method(
            Some(SD),
            path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.systemd1.Unit", "ActiveState"),
        )
        .await
        .ok()?;
    let v: zbus::OwnedVariant = m.body().deserialize().ok()?;
    String::try_from(v).ok()
}

/// Known tools that are installed and either running or set to start. Empty
/// when systemd cannot be asked (for example on a test bus).
pub async fn find(conn: &Connection) -> Vec<Conflict> {
    let mut out = Vec::new();
    for (unit, name, _) in KNOWN {
        let Some(state) = unit_file_state(conn, unit).await else {
            continue;
        };
        let active = active_state(conn, unit).await.as_deref() == Some("active");
        if active || state == "enabled" {
            out.push(Conflict {
                unit: (*unit).into(),
                name: (*name).into(),
                active,
                state,
            });
        }
    }
    out
}

/// Stop and disable (or mask) one known tool, and record it for uninstall.
pub async fn switch_off(conn: &Connection, unit: &str, state_dir: &Path) -> Result<(), String> {
    let Some((unit, _, mask)) = KNOWN.iter().find(|(u, _, _)| *u == unit) else {
        return Err(format!(
            "'{unit}' is not a power tool Dynamis Power knows how to switch off"
        ));
    };
    let call = |method: &'static str| async move {
        let r = match method {
            "StopUnit" => {
                conn.call_method(Some(SD), SD_PATH, Some(SD_MGR), method, &(*unit, "replace"))
                    .await
            }
            "DisableUnitFiles" => {
                conn.call_method(
                    Some(SD),
                    SD_PATH,
                    Some(SD_MGR),
                    method,
                    &(vec![*unit], false),
                )
                .await
            }
            "MaskUnitFiles" => {
                conn.call_method(
                    Some(SD),
                    SD_PATH,
                    Some(SD_MGR),
                    method,
                    &(vec![*unit], false, true),
                )
                .await
            }
            _ => {
                conn.call_method(Some(SD), SD_PATH, Some(SD_MGR), "Reload", &())
                    .await
            }
        };
        r.map(|_| ()).map_err(|e| e.to_string())
    };
    // Record first, so a failure half-way is still undone on uninstall.
    record(state_dir, unit).map_err(|e| e.to_string())?;
    call("StopUnit").await.or_else(|e| {
        if e.contains("NoSuchUnit") || e.contains("not loaded") {
            Ok(())
        } else {
            Err(e)
        }
    })?;
    call("DisableUnitFiles").await?;
    if *mask {
        call("MaskUnitFiles").await?;
    }
    call("Reload").await
}

fn record(state_dir: &Path, unit: &str) -> std::io::Result<()> {
    let path = state_dir.join("replaced-services");
    let name = unit.trim_end_matches(".service");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == name) {
        return Ok(());
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(f, "{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_tools_and_each_recorded_once() {
        let dir = std::env::temp_dir().join(format!("dynamisd-conflicts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        record(&dir, "tlp.service").unwrap();
        record(&dir, "tlp.service").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("replaced-services")).unwrap(),
            "tlp\n"
        );
        std::fs::remove_dir_all(&dir).ok();
        assert!(KNOWN.iter().all(|(u, _, _)| u.ends_with(".service")));
    }
}
