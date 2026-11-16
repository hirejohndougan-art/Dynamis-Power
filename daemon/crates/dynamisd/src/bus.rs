//! The standard power-profiles D-Bus interface.
//!
//! Desktops (KDE's battery applet, GNOME's quick settings) and tools like
//! `powerprofilesctl` talk to this interface. It is published under two names,
//! the current one and the older `net.hadess` one, exactly as other
//! implementations do. This is a fresh implementation of the public interface;
//! see docs/decisions/0004-standard-power-profiles-interface.md.
//!
//! Rust note: `#[zbus::interface]` is a macro that turns an `impl` block into a
//! D-Bus object. Methods marked `property` become D-Bus properties, plain
//! methods become D-Bus methods. The interface name must be a literal, so a
//! small `macro_rules!` stamps out the same block twice, once per name.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use dynamis_core::profiles::{self, Profile};
use zbus::fdo;
use zbus::message::Header;
use zbus::names::InterfaceName;
use zbus::object_server::SignalEmitter;
use zbus::{Connection, OwnedVariant, Str, Variant};

use crate::core::{Core, SessionReport};

pub type Shared = Arc<Mutex<Core>>;

pub const BUS_NAMES: [&str; 2] = [
    "org.freedesktop.UPower.PowerProfiles",
    "net.hadess.PowerProfiles",
];
pub const OBJECTS: [(&str, &str); 2] = [
    (
        "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles",
    ),
    ("/net/hadess/PowerProfiles", "net.hadess.PowerProfiles"),
];

/// Polkit actions, defined in dist/net.dynamis.power.policy.
const ACTION_SWITCH: &str = "net.dynamis.power.switch-profile";
const ACTION_HOLD: &str = "net.dynamis.power.hold-profile";
const ACTION_REPORT: &str = "net.dynamis.power.report-session";
/// Switching off another power tool: needs an administrator password.
const ACTION_CONFLICTS: &str = "net.dynamis.power.manage-conflicts";

/// Dynamis Power's own interface: auto mode, and reports from the session agent.
pub const OWN_NAME: &str = "net.dynamis.Power";
pub const OWN_PATH: &str = "/net/dynamis/Power";
pub const OWN_IFACE: &str = "net.dynamis.Power1";

/// Shared by both interface objects.
#[derive(Clone)]
pub struct Service {
    pub core: Shared,
    /// Ask polkit before changes. Off only for tests on a private session bus.
    pub polkit: bool,
    /// Wakes the auto-mode thread early (a session report, auto mode switched on).
    pub ticker: Arc<Ticker>,
    /// The clock auto mode measures time with.
    pub epoch: Instant,
}

/// Lets D-Bus handlers wake the auto-mode thread without waiting for its
/// next reading.
#[derive(Default)]
pub struct Ticker {
    woken: Mutex<bool>,
    cv: Condvar,
}

impl Ticker {
    pub fn poke(&self) {
        *self.woken.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.cv.notify_one();
    }

    /// Sleep until poked, or until `timeout` passes (`None`: until poked).
    /// This is the only place the daemon waits on a timer, and only while
    /// auto mode is on.
    pub fn wait(&self, timeout: Option<Duration>) {
        let mut woken = self.woken.lock().unwrap_or_else(|p| p.into_inner());
        if !*woken {
            woken = match timeout {
                Some(t) => {
                    self.cv
                        .wait_timeout(woken, t)
                        .unwrap_or_else(|p| p.into_inner())
                        .0
                }
                None => self.cv.wait(woken).unwrap_or_else(|p| p.into_inner()),
            };
        }
        *woken = false;
    }
}

impl Service {
    fn lock(&self) -> MutexGuard<'_, Core> {
        // A panic while holding the lock leaves a poisoned mutex; the state
        // inside is still consistent enough to keep serving, so use it.
        self.core.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn profiles(&self) -> Vec<HashMap<String, OwnedVariant>> {
        let core = self.lock();
        let (cpu, platform) = profiles::drivers(core.root());
        core.available()
            .iter()
            .map(|p| {
                let mut m = HashMap::new();
                m.insert("Profile".into(), sv(p.as_str()));
                m.insert("Driver".into(), sv("dynamis"));
                if let Some(c) = &cpu {
                    m.insert("CpuDriver".into(), sv(c));
                }
                if let Some(pl) = &platform {
                    m.insert("PlatformDriver".into(), sv(pl));
                }
                m
            })
            .collect()
    }

