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
use std::sync::{Arc, Mutex, MutexGuard};

use dynamis_core::profiles::{self, Profile};
use zbus::fdo;
use zbus::message::Header;
use zbus::names::InterfaceName;
use zbus::object_server::SignalEmitter;
use zbus::{Connection, OwnedVariant, Str, Variant};

use crate::core::Core;

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

/// Shared by both interface objects.
#[derive(Clone)]
pub struct Service {
    pub core: Shared,
    /// Ask polkit before changes. Off only for tests on a private session bus.
    pub polkit: bool,
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
