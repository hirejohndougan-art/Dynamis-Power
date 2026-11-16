//! Passing the brightness the service set on to the desktop.
//!
//! The service sets the screen brightness chosen for each mode (it writes the
//! kernel's backlight file). KDE Plasma keeps its own idea of the brightness:
//! without being told, its slider shows the old value and it may put the old
//! value back later (after dimming, or at the next login). So when the
//! service's count of brightness changes goes up, the agent tells Plasma the
//! same value:
//!
//! - **Plasma 6.1 and later:** `org.kde.ScreenBrightness`, the built-in
//!   display's `SetBrightness(value, flags)` with flag 1 (no on-screen
//!   indicator: the change was not a key press).
//! - **Older Plasma:** `org.kde.Solid.PowerManagement`'s brightness control,
//!   `setBrightnessSilent(value)`.
//!
//! Other desktops read the kernel's value, so nothing is needed there. Both
//! KDE interfaces still need confirming on real hardware.

use serde_json::Value;
use zbus::Connection;

use crate::{SERVICE, SERVICE_IFACE, SERVICE_PATH};

/// No on-screen brightness indicator (Plasma's `SuppressIndicator`).
const SUPPRESS_INDICATOR: u32 = 1;

#[derive(Default)]
pub struct Follower {
    /// The service's count at the last look; `None` before the first.
    seen: Option<u64>,
}

impl Follower {
    /// Look at the service's brightness count; if it went up, tell the
    /// desktop. The first look only takes the count (no replay at login).
    pub async fn check(&mut self, service: &Connection, session: &Connection, verbose: bool) {
        let Some(v) = brightness(service).await else {
            return;
        };
        let seq = v["set_seq"].as_u64().unwrap_or(0);
        let first = self.seen.is_none();
        if self.seen == Some(seq) {
            return;
        }
        self.seen = Some(seq);
        let Some(pct) = v["set_pct"].as_u64().filter(|_| !first) else {
            return;
        };
        match tell_desktop(session, pct.min(100) as u32).await {
            Ok(Some(how)) if verbose => {
                eprintln!("dynamis-agent: brightness {pct}% passed to {how}")
            }
            Ok(_) => {}
            Err(e) if verbose => eprintln!("dynamis-agent: brightness: {e}"),
            Err(_) => {}
        }
    }
}

async fn brightness(service: &Connection) -> Option<Value> {
    let m = service
        .call_method(
            Some(SERVICE),
            SERVICE_PATH,
            Some(SERVICE_IFACE),
            "Brightness",
            &(),
        )
        .await
        .ok()?;
    serde_json::from_str(&m.body().deserialize::<String>().ok()?).ok()
}

/// A raw value for a percentage of `max`.
pub fn raw(max: i32, pct: u32) -> i32 {
    ((f64::from(max.max(0)) * f64::from(pct) / 100.0).round() as i32).clamp(0, max.max(0))
}

/// Tell the desktop; `Ok(None)` if it is not one that needs telling.
async fn tell_desktop(session: &Connection, pct: u32) -> zbus::Result<Option<&'static str>> {
    if plasma_6(session, pct).await? {
        return Ok(Some("Plasma"));
    }
    if plasma_5(session, pct).await? {
        return Ok(Some("Plasma (older)"));
    }
    Ok(None)
}

async fn proxy<'a>(
    session: &Connection,
    dest: &'static str,
    path: String,
    iface: &'static str,
) -> zbus::Result<zbus::Proxy<'a>> {
    zbus::proxy::Builder::new(session)
        .destination(dest)
        .path(path)
        .interface(iface)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

fn missing(e: &zbus::Error) -> bool {
    let t = e.to_string();
    t.contains("ServiceUnknown")
        || t.contains("UnknownObject")
        || t.contains("UnknownMethod")
        || t.contains("UnknownInterface")
}

async fn plasma_6(session: &Connection, pct: u32) -> zbus::Result<bool> {
    const DEST: &str = "org.kde.ScreenBrightness";
    let top = proxy(session, DEST, "/org/kde/ScreenBrightness".into(), DEST).await?;
    let names: Vec<String> = match top.get_property("DisplaysDBusNames").await {
        Ok(n) => n,
        Err(e) if missing(&e) => return Ok(false),
        Err(e) => return Err(e),
    };
    for n in names {
        let d = proxy(
            session,
            DEST,
            format!("/org/kde/ScreenBrightness/{n}"),
            "org.kde.ScreenBrightness.Display",
        )
        .await?;
        // The laptop's own screen only: the service sets that one.
        if !d.get_property::<bool>("IsInternal").await.unwrap_or(false) {
            continue;
        }
        let max: i32 = d.get_property("MaxBrightness").await?;
        d.call::<_, _, ()>("SetBrightness", &(raw(max, pct), SUPPRESS_INDICATOR))
            .await?;
        return Ok(true);
    }
    Ok(false)
}

async fn plasma_5(session: &Connection, pct: u32) -> zbus::Result<bool> {
    let c = proxy(
        session,
        "org.kde.Solid.PowerManagement",
        "/org/kde/Solid/PowerManagement/Actions/BrightnessControl".into(),
        "org.kde.Solid.PowerManagement.Actions.BrightnessControl",
    )
    .await?;
    let max: i32 = match c.call("brightnessMax", &()).await {
        Ok(m) => m,
        Err(e) if missing(&e) => return Ok(false),
        Err(e) => return Err(e),
    };
    c.call::<_, _, ()>("setBrightnessSilent", &(raw(max, pct),))
        .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_to_raw() {
        assert_eq!(raw(1000, 40), 400);
        assert_eq!(raw(255, 100), 255);
        assert_eq!(raw(96000, 33), 31680);
        assert_eq!(raw(-1, 50), 0);
    }
}
