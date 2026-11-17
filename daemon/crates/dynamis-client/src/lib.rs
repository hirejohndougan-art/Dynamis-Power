//! A small client for the Dynamis Power service, used by the app's Tauri
//! shell. Every call goes over D-Bus to `dynamisd`; nothing here touches the
//! system directly, and nothing here needs root.
//!
//! Errors are plain sentences, ready to show to people.
//!
//! The service is on the system bus. Set `DYNAMIS_BUS=session` to use the
//! session bus instead (for development against a test service).

use serde_json::Value;
use zbus::Connection;

const NAME: &str = "net.dynamis.Power";
const PATH: &str = "/net/dynamis/Power";
const IFACE: &str = "net.dynamis.Power1";
const PPD: &str = "org.freedesktop.UPower.PowerProfiles";
const PPD_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

pub type Result<T> = std::result::Result<T, String>;

fn explain(e: impl std::fmt::Display) -> String {
    let t = e.to_string();
    if t.contains("ServiceUnknown")
        || t.contains("was not provided")
        || t.contains("NameHasNoOwner")
    {
        "Dynamis Power's service is not running. Install it, or start it with: sudo systemctl start dynamisd".into()
    } else if t.contains("AccessDenied") {
        "You are not allowed to change this. Only the person at the laptop can.".into()
    } else {
        // Strip the D-Bus error name prefix, keep the human part.
        t.split_once(": ")
            .map(|(_, rest)| rest.to_string())
            .unwrap_or(t)
    }
}

#[derive(Clone)]
pub struct Client {
    conn: Connection,
}

impl Client {
    pub async fn connect() -> Result<Client> {
        let conn = if std::env::var("DYNAMIS_BUS").as_deref() == Ok("session") {
            Connection::session().await
        } else {
            Connection::system().await
        }
        .map_err(explain)?;
        Ok(Client { conn })
    }

    pub async fn connect_to(address: &str) -> Result<Client> {
        let conn = zbus::connection::Builder::address(address)
            .build()
            .await
            .map_err(explain)?;
        Ok(Client { conn })
    }