    fn holds(&self) -> Vec<HashMap<String, OwnedVariant>> {
        self.lock()
            .holds()
            .values()
            .map(|h| {
                HashMap::from([
                    ("Profile".to_string(), sv(h.profile.as_str())),
                    ("Reason".to_string(), sv(&h.reason)),
                    ("ApplicationId".to_string(), sv(&h.application_id)),
                ])
            })
            .collect()
    }

    async fn check(
        &self,
        conn: &Connection,
        hdr: Option<&Header<'_>>,
        action: &str,
    ) -> fdo::Result<String> {
        let sender = hdr
            .and_then(|h| h.sender())
            .map(|s| s.to_string())
            .ok_or_else(|| fdo::Error::AccessDenied("unknown caller".into()))?;
        if self.polkit && !polkit_allows(conn, &sender, action).await {
            return Err(fdo::Error::AccessDenied(format!(
                "not authorised for {action}"
            )));
        }
        Ok(sender)
    }

    async fn set_active(
        &self,
        conn: &Connection,
        hdr: Option<&Header<'_>>,
        profile: &str,
    ) -> fdo::Result<()> {
        self.check(conn, hdr, ACTION_SWITCH).await?;
        let p = Profile::parse(profile)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown profile '{profile}'")))?;
        let released = self.lock().choose(p).map_err(fdo::Error::Failed)?;
        for cookie in released {
            emit_released(conn, cookie).await;
        }
        notify(conn, self).await;
        Ok(())
    }

    async fn hold(
        &self,
        conn: &Connection,
        hdr: &Header<'_>,
        profile: &str,
        reason: &str,
        app: &str,
    ) -> fdo::Result<u32> {
        let sender = self.check(conn, Some(hdr), ACTION_HOLD).await?;
        let p = Profile::parse(profile)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown profile '{profile}'")))?;
        let cookie = self
            .lock()
            .hold(p, reason, app, &sender)
            .map_err(fdo::Error::InvalidArgs)?;
        notify(conn, self).await;
        Ok(cookie)
    }

    async fn release(&self, conn: &Connection, hdr: &Header<'_>, cookie: u32) -> fdo::Result<()> {
        let sender = hdr
            .sender()
            .map(|s| s.to_string())
            .ok_or_else(|| fdo::Error::AccessDenied("unknown caller".into()))?;
        self.lock()
            .release(cookie, &sender)
            .map_err(fdo::Error::InvalidArgs)?;
        notify(conn, self).await;
        Ok(())
    }
}

fn sv(s: &str) -> OwnedVariant {
    Str::from(s.to_string()).into()
}

macro_rules! power_profiles_object {
    ($ty:ident, $iface:literal) => {
        pub struct $ty(pub Service);

        #[zbus::interface(name = $iface)]
        impl $ty {
            #[zbus(property(emits_changed_signal = "false"))]
            fn active_profile(&self) -> String {
                self.0.lock().active().as_str().to_string()
            }

            #[zbus(property)]
            async fn set_active_profile(
                &mut self,
                profile: String,
                #[zbus(connection)] conn: &Connection,
                #[zbus(header)] hdr: Option<Header<'_>>,
            ) -> fdo::Result<()> {
                self.0.set_active(conn, hdr.as_ref(), &profile).await
            }

            /// Deprecated in the standard; always empty.
            #[zbus(property(emits_changed_signal = "const"))]
            fn performance_inhibited(&self) -> String {
                String::new()
            }

            /// Why performance is limited (for example overheating). Not
            /// detected yet, so always empty.
            #[zbus(property(emits_changed_signal = "false"))]
            fn performance_degraded(&self) -> String {
                String::new()
            }

            #[zbus(property(emits_changed_signal = "const"))]
            fn profiles(&self) -> Vec<HashMap<String, OwnedVariant>> {
                self.0.profiles()
            }

            #[zbus(property(emits_changed_signal = "const"))]
            fn actions(&self) -> Vec<String> {
                Vec::new()
            }

            #[zbus(property(emits_changed_signal = "false"))]
            fn active_profile_holds(&self) -> Vec<HashMap<String, OwnedVariant>> {
                self.0.holds()
            }

            #[zbus(property(emits_changed_signal = "const"))]
            fn version(&self) -> String {
                concat!("dynamis ", env!("CARGO_PKG_VERSION")).to_string()
            }

            async fn hold_profile(
                &self,
                profile: String,
                reason: String,
                application_id: String,
                #[zbus(connection)] conn: &Connection,
                #[zbus(header)] hdr: Header<'_>,
            ) -> fdo::Result<u32> {
                self.0
                    .hold(conn, &hdr, &profile, &reason, &application_id)
                    .await
            }

            async fn release_profile(
                &self,
                cookie: u32,
                #[zbus(connection)] conn: &Connection,
                #[zbus(header)] hdr: Header<'_>,
            ) -> fdo::Result<()> {
                self.0.release(conn, &hdr, cookie).await
            }
        }
    };
}

