//! Words the agent shows people (the panel icon's menu and tooltip).
//!
//! These follow the app's translation catalog (`gui/src/i18n/en.ts`): the
//! same keys where the app has them, the same plain language. To translate,
//! add a table for the language and pick it in [`s`] from `LANGUAGE`/`LANG`,
//! the way the app picks from the desktop's language.

const EN: &[(&str, &str)] = &[
    ("app_name", "Dynamis Power"),
    ("mode_power_saver", "Longer battery"),
    ("mode_balanced", "Balanced"),
    ("mode_performance", "Faster"),
    ("tray_auto", "Choose automatically"),
    ("tray_last", "Make it last"),
    ("tray_last_2h", "2 hours"),
    ("tray_last_4h", "4 hours"),
    ("tray_last_6h", "6 hours"),
    ("tray_last_stop", "Stop making it last"),
    ("tray_lasting", "making it last"),
    ("tray_full_charge", "Charge fully once"),
    ("tray_full_charge_stop", "Stop charging fully"),
    ("tray_open", "Open Dynamis Power"),
    ("tray_hide", "Hide this icon"),
    ("tray_left", "about {h} h {m} min left"),
    ("tray_plugged", "plugged in"),
    ("tray_safe_mode", "Safe mode is on: nothing is changed"),
    ("tray_no_service", "Dynamis Power is not running"),
    ("tray_feels_slow", "It feels slow"),
    ("tray_feels_slow_end", "Back to normal settings"),
    ("tray_something_broke", "Something stopped working…"),
    ("tray_auto_reason", "Chosen automatically: {reason}"),
    (
        "tray_relief",
        "Faster settings for the rest of the day (it felt slow)",
    ),
];

/// The text for a key (the key itself if missing, so a gap is visible).
pub fn s(key: &str) -> &str {
    EN.iter().find(|(k, _)| *k == key).map_or(key, |(_, v)| v)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_key_is_unique() {
        let mut keys: Vec<&str> = super::EN.iter().map(|(k, _)| *k).collect();
        keys.sort();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n);
    }
}
