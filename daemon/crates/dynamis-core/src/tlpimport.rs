//! Importing a TLP configuration (decision 0017).
//!
//! Reads `/etc/tlp.d/*.conf` (in name order) and then `/etc/tlp.conf`, the
//! order TLP itself uses, so later values win. Text only: nothing in the
//! files is run. Each line that maps to a Dynamis Power setting becomes one
//! proposed change:
//!
//! - A value that is the same on mains and on battery, or has no
//!   `_ON_AC`/`_ON_BAT` form, becomes an ordinary setting, kept after
//!   restarts.
//! - Where mains and battery differ, the battery value goes into Longer
//!   battery and the mains value into Faster, for the settings a mode sets
//!   on this laptop. For other settings Dynamis Power uses one value, so the
//!   battery value is proposed (TLP's battery tuning is usually the point).
//! - Values this laptop does not offer, settings Dynamis Power leaves to the
//!   person (risky ones) or does not have, are listed with the reason.
//!
//! Nothing is applied here: the service shows the list and applies the
//! lines the person keeps ticked.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::fs::SysRoot;
use crate::profiles::{self, Profile};
use crate::settings::{self, Probe};

const MAIN: &str = "/etc/tlp.conf";
const DROPINS: &str = "/etc/tlp.d";

/// Where an imported value goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    /// An ordinary setting, kept after restarts.
    Setting,
    /// The person's choice for one mode (Advanced → Profiles).
    PowerSaver,
    Performance,
}

impl Target {
    pub fn profile(self) -> Option<Profile> {
        match self {
            Target::Setting => None,
            Target::PowerSaver => Some(Profile::PowerSaver),
            Target::Performance => Some(Profile::Performance),
        }
    }
}

/// One line of the preview.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Line {
    /// The TLP line(s) it comes from, as written there.
    pub tlp: String,
    pub setting: Option<&'static str>,
    pub label: Option<&'static str>,
    pub value: Option<String>,
    pub target: Option<Target>,
    /// Why it is not imported, if it is not.
    pub skipped: Option<String>,
    /// Something the person should know (one value used for both).
    pub note: Option<String>,
}

/// Read and merge the files. Keys keep their last value; `KEY+=` appends.
pub fn read(root: &SysRoot) -> Option<BTreeMap<String, String>> {
    let mut files: Vec<String> = root
        .list_dir(DROPINS)
        .into_iter()
        .filter(|n| n.ends_with(".conf"))
        .map(|n| format!("{DROPINS}/{n}"))
        .collect();
    files.sort();
    files.push(MAIN.into());
    let mut found = false;
    let mut out = BTreeMap::new();
    for f in files {
        let Ok(text) = root.read_string(&f) else {
            continue;
        };
        found = true;
        parse_into(&text, &mut out);
    }
    found.then_some(out)
}

/// TLP's format: `KEY=value` or `KEY="value"`, `#` comments.
pub fn parse_into(text: &str, out: &mut BTreeMap<String, String>) {
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, rest)) = line.split_once('=') else {
            continue;
        };
        let (key, append) = match key.strip_suffix('+') {
            Some(k) => (k.trim(), true),
            None => (key.trim(), false),
        };
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            continue;
        }
        let rest = rest.trim();
        let value = if let Some(q) = rest.strip_prefix('"') {
            q.split('"').next().unwrap_or("").to_string()
        } else {
            rest.split('#').next().unwrap_or("").trim().to_string()
        };
        if append {
            let v = out.entry(key.to_string()).or_default();
            if !v.is_empty() {
                v.push(' ');
            }
            v.push_str(&value);
        } else {
            out.insert(key.to_string(), value);
        }
    }
}

/// How a TLP value becomes a Dynamis Power value (`None`: no meaning here,
/// such as TLP's "default").
type Convert = fn(&str) -> Option<String>;