power_profiles_object!(UPowerProfiles, "org.freedesktop.UPower.PowerProfiles");
power_profiles_object!(HadessProfiles, "net.hadess.PowerProfiles");

/// Tell listeners on both objects that the active profile and holds changed.
/// Emitted by hand (rather than by zbus's setter) so both objects stay in step
/// whichever one was used.
pub async fn notify(conn: &Connection, svc: &Service) {
    let active = svc.lock().active().as_str();
    for (path, iface) in OBJECTS {
        let result = async {
            let emitter = SignalEmitter::new(conn, path)?;
            let changed = HashMap::from([("ActiveProfile", Variant::from(active))]);
            fdo::Properties::properties_changed(
                &emitter,
                InterfaceName::try_from(iface)?,
                changed,
                Cow::Borrowed(&["ActiveProfileHolds"]),
            )
            .await
        }
        .await;
        if let Err(e) = result {
            eprintln!("dynamisd: could not signal a change on {path}: {e}");
        }
    }
}

/// The standard's `ProfileReleased` signal, on both objects.
pub async fn emit_released(conn: &Connection, cookie: u32) {
    for (path, iface) in OBJECTS {
        let result = async {
            SignalEmitter::new(conn, path)?
                .emit(iface, "ProfileReleased", &(cookie,))
                .await
        }
        .await;
        if let Err(e) = result {
            eprintln!("dynamisd: could not signal ProfileReleased on {path}: {e}");
        }
    }
}

/// Ask polkit whether `sender` may perform `action`. Any failure to ask counts
/// as no: changing settings must never be allowed by accident.
async fn polkit_allows(conn: &Connection, sender: &str, action: &str) -> bool {
    let subject = (
        "system-bus-name",
        HashMap::from([("name", Variant::from(sender))]),
    );
    let details: HashMap<&str, &str> = HashMap::new();
    // Flag 1 = allow polkit to show a password prompt if the policy asks for one.
    let reply = conn
        .call_method(
            Some("org.freedesktop.PolicyKit1"),
            "/org/freedesktop/PolicyKit1/Authority",
            Some("org.freedesktop.PolicyKit1.Authority"),
            "CheckAuthorization",
            &(subject, action, details, 1u32, ""),
        )
        .await;
    match reply.map(|m| {
        m.body()
            .deserialize::<(bool, bool, HashMap<String, String>)>()
    }) {
        Ok(Ok((authorised, _, _))) => authorised,
        Ok(Err(e)) | Err(e) => {
            eprintln!("dynamisd: polkit check for {action} failed: {e}");
            false
        }
    }
}

/// `net.dynamis.Power1`: what the app (later) and `dynamis auto` use.
pub struct DynamisPower(pub Service);

#[zbus::interface(name = "net.dynamis.Power1")]
impl DynamisPower {
    /// Whether auto mode chooses the profile.
    #[zbus(property(emits_changed_signal = "false"))]
    fn auto_mode(&self) -> bool {
        self.0.lock().auto.enabled
    }