    async fn proxy(
        &self,
        dest: &'static str,
        path: &'static str,
        iface: &'static str,
    ) -> Result<zbus::Proxy<'static>> {
        zbus::proxy::Builder::new(&self.conn)
            .destination(dest)
            .path(path)
            .interface(iface)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(explain)
    }

    async fn own(&self) -> Result<zbus::Proxy<'static>> {
        self.proxy(NAME, PATH, IFACE).await
    }

    async fn json(
        &self,
        method: &str,
        body: &(impl serde::Serialize + zbus::wire::DynamicType),
    ) -> Result<Value> {
        let text: String = self
            .own()
            .await?
            .call(method, body)
            .await
            .map_err(explain)?;
        serde_json::from_str(&text).map_err(explain)
    }

    pub async fn status(&self) -> Result<Value> {
        self.json("Status", &()).await
    }

    pub async fn journal(&self, n: u32) -> Result<Value> {
        self.json("Journal", &(n,)).await
    }

    pub async fn settings(&self) -> Result<Value> {
        self.json("Settings", &()).await
    }

    pub async fn inventory(&self) -> Result<Value> {
        self.json("Inventory", &()).await
    }

    pub async fn learned(&self) -> Result<Value> {
        self.json("LearnedJson", &()).await
    }

    pub async fn set_profile(&self, profile: &str) -> Result<()> {
        self.proxy(PPD, PPD_PATH, PPD)
            .await?
            .set_property("ActiveProfile", profile)
            .await
            .map_err(explain)
    }

    pub async fn set_auto(&self, on: bool) -> Result<()> {
        self.own()
            .await?
            .set_property("AutoMode", on)
            .await
            .map_err(explain)
    }

    pub async fn set_learning(&self, on: bool) -> Result<()> {
        self.own()
            .await?
            .set_property("Learning", on)
            .await
            .map_err(explain)
    }

    /// "It feels slow" (`lasting`: keep it this way). Returns what was in
    /// effect.
    pub async fn feels_slow(&self, lasting: bool) -> Result<Value> {
        let text: String = self
            .own()
            .await?
            .call("FeelsSlow", &(lasting,))
            .await
            .map_err(explain)?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    pub async fn end_relief(&self) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("EndRelief", &())
            .await
            .map_err(explain)
    }

    /// Battery recalibration: supported, phase, last result.
    pub async fn recalibration(&self) -> Result<Value> {
        self.json("Recalibration", &()).await
    }

    pub async fn start_recalibration(&self) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("StartRecalibration", &())
            .await
            .map_err(explain)
    }

    pub async fn stop_recalibration(&self) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("StopRecalibration", &())
            .await
            .map_err(explain)
    }

    /// What importing TLP's configuration would change, line by line.
    pub async fn tlp_import(&self) -> Result<Value> {
        self.json("TlpImport", &()).await
    }

    /// Apply the preview's lines at these positions.
    pub async fn apply_tlp_import(&self, lines: &[u32]) -> Result<Value> {
        let text: String = self
            .own()
            .await?
            .call("ApplyTlpImport", &(lines.to_vec(),))
            .await
            .map_err(explain)?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    /// "Learn now": phase, progress and results.
    pub async fn learn_session(&self) -> Result<Value> {
        self.json("LearnSession", &()).await
    }

    /// Start "Learn now" (`workload`: measure lag-costing settings with a
    /// built-in light load).
    pub async fn start_learn_session(&self, workload: bool) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("StartLearnSession", &(workload,))
            .await
            .map_err(explain)
    }

    pub async fn stop_learn_session(&self) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("StopLearnSession", &())
            .await
            .map_err(explain)
    }

    pub async fn undo(&self, setting: &str) -> Result<String> {
        self.own()
            .await?
            .call("Undo", &(setting,))
            .await
            .map_err(explain)
    }

    pub async fn restore(&self) -> Result<Value> {
        self.json("Restore", &()).await
    }

    pub async fn set_setting(&self, setting: &str, value: &str, allow_risky: bool) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetSetting", &(setting, value, allow_risky))
            .await
            .map_err(explain)
    }

    /// Returns why safe mode is still on ("" if it is off).
    pub async fn set_safe_mode(&self, on: bool) -> Result<String> {
        self.own()
            .await?
            .call("SetSafeMode", &(on,))
            .await
            .map_err(explain)
    }

    pub async fn forget(&self, setting: &str) -> Result<bool> {
        self.own()
            .await?
            .call("Forget", &(setting,))
            .await
            .map_err(explain)
    }

    /// Other power tools installed and running or set to start.
    pub async fn conflicts(&self) -> Result<Value> {
        self.json("Conflicts", &()).await
    }

    /// Switch off one known power tool (asks for an administrator password).
    pub async fn switch_off_conflict(&self, unit: &str) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SwitchOffConflict", &(unit,))
            .await
            .map_err(explain)
    }

    pub async fn auto_prefs(&self) -> Result<Value> {
        self.json("AutoPrefs", &()).await
    }

    pub async fn set_auto_prefs(&self, prefs: &Value) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetAutoPrefs", &(prefs.to_string(),))
            .await
            .map_err(explain)
    }

    /// Per-app energy (estimates), open apps, the app in front, the
    /// slow-down policy and per-app modes.
    pub async fn apps(&self) -> Result<Value> {
        self.json("Apps", &()).await
    }

    /// Change how apps share the CPU. Takes the `policy` object from `apps`.
    pub async fn set_app_policy(&self, policy: &Value) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetAppPolicy", &(policy.to_string(),))
            .await
            .map_err(explain)
    }

    /// The guided sleep test: what it is doing and its results.
    pub async fn sleep_test(&self) -> Result<Value> {
        self.json("SleepTest", &()).await
    }

    /// Start "Check deep idle" (one minute; the result is `deep_idle` in
    /// the status).
    pub async fn check_deep_idle(&self) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("CheckDeepIdle", &())
            .await
            .map_err(explain)
    }

    /// Start the sleep test, `minutes` per sleep type.
    pub async fn start_sleep_test(&self, minutes: f64) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("StartSleepTest", &(minutes,))
            .await
            .map_err(explain)
    }

    pub async fn stop_sleep_test(&self) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("StopSleepTest", &())
            .await
            .map_err(explain)
    }

    /// The full diagnostic report in plain text (`dynamis stat`).
    pub async fn diagnostics(&self) -> Result<String> {
        self.own()
            .await?
            .call("Diagnostics", &())
            .await
            .map_err(explain)
    }

    /// Devices that can sleep when idle, and the two switches.
    pub async fn devices(&self) -> Result<Value> {
        self.json("Devices", &()).await
    }

    /// Change the device switches (`{"usb": bool, "pci": bool}`).
    pub async fn set_device_policy(&self, policy: &Value) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetDevicePolicy", &(policy.to_string(),))
            .await
            .map_err(explain)
    }

    /// Call `on_change` each time the service signals a change (mode, auto
    /// mode, learning, anything shown in `Status`), until the connection
    /// ends. Lets the app refresh at once instead of waiting for its next
    /// look.
    pub async fn watch(&self, mut on_change: impl FnMut()) -> Result<()> {
        use futures_lite::StreamExt;
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .member("PropertiesChanged")
            .sender(NAME)
            .build()
            .map_err(explain)?;
        let mut stream = zbus::MessageStream::for_match_rule(rule, &self.conn, None)
            .await
            .map_err(explain)?;
        while let Some(m) = stream.next().await {
            m.map_err(explain)?;
            on_change();
        }
        Ok(())
    }

    /// Also learn speed settings during heavy work.
    pub async fn set_learn_during_heavy_work(&self, on: bool) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetLearnDuringHeavyWork", &(on,))
            .await
            .map_err(explain)
    }

    /// "Something stopped working": undo the last hour (step 1).
    pub async fn something_broke(&self) -> Result<Value> {
        self.json("SomethingBroke", &()).await
    }

    /// Step 2: what stopped working.
    pub async fn what_broke(&self, kind: &str) -> Result<Value> {
        self.json("WhatBroke", &(kind,)).await
    }

    /// Yes or no to the suggestion in `Status`.
    pub async fn answer_suggestion(&self, id: &str, yes: bool) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("AnswerSuggestion", &(id, yes))
            .await
            .map_err(explain)
    }

    /// The last sleeps: what woke the laptop and the battery used.
    pub async fn sleeps(&self) -> Result<Value> {
        self.json("Sleeps", &()).await
    }

    /// Radios and the rules that switch them off.
    pub async fn radios(&self) -> Result<Value> {
        self.json("Radios", &()).await
    }

    pub async fn set_radio_policy(&self, policy: &Value) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetRadioPolicy", &(policy.to_string(),))
            .await
            .map_err(explain)
    }

    /// The choices for acting through the desktop on battery.
    pub async fn set_desktop_policy(&self, policy: &Value) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetDesktopPolicy", &(policy.to_string(),))
            .await
            .map_err(explain)
    }

    /// The screen brightness chosen for each mode, and the level now.
    pub async fn brightness(&self) -> Result<Value> {
        self.json("Brightness", &()).await
    }

    /// Choose the screen brightness for each mode
    /// (`{"levels": {"power-saver": 40}}`).
    pub async fn set_brightness_prefs(&self, prefs: &Value) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetBrightnessPrefs", &(prefs.to_string(),))
            .await
            .map_err(explain)
    }

    pub async fn forget_app_usage(&self) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("ForgetAppUsage", &())
            .await
            .map_err(explain)
    }

    /// Battery forecast, usual unplug time, "Last until" and charging that
    /// follows habits.
    pub async fn predict(&self) -> Result<Value> {
        self.json("Predict", &()).await
    }

    /// "Last until" this Unix time; 0 ends it.
    pub async fn set_target(&self, until: f64) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetTarget", &(until,))
            .await
            .map_err(explain)
    }

    pub async fn set_adaptive_charging(&self, on: bool) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetAdaptiveCharging", &(on,))
            .await
            .map_err(explain)
    }

    /// "Charge fully once" on, or off (puts the limit back).
    pub async fn charge_fully_once(&self, on: bool) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("ChargeFullyOnce", &(on,))
            .await
            .map_err(explain)
    }

    /// Whether the panel icon is on. The icon belongs to the session agent,
    /// which runs as the person on the session bus, not the service.
    pub async fn tray_shown() -> Result<bool> {
        let session = zbus::Connection::session()
            .await
            .map_err(|e| e.to_string())?;
        let p = zbus::Proxy::new(&session, AGENT, AGENT_PATH, AGENT_IFACE)
            .await
            .map_err(|e| e.to_string())?;
        p.get_property::<bool>("Tray").await.map_err(|_| {
            "the Dynamis Power session helper is not running; log out and back in".to_string()
        })
    }

    /// Whether the desktop shows panel icons at all (a StatusNotifierItem
    /// host is running). GNOME does not without the AppIndicator extension.
    pub async fn tray_host() -> Result<bool> {
        let session = zbus::Connection::session()
            .await
            .map_err(|e| e.to_string())?;
        let dbus = zbus::fdo::DBusProxy::new(&session)
            .await
            .map_err(|e| e.to_string())?;
        let name = zbus::names::BusName::try_from("org.kde.StatusNotifierWatcher")
            .map_err(|e| e.to_string())?;
        dbus.name_has_owner(name).await.map_err(|e| e.to_string())
    }

    /// Show or hide the panel icon.
    pub async fn set_tray(on: bool) -> Result<()> {
        let session = zbus::Connection::session()
            .await
            .map_err(|e| e.to_string())?;
        session
            .call_method(
                Some(AGENT),
                AGENT_PATH,
                Some(AGENT_IFACE),
                "SetTray",
                &(on,),
            )
            .await
            .map(|_| ())
            .map_err(|_| {
                "the Dynamis Power session helper is not running; log out and back in".to_string()
            })
    }

    /// What each profile sets here, and the person's own choices.
    pub async fn profiles(&self) -> Result<Value> {
        self.json("Profiles", &()).await
    }

    pub async fn set_profile_overrides(&self, overrides: &Value) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetProfileOverrides", &(overrides.to_string(),))
            .await
            .map_err(explain)
    }

    /// Live sensor readings.
    pub async fn sensors(&self) -> Result<Value> {
        self.json("Sensors", &()).await
    }

    /// About the machine and the service.
    pub async fn system(&self) -> Result<Value> {
        self.json("System", &()).await
    }

    /// What would be set, as a "tlp" or "tuned" configuration.
    pub async fn advise(&self, format: &str) -> Result<String> {
        self.own()
            .await?
            .call::<_, _, String>("Advise", &(format,))
            .await
            .map_err(explain)
    }

    pub async fn set_dry_run(&self, on: bool) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetDryRun", &(on,))
            .await
            .map_err(explain)
    }

    /// Keep a setting's current value after restarts and sleep, or stop.
    pub async fn set_kept(&self, setting: &str, keep: bool) -> Result<()> {
        self.own()
            .await?
            .call::<_, _, ()>("SetKept", &(setting, keep))
            .await
            .map_err(explain)
    }
}

const AGENT: &str = "net.dynamis.Agent";
const AGENT_PATH: &str = "/net/dynamis/Agent";
const AGENT_IFACE: &str = "net.dynamis.Agent1";