fn same(v: &str) -> Option<String> {
    (!v.is_empty() && v != "default").then(|| v.to_string())
}
fn on_off(v: &str) -> Option<String> {
    match v {
        "1" | "Y" | "y" => Some("on".into()),
        "0" | "N" | "n" => Some("off".into()),
        _ => None,
    }
}
/// `WOL_DISABLE=Y` means Wake-on-LAN off.
fn wol(v: &str) -> Option<String> {
    match v {
        "Y" | "y" | "1" => Some("off".into()),
        "N" | "n" | "0" => Some("on".into()),
        _ => None,
    }
}
/// TLP's APM levels are per disk ("128 128"); the first one counts, moved
/// up to the nearest level offered (never below 128); 255 means off.
fn apm_level(v: &str) -> Option<String> {
    let l: u32 = v.split_whitespace().next()?.parse().ok()?;
    Some(match l {
        255 => "off".into(),
        0..=128 => "128".into(),
        129..=192 => "192".into(),
        _ => "254".into(),
    })
}

/// Graphics frequencies are offered in 50 MHz steps.
fn gpu_mhz(v: &str) -> Option<String> {
    v.parse::<u64>()
        .ok()
        .map(|m| ((m + 25) / 50 * 50).to_string())
}
fn khz_to_mhz(v: &str) -> Option<String> {
    v.parse::<u64>().ok().map(|k| (k / 1000).to_string())
}
fn secs_to_centisecs(v: &str) -> Option<String> {
    v.parse::<u64>().ok().map(|s| (s * 100).to_string())
}
/// A list of choices, the first one this laptop accepts wins (TLP does the
/// same for `SATA_LINKPWR`); the whole list is kept for that check.
fn list(v: &str) -> Option<String> {
    let v = v.trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// TLP keys with a value for mains and one for battery.
const PER_SOURCE: &[(&str, &str, Convert)] = &[
    ("CPU_ENERGY_PERF_POLICY", "cpu.epp", same),
    ("PLATFORM_PROFILE", "platform.profile", same),
    ("CPU_BOOST", "cpu.turbo", on_off),
    ("CPU_HWP_DYN_BOOST", "cpu.hwp_dynamic_boost", on_off),
    ("CPU_MAX_PERF", "cpu.max_perf_pct", same),
    ("CPU_MIN_PERF", "cpu.min_perf_pct", same),
    ("CPU_SCALING_GOVERNOR", "cpu.governor", same),
    ("CPU_SCALING_MAX_FREQ", "cpu.max_freq", khz_to_mhz),
    ("CPU_SCALING_MIN_FREQ", "cpu.min_freq", khz_to_mhz),
    ("SOUND_POWER_SAVE", "audio.power_save", same),
    ("PCIE_ASPM", "pcie.aspm", same),
    ("SATA_LINKPWR", "storage.sata_alpm", list),
    ("RADEON_DPM_PERF_LEVEL", "gpu.amd_power_level", same),
    ("MEM_SLEEP", "system.mem_sleep", same),
    (
        "MAX_LOST_WORK_SECS",
        "vm.dirty_writeback",
        secs_to_centisecs,
    ),
    ("WIFI_PWR", "wifi.power_save", same),
    ("INTEL_GPU_MAX_FREQ", "gpu.max_freq", gpu_mhz),
    ("INTEL_GPU_MIN_FREQ", "gpu.min_freq", gpu_mhz),
    ("DISK_APM_LEVEL", "storage.disk_apm", apm_level),
];

/// TLP keys with one value.
const SINGLE: &[(&str, &str, Convert)] = &[
    ("NMI_WATCHDOG", "kernel.nmi_watchdog", on_off),
    (
        "SOUND_POWER_SAVE_CONTROLLER",
        "audio.controller_power",
        on_off,
    ),
    ("START_CHARGE_THRESH_BAT0", "battery.charge_start", same),
    ("STOP_CHARGE_THRESH_BAT0", "battery.charge_limit", same),
    ("DISK_IOSCHED", "storage.io_scheduler", list),
    ("WOL_DISABLE", "net.wake_on_lan", wol),
];

/// TLP keys Dynamis Power handles another way, with where.
const ELSEWHERE: &[(&str, &str)] = &[
    ("DEVICES_TO_DISABLE", "use Devices → Radios"),
    ("DEVICES_TO_ENABLE", "use Devices → Radios"),
    (
        "RESTORE_DEVICE_STATE_ON_STARTUP",
        "systemd-rfkill already does this",
    ),
    (
        "USB_AUTOSUSPEND",
        "use Devices → Let idle USB devices sleep",
    ),
    (
        "RUNTIME_PM_ON_AC",
        "use Devices → Let idle built-in hardware sleep",
    ),
    (
        "RUNTIME_PM_ON_BAT",
        "use Devices → Let idle built-in hardware sleep",
    ),
    (
        "TLP_PS_IGNORE",
        "use Advanced → System → When is the laptop plugged in?",
    ),
    (
        "TLP_DEFAULT_MODE",
        "Dynamis Power starts with the mode last chosen",
    ),
    ("TLP_ENABLE", "not needed"),
    ("TLP_PERSISTENT_DEFAULT", "not needed"),
    ("TLP_WARN_LEVEL", "not needed"),
    ("TLP_MSG_COLORS", "not needed"),
];

/// Values TLP uses that mean "leave it alone".
fn means_nothing(v: &str) -> bool {
    v.is_empty() || v == "default" || v == "keep"
}

/// A value this laptop accepts for `id`, or why not. Lists (`a b c`) pick
/// the first value accepted.
fn accept(root: &SysRoot, id: &str, value: &str) -> Result<String, String> {
    let s = settings::by_id(id).ok_or("not a setting here")?;
    let allowed = match s.probe(root) {
        Probe::Supported { allowed } => allowed,
        Probe::Unsupported { reason } => return Err(format!("not on this laptop ({reason})")),
    };
    let mut last_err = String::new();
    for v in value.split_whitespace() {
        match allowed.check(v) {
            Ok(v) => return Ok(v),
            Err(e) => last_err = e,
        }
    }
    Err(format!("{value} is not allowed here: {last_err}"))
}

/// Build the preview. `blocked` and `refused` say which settings this
/// laptop must not change (blocked here, or a quirk forbids the value).
pub fn plan(
    root: &SysRoot,
    conf: &BTreeMap<String, String>,
    blocked: &BTreeMap<String, String>,
    refused: impl Fn(&str, &str) -> Option<String>,
) -> Vec<Line> {
    let mut out = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let label = |id: &str| settings::by_id(id).map(|s| s.label());
    let check = |id: &'static str, value: &str| -> Result<String, String> {
        let s = settings::by_id(id).ok_or("not a setting here")?;
        if s.risk().needs_opt_in() {
            return Err("risky: set it in Advanced, where it asks for a yes".into());
        }
        if let Some(why) = blocked.get(id) {
            return Err(format!("blocked on this laptop: {why}"));
        }
        let v = accept(root, id, value)?;
        if let Some(why) = refused(id, &v) {
            return Err(format!("not allowed on this laptop: {why}"));
        }
        Ok(v)
    };
    let line = |tlp: String, id: &'static str, value: Option<String>, target: Target| Line {
        tlp,
        setting: Some(id),
        label: label(id),
        value,
        target: Some(target),
        skipped: None,
        note: None,
    };
    let skip = |tlp: String, id: Option<&'static str>, why: String| Line {
        tlp,
        setting: id,
        label: id.and_then(label),
        value: None,
        target: None,
        skipped: Some(why),
        note: None,
    };

    for (key, id, conv) in PER_SOURCE {
        let ac_key = format!("{key}_ON_AC");
        let bat_key = format!("{key}_ON_BAT");
        let ac_raw = conf.get(&ac_key).filter(|v| !means_nothing(v));
        let bat_raw = conf.get(&bat_key).filter(|v| !means_nothing(v));
        used.push(ac_key.clone());
        used.push(bat_key.clone());
        if ac_raw.is_none() && bat_raw.is_none() {
            continue;
        }
        let shown = |k: &str, v: Option<&String>| v.map(|v| format!("{k}=\"{v}\""));
        let tlp_text = [shown(&ac_key, ac_raw), shown(&bat_key, bat_raw)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("  ");
        let ac = ac_raw.and_then(|v| conv(v));
        let bat = bat_raw.and_then(|v| conv(v));
        let ac = ac.map(|v| check(id, &v));
        let bat = bat.map(|v| check(id, &v));
        match (ac, bat) {
            (Some(Ok(a)), Some(Ok(b))) if a == b => {
                out.push(line(tlp_text, id, Some(b), Target::Setting))
            }
            (a, b) => {
                let by_mode = |p: Profile| profiles::editable(root, p).iter().any(|(e, _)| e == id);
                if by_mode(Profile::PowerSaver) || by_mode(Profile::Performance) {
                    for (val, target, k) in [
                        (b, Target::PowerSaver, &bat_key),
                        (a, Target::Performance, &ac_key),
                    ] {
                        let Some(val) = val else { continue };
                        let p = target.profile().unwrap_or_default();
                        let t = format!(
                            "{k}=\"{}\"",
                            conf.get(k.as_str()).cloned().unwrap_or_default()
                        );
                        match val {
                            Ok(v) if by_mode(p) => {
                                let ok = profiles::editable(root, p)
                                    .iter()
                                    .any(|(e, c)| e == id && c.contains(&v));
                                if ok {
                                    out.push(line(t, id, Some(v), target));
                                } else {
                                    out.push(skip(
                                        t,
                                        Some(id),
                                        format!("{v} is not a choice for this mode here"),
                                    ));
                                }
                            }
                            Ok(_) => {
                                out.push(skip(t, Some(id), "this mode does not set it here".into()))
                            }
                            Err(e) => out.push(skip(t, Some(id), e)),
                        }
                    }
                } else {
                    // One value for this setting: the battery one if any.
                    match (b, a) {
                        (Some(Ok(v)), other) => {
                            let mut l = line(tlp_text, id, Some(v), Target::Setting);
                            if other.is_some() {
                                l.note = Some("Dynamis Power uses one value for this setting; the battery value is used".into());
                            }
                            out.push(l);
                        }
                        (None, Some(Ok(v))) => {
                            out.push(line(tlp_text, id, Some(v), Target::Setting))
                        }
                        (Some(Err(e)), _) | (None, Some(Err(e))) => {
                            out.push(skip(tlp_text, Some(id), e))
                        }
                        (None, None) => {}
                    }
                }
            }
        }
    }

    for (key, id, conv) in SINGLE {
        used.push(key.to_string());
        let Some(raw) = conf.get(*key).filter(|v| !means_nothing(v)) else {
            continue;
        };
        let t = format!("{key}=\"{raw}\"");
        match conv(raw).map(|v| check(id, &v)) {
            Some(Ok(v)) => out.push(line(t, id, Some(v), Target::Setting)),
            Some(Err(e)) => out.push(skip(t, Some(id), e)),
            None => out.push(skip(t, Some(id), format!("{raw} has no meaning here"))),
        }
    }

    for (key, value) in conf {
        if used.contains(key) || means_nothing(value) {
            continue;
        }
        let why = ELSEWHERE
            .iter()
            .find(|(k, _)| key.starts_with(k))
            .map(|(_, w)| w.to_string())
            .unwrap_or_else(|| "Dynamis Power has no setting for this yet".into());
        out.push(skip(format!("{key}=\"{value}\""), None, why));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tlp_syntax() {
        let mut m = BTreeMap::new();
        parse_into(
            "# comment\nTLP_ENABLE=1\nCPU_BOOST_ON_BAT=\"0\"  # off on battery\n\
             SATA_LINKPWR_ON_BAT=\"med_power_with_dipm min_power\"\nbad line\n\
             lower=1\nDISK_IOSCHED=\"mq-deadline\"\nDISK_IOSCHED+=\"bfq\"\n",
            &mut m,
        );
        assert_eq!(m["TLP_ENABLE"], "1");
        assert_eq!(m["CPU_BOOST_ON_BAT"], "0");
        assert_eq!(m["SATA_LINKPWR_ON_BAT"], "med_power_with_dipm min_power");
        assert_eq!(m["DISK_IOSCHED"], "mq-deadline bfq");
        assert!(!m.contains_key("lower"));
        assert_eq!(m.len(), 4);
    }

    #[test]
    fn conversions() {
        assert_eq!(on_off("1").as_deref(), Some("on"));
        assert_eq!(on_off("N").as_deref(), Some("off"));
        assert_eq!(khz_to_mhz("2400000").as_deref(), Some("2400"));
        assert_eq!(secs_to_centisecs("15").as_deref(), Some("1500"));
        assert_eq!(same("default"), None);
    }
}