    #[zbus(property)]
    async fn set_auto_mode(
        &mut self,
        on: bool,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Option<Header<'_>>,
    ) -> fdo::Result<()> {
        self.0.check(conn, hdr.as_ref(), ACTION_SWITCH).await?;
        let now = self.0.epoch.elapsed().as_secs_f64();
        self.0
            .lock()
            .set_auto(on, now)
            .map_err(fdo::Error::Failed)?;
        self.0.ticker.poke();
        notify_auto(conn, &self.0).await;
        Ok(())
    }

    /// The detected workload class (`light_interactive`, `video`, …), or
    /// empty while auto mode is off.
    #[zbus(property(emits_changed_signal = "false"))]
    fn workload(&self) -> String {
        self.0
            .lock()
            .auto_status()
            .0
            .map(|w| {
                serde_json::to_value(w)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    /// One plain-language line on what auto mode is doing and why.
    #[zbus(property(emits_changed_signal = "false"))]
    fn auto_reason(&self) -> String {
        self.0.lock().auto_status().1
    }

    /// Whether Dynamis Power runs calibration tests at quiet moments on
    /// battery, and applies what it learned (on battery only).
    #[zbus(property(emits_changed_signal = "false"))]
    fn learning(&self) -> bool {
        self.0.lock().learn.enabled
    }

    #[zbus(property)]
    async fn set_learning(
        &mut self,
        on: bool,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Option<Header<'_>>,
    ) -> fdo::Result<()> {
        self.0.check(conn, hdr.as_ref(), ACTION_SWITCH).await?;
        self.0.lock().set_learning(on).map_err(fdo::Error::Failed)?;
        self.0.ticker.poke();
        notify_auto(conn, &self.0).await;
        Ok(())
    }

    /// One plain-language line on what learning is doing.
    #[zbus(property(emits_changed_signal = "false"))]
    fn learning_status(&self) -> String {
        self.0.lock().learn.status()
    }

    // --- For the app. Reads need no permission; changes ask polkit. -------

    /// Status summary for the app (JSON).
    fn status(&self) -> String {
        self.0.lock().status_json().to_string()
    }

    /// A full diagnostic report in plain text, like `tlp-stat` (also
    /// `dynamis stat`). Contains no serial numbers or personal data.
    fn diagnostics(&self) -> String {
        let c = self.0.lock();
        let status = c.status_json();
        dynamis_core::diag::report(
            &c.root,
            Some(&c.state_dir),
            Some(&status),
            env!("CARGO_PKG_VERSION"),
        )
    }

    /// The last `n` journal entries, newest first (JSON).
    fn journal(&self, n: u32) -> fdo::Result<String> {
        self.0
            .lock()
            .journal_json(n as usize)
            .map(|v| v.to_string())
            .map_err(fdo::Error::Failed)
    }

    /// Every setting with its current and original value (JSON).
    fn settings(&self) -> fdo::Result<String> {
        self.0
            .lock()
            .settings_json()
            .map(|v| v.to_string())
            .map_err(fdo::Error::Failed)
    }

    /// Hardware summary (no serial numbers) for exported logs (JSON).
    fn inventory(&self) -> String {
        self.0.lock().inventory_json().to_string()
    }

    /// Put one setting back to its original value. Returns a sentence.
    async fn undo(
        &self,
        setting: String,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<String> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        let r = self.0.lock().undo(&setting).map_err(fdo::Error::Failed);
        notify_auto(conn, &self.0).await;
        r
    }

    /// Restore every original setting; also switches auto mode and learning
    /// off. Works in safe mode. Returns what was restored (JSON).
    async fn restore(
        &self,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<String> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        let now = self.0.epoch.elapsed().as_secs_f64();
        let r = self
            .0
            .lock()
            .restore_all(now)
            .map(|v| v.to_string())
            .map_err(fdo::Error::Failed);
        notify(conn, &self.0).await;
        notify_auto(conn, &self.0).await;
        r
    }

    /// Change one setting (Advanced). `allow_risky` must be true for risky
    /// and experimental settings; the app sends it only after an explicit yes.
    async fn set_setting(
        &self,
        setting: String,
        value: String,
        allow_risky: bool,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        self.0
            .lock()
            .set_setting(&setting, &value, allow_risky)
            .map_err(fdo::Error::Failed)
    }

    /// Keep a setting's current value after restarts and sleep (true), or
    /// stop keeping it (false). The battery's limits are always kept.
    async fn set_kept(
        &self,
        setting: String,
        keep: bool,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        self.0
            .lock()
            .set_kept(&setting, keep)
            .map_err(fdo::Error::Failed)
    }

    /// Switch safe mode on or off. Returns why it is still on, or "".
    async fn set_safe_mode(
        &self,
        on: bool,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<String> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        self.0
            .lock()
            .set_safe_mode(on)
            .map(|w| w.unwrap_or_default())
            .map_err(fdo::Error::Failed)
    }

    /// Other power tools installed and running or set to start (JSON).
    async fn conflicts(&self, #[zbus(connection)] conn: &Connection) -> String {
        serde_json::to_string(&crate::conflicts::find(conn).await).unwrap_or_else(|_| "[]".into())
    }

    /// Stop and disable one known power tool (needs an administrator
    /// password). Removing Dynamis Power turns it back on.
    async fn switch_off_conflict(
        &self,
        unit: String,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_CONFLICTS).await?;
        let dir = self.0.lock().state_dir.clone();
        crate::conflicts::switch_off(conn, &unit, &dir)
            .await
            .map_err(fdo::Error::Failed)
    }

    /// Auto mode's preferences (JSON).
    fn auto_prefs(&self) -> String {
        serde_json::to_string(self.0.lock().auto_prefs()).unwrap_or_default()
    }

    /// Change auto mode's preferences. Takes the same JSON `AutoPrefs`
    /// returns; out-of-range values are refused with a sentence saying why.
    async fn set_auto_prefs(
        &self,
        prefs: String,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        let p: dynamis_core::auto::Prefs =
            serde_json::from_str(&prefs).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        let now = self.0.epoch.elapsed().as_secs_f64();
        self.0
            .lock()
            .set_auto_prefs(p, now)
            .map_err(fdo::Error::InvalidArgs)?;
        self.0.ticker.poke();
        Ok(())
    }

    /// Apps: energy today and this week (estimates), what is open now, the
    /// app in front, the slow-down policy and per-app modes (JSON).
    fn apps(&self) -> String {
        let c = self.0.lock();
        c.apps
            .json(crate::core::unix_now(), &c.auto_prefs().app_rules)
            .to_string()
    }

    /// Change how apps share the CPU. Takes the `policy` object from `Apps`.
    async fn set_app_policy(
        &self,
        policy: String,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        let p: crate::apps::AppPolicy =
            serde_json::from_str(&policy).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        self.0
            .lock()
            .set_app_policy(p)
            .map_err(fdo::Error::InvalidArgs)?;
        self.0.ticker.poke();
        Ok(())
    }

    /// Devices that can sleep when idle: the two switches and every USB
    /// device and PCI function, with whether each is managed or kept awake
    /// and why (JSON).
    fn devices(&self) -> String {
        let c = self.0.lock();
        c.devices.json(&c.root, &c.state_dir).to_string()
    }

    /// Change the device switches. Takes the `policy` object from `Devices`.
    async fn set_device_policy(
        &self,
        policy: String,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        let p: crate::devices::DevicePolicy =
            serde_json::from_str(&policy).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        self.0
            .lock()
            .set_device_policy(p)
            .map_err(fdo::Error::InvalidArgs)?;
        self.0.ticker.poke();
        Ok(())
    }

    /// The screen brightness chosen for each mode, whether this screen's
    /// brightness can be set, and its level now (JSON).
    fn brightness(&self) -> String {
        let c = self.0.lock();
        c.brightness.json(&c.root).to_string()
    }

    /// Choose the screen brightness for each mode. Takes
    /// `{"levels": {"power-saver": 40, ...}}`; a mode left out is left
    /// alone. A new level for the mode in use is set at once.
    async fn set_brightness_prefs(
        &self,
        prefs: String,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        let p: crate::brightness::BrightnessPrefs =
            serde_json::from_str(&prefs).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        self.0
            .lock()
            .set_brightness_prefs(p)
            .map_err(fdo::Error::InvalidArgs)?;
        Ok(())
    }

    /// Forget every per-app energy total.
    async fn forget_app_usage(
        &self,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        let mut c = self.0.lock();
        let dir = c.state_dir.clone();
        c.apps.forget_usage(&dir, crate::core::unix_now());
        Ok(())
    }

    /// Predictions for the Battery screen: when the battery will run out at
    /// your usual use, when you usually unplug, "Last until" and charging
    /// that follows habits (JSON; times are Unix seconds).
    fn predict(&self) -> String {
        let c = self.0.lock();
        c.predict
            .json(c.root(), crate::core::unix_now())
            .to_string()
    }

    /// "Last until": save as much as needed for the battery to last until
    /// `until` (Unix seconds). 0 ends it.
    async fn set_target(
        &self,
        until: f64,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        let r = {
            let mut c = self.0.lock();
            let dir = c.state_dir.clone();
            let ended = until <= 0.0;
            let r = c
                .predict
                .set_target((!ended).then_some(until), crate::core::unix_now(), &dir);
            if r.is_ok() && ended {
                // Ending lifts the cap at once.
                if let Err(e) = c.apply_active("Last until ended") {
                    eprintln!("dynamisd: {e}");
                }
                c.apps.forced_slow = false;
            }
            r
        };
        r.map_err(fdo::Error::InvalidArgs)?;
        self.0.ticker.poke();
        notify(conn, &self.0).await;
        Ok(())
    }

    /// The guided sleep test: what it is doing and its results (JSON).
    fn sleep_test(&self) -> String {
        let c = self.0.lock();
        c.sleeptest.json(&c.root).to_string()
    }

    /// Start the sleep test: the laptop sleeps `minutes` (5–60) with each
    /// sleep type it offers and wakes by itself. Needs the battery.
    async fn start_sleep_test(
        &self,
        minutes: f64,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        self.0
            .lock()
            .start_sleep_test(minutes)
            .map_err(fdo::Error::Failed)?;
        self.0.ticker.poke();
        Ok(())
    }

    /// Stop the sleep test and put the original sleep type back.
    async fn stop_sleep_test(
        &self,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        let mut c = self.0.lock();
        let (root, dir) = (c.root().clone(), c.state_dir.clone());
        c.sleeptest.stop(&root, &dir);
        Ok(())
    }

    /// "Charge fully once": lift the charge limit until the battery is full
    /// or the charger is unplugged (`true`), or stop and put it back.
    async fn charge_fully_once(
        &self,
        on: bool,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        {
            let mut c = self.0.lock();
            let (root, dir) = (c.root().clone(), c.state_dir.clone());
            let on_ac = dynamis_core::sensors::power::read(&root).on_ac();
            let now = crate::core::unix_now();
            if on {
                c.fullcharge.start(&root, &dir, now, on_ac)
            } else {
                c.fullcharge.stop(&root, &dir, "you stopped it")
            }
            .map_err(fdo::Error::Failed)?;
            c.predict.hold_charge = c.fullcharge.is_active();
        }
        self.0.ticker.poke();
        Ok(())
    }

    /// Charging that follows when you usually unplug (where the battery has
    /// a charge limit). Off puts the original limit back.
    async fn set_adaptive_charging(
        &self,
        on: bool,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        {
            let mut c = self.0.lock();
            let (root, dir) = (c.root().clone(), c.state_dir.clone());
            c.predict
                .set_adaptive_charging(on, &root, &dir)
                .map_err(fdo::Error::Failed)?;
        }
        self.0.ticker.poke();
        Ok(())
    }

    /// What each profile sets on this machine, and the person's own choices
    /// (Advanced → Profiles), as JSON.
    fn profiles(&self) -> String {
        self.0.lock().profiles_json().to_string()
    }

    /// Replace the person's profile choices. Takes the `overrides` object
    /// from `Profiles`; values must be among those offered here.
    async fn set_profile_overrides(
        &self,
        overrides: String,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        let ov: profiles::Overrides =
            serde_json::from_str(&overrides).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        self.0
            .lock()
            .set_profile_overrides(ov)
            .map_err(fdo::Error::InvalidArgs)?;
        notify(conn, &self.0).await;
        Ok(())
    }

    /// Live sensor readings (JSON). Reads only.
    fn sensors(&self) -> String {
        self.0.lock().sensors_json().to_string()
    }

    /// About this machine and the service (JSON).
    fn system(&self) -> String {
        let up = self.0.epoch.elapsed().as_secs_f64();
        self.0.lock().system_json(up).to_string()
    }

    /// Advisor mode: what would be set, as a "tlp" or "tuned" configuration
    /// (text only; nothing is written anywhere).
    fn advise(&self, format: String) -> fdo::Result<String> {
        self.0
            .lock()
            .advise(&format)
            .map_err(fdo::Error::InvalidArgs)
    }

    /// Dry-run mode: changes are journaled as "would" and not made.
    async fn set_dry_run(
        &self,
        on: bool,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        self.0.lock().set_dry_run(on).map_err(fdo::Error::Failed)
    }

    /// Every calibration result and the display model, as JSON.
    fn learned_json(&self) -> String {
        let c = self.0.lock();
        serde_json::json!({
            "records": c.learn.records,
            "learned": c.learn.learned(),
            "display": c.learn.display.fit(),
        })
        .to_string()
    }

    /// Forget what was learned about a setting and put its original value
    /// back. Returns false if nothing was learned about it.
    async fn forget(
        &self,
        setting: String,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<bool> {
        self.0.check(conn, Some(&hdr), ACTION_SWITCH).await?;
        self.0.lock().forget(&setting).map_err(fdo::Error::Failed)
    }

    /// Called by the session agent when something in the desktop session
    /// changes. Keys: media_playing, screen_inhibited, game_running,
    /// screen_locked, user_idle (booleans) and focused_pid (u32, the process
    /// of the window in front; 0 if unknown). Unknown keys are ignored.
    async fn report_session(
        &self,
        report: HashMap<String, OwnedVariant>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> fdo::Result<()> {
        let sender = self.0.check(conn, Some(&hdr), ACTION_REPORT).await?;
        let flag = |k: &str| {
            report
                .get(k)
                .and_then(|v| bool::try_from(&**v).ok())
                .unwrap_or(false)
        };
        let r = SessionReport {
            media_playing: flag("media_playing"),
            screen_inhibited: flag("screen_inhibited"),
            game_running: flag("game_running"),
            screen_locked: flag("screen_locked"),
            user_idle: flag("user_idle"),
            focused_pid: report
                .get("focused_pid")
                .and_then(|v| u32::try_from(&**v).ok())
                .unwrap_or(0),
        };
        self.0.lock().report_session(&sender, r);
        self.0.ticker.poke();
        Ok(())
    }
}

/// Signal changes to auto mode's properties.
pub async fn notify_auto(conn: &Connection, svc: &Service) {
    let (enabled, (workload, reason), learning, learning_status) = {
        let c = svc.lock();
        (
            c.auto.enabled,
            c.auto_status(),
            c.learn.enabled,
            c.learn.status(),
        )
    };
    let workload = workload
        .and_then(|w| serde_json::to_value(w).ok())
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    let result = async {
        let emitter = SignalEmitter::new(conn, OWN_PATH)?;
        let changed = HashMap::from([
            ("AutoMode", Variant::from(enabled)),
            ("Workload", Variant::from(workload.as_str())),
            ("AutoReason", Variant::from(reason.as_str())),
            ("Learning", Variant::from(learning)),
            ("LearningStatus", Variant::from(learning_status.as_str())),
        ]);
        fdo::Properties::properties_changed(
            &emitter,
            InterfaceName::try_from(OWN_IFACE)?,
            changed,
            Cow::Borrowed(&[]),
        )
        .await
    }
    .await;
    if let Err(e) = result {
        eprintln!("dynamisd: could not signal an auto-mode change: {e}");
    }
}
