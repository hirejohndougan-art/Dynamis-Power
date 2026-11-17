//! Acting through the desktop (decision 0027): the service decides, the
//! agent acts and puts back.
//!
//! The service's `Desktop()` says what it wants now (on battery, from the
//! person's choices). The agent acts only when that changes, so a change the
//! person makes by hand in between is left alone until the next charger
//! change:
//!
//! - **File indexing** paused. KDE: Baloo's scheduler (`suspend`/`resume`),
//!   only if Baloo was not already suspended by the person. GNOME:
//!   LocalSearch (formerly Tracker) `Pause`/`Resume`; the indexer itself
//!   resumes if the agent goes away.
//! - **Refresh rate**: the built-in screen goes to the slowest mode at the
//!   same size that is still 60 Hz or more. KDE: `kscreen-doctor` (saved by
//!   the desktop, so put back). GNOME: Mutter's display configuration,
//!   applied as temporary.
//! - **Dark style**. KDE: `plasma-apply-colorscheme BreezeDark`. GNOME:
//!   the `color-scheme` setting. Both saved by the desktop, so put back.
//!
//! What was changed is written to `~/.local/state/dynamis-power/desktop.json`
//! before acting, so if the agent is killed it is put back at the next start.
//! Putting back leaves anything the person changed since: only a value that
//! is still the one the agent set is put back.
//!
//! None of this has been confirmed on a real desktop yet (hardware tests).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zbus::{Connection, OwnedVariant, Variant};

use crate::{SERVICE, SERVICE_IFACE, SERVICE_PATH};

const VERSION: u32 = 1;
const DARK_KDE: &str = "BreezeDark";
/// The lowest refresh rate the agent picks.
const MIN_HZ: f64 = 59.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Kde,
    Gnome,
    Other,
}

impl Kind {
    fn detect() -> Kind {
        let d = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_ascii_uppercase();
        if d.split(':').any(|p| p == "KDE") {
            Kind::Kde
        } else if d.split(':').any(|p| p == "GNOME") {
            Kind::Gnome
        } else {
            Kind::Other
        }
    }
    fn name(self) -> &'static str {
        match self {
            Kind::Kde => "kde",
            Kind::Gnome => "gnome",
            Kind::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
struct Want {
    #[serde(default)]
    pause_indexing: bool,
    #[serde(default)]
    lower_refresh: bool,
    #[serde(default)]
    dark_style: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Changed {
    /// Output (KDE) or connector (GNOME).
    output: String,
    /// Mode id, or style name, before and as set.
    before: String,
    set: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Applied {
    v: u32,
    #[serde(default)]
    baloo: bool,
    #[serde(default)]
    refresh: Option<Changed>,
    #[serde(default)]
    style: Option<Changed>,
}

pub struct Desktop {
    kind: Kind,
    applied: Applied,
    last: Option<Want>,
    /// GNOME indexer pause: (bus name, cookie). Not saved: the indexer
    /// resumes by itself when the agent leaves the bus.
    paused: Option<(String, i32)>,
    verbose: bool,
}

fn state_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("dynamis-power/desktop.json"))
}

impl Desktop {
    pub fn new(verbose: bool) -> Desktop {
        let applied = state_file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Applied>(&t).ok())
            .filter(|a| a.v == VERSION)
            .unwrap_or_default();
        Desktop {
            kind: Kind::detect(),
            applied,
            last: None,
            paused: None,
            verbose,
        }
    }

    fn save(&self) {
        let Some(p) = state_file() else {
            return;
        };
        let empty =
            !self.applied.baloo && self.applied.refresh.is_none() && self.applied.style.is_none();
        if empty {
            let _ = std::fs::remove_file(p);
            return;
        }
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let a = Applied {
            v: VERSION,
            baloo: self.applied.baloo,
            refresh: self.applied.refresh.clone(),
            style: self.applied.style.clone(),
        };
        if let Ok(t) = serde_json::to_string_pretty(&a) {
            let _ = std::fs::write(p, t);
        }
    }

    fn say(&self, what: &str) {
        if self.verbose {
            eprintln!("dynamis-agent: desktop: {what}");
        }
    }

    /// At start: put back whatever a previous run left (it was killed), and
    /// tell the service what this desktop can do.
    pub async fn start(&mut self, session: &Connection, service: &Connection) {
        self.put_back_indexing(session).await;
        self.put_back_refresh(session).await;
        self.put_back_style();
        self.report(session, service).await;
    }

    /// Look at what the service wants; act on what changed.
    pub async fn sync(&mut self, session: &Connection, service: &Connection) {
        let Some(v) = call_json(service, "Desktop").await else {
            return;
        };
        let want: Want = serde_json::from_value(v["want"].clone()).unwrap_or_default();
        let last = self.last.unwrap_or_default();
        if self.last == Some(want) {
            return;
        }
        self.last = Some(want);
        if want.pause_indexing != last.pause_indexing {
            if want.pause_indexing {
                self.pause_indexing(session).await;
            } else {
                self.put_back_indexing(session).await;
            }
        }
        if want.lower_refresh != last.lower_refresh {
            if want.lower_refresh {
                self.lower_refresh(session).await;
            } else {
                self.put_back_refresh(session).await;
            }
        }
        if want.dark_style != last.dark_style {
            if want.dark_style {
                self.dark_style();
            } else {
                self.put_back_style();
            }
        }
    }

    async fn report(&self, session: &Connection, service: &Connection) {
        let indexer = match self.kind {
            Kind::Kde if has_name(session, "org.kde.baloo").await => Some("baloo"),
            _ => tracker_name(session).await.map(|_| "localsearch"),
        };
        let (refresh_hz, low_hz) = match self.builtin_modes(session).await {
            Some(b) => {
                let now = b.modes.iter().find(|m| m.id == b.current).map(|m| m.hz);
                let low = pick_low(&b.modes, &b.current)
                    .and_then(|id| b.modes.iter().find(|m| m.id == id))
                    .map(|m| m.hz);
                (now, low)
            }
            None => (None, None),
        };
        let can_style = match self.kind {
            Kind::Kde => in_path("plasma-apply-colorscheme"),
            Kind::Gnome => in_path("gsettings"),
            Kind::Other => false,
        };
        let found = json!({
            "desktop": self.kind.name(),
            "indexer": indexer,
            "refresh_hz": refresh_hz,
            "low_hz": low_hz,
            "can_style": can_style,
        });
        let _ = service
            .call_method(
                Some(SERVICE),
                SERVICE_PATH,
                Some(SERVICE_IFACE),
                "ReportDesktop",
                &(found.to_string(),),
            )
            .await;
    }

    // --- File indexing -------------------------------------------------

    async fn pause_indexing(&mut self, session: &Connection) {
        match self.kind {
            Kind::Kde => {
                // Suspended by the person already (state 1): leave it.
                let state: Option<i32> = baloo(session, "state").await;
                if state.is_none() || state == Some(BALOO_SUSPENDED) {
                    return;
                }
                self.applied.baloo = true;
                self.save();
                if baloo::<()>(session, "suspend").await.is_some() {
                    self.say("Baloo paused");
                }
            }
            _ => {
                let Some(name) = tracker_name(session).await else {
                    return;
                };
                let r = session
                    .call_method(
                        Some(name.as_str()),
                        TRACKER_PATH,
                        Some(TRACKER_IFACE),
                        "Pause",
                        &("Dynamis Power", "on battery"),
                    )
                    .await
                    .ok()
                    .and_then(|m| m.body().deserialize::<i32>().ok());
                if let Some(cookie) = r {
                    self.say("file indexing paused");
                    self.paused = Some((name, cookie));
                }
            }
        }
    }

    async fn put_back_indexing(&mut self, session: &Connection) {
        if let Some((name, cookie)) = self.paused.take() {
            let _ = session
                .call_method(
                    Some(name.as_str()),
                    TRACKER_PATH,
                    Some(TRACKER_IFACE),
                    "Resume",
                    &(cookie,),
                )
                .await;
            self.say("file indexing resumed");
        }
        if self.applied.baloo {
            // Only if still suspended (the person may have resumed it).
            if baloo::<i32>(session, "state").await == Some(BALOO_SUSPENDED) {
                let _ = baloo::<()>(session, "resume").await;
                self.say("Baloo resumed");
            }
            self.applied.baloo = false;
            self.save();
        }
    }

    // --- Refresh rate --------------------------------------------------

    async fn builtin_modes(&self, session: &Connection) -> Option<Builtin> {
        match self.kind {
            Kind::Kde => kscreen_json().and_then(|j| kde_builtin(&j)),
            Kind::Gnome => mutter_state(session).await.and_then(|s| gnome_builtin(&s)),
            Kind::Other => None,
        }
    }

    async fn lower_refresh(&mut self, session: &Connection) {
        let Some(b) = self.builtin_modes(session).await else {
            return;
        };
        let Some(low) = pick_low(&b.modes, &b.current) else {
            return;
        };
        self.applied.refresh = Some(Changed {
            output: b.output.clone(),
            before: b.current.clone(),
            set: low.clone(),
        });
        self.save();
        if self.set_mode(session, &b.output, &low).await {
            self.say(&format!("{} mode {} → {low}", b.output, b.current));
        } else {
            self.applied.refresh = None;
            self.save();
        }
    }

    async fn put_back_refresh(&mut self, session: &Connection) {
        let Some(c) = self.applied.refresh.take() else {
            return;
        };
        self.save();
        // Only if the screen is still in the mode the agent set.
        if let Some(b) = self.builtin_modes(session).await {
            if b.output == c.output && b.current == c.set {
                self.set_mode(session, &c.output, &c.before).await;
                self.say(&format!("{} mode back to {}", c.output, c.before));
            }
        }
    }

    async fn set_mode(&self, session: &Connection, output: &str, mode: &str) -> bool {
        match self.kind {
            Kind::Kde => Command::new("kscreen-doctor")
                .arg(format!("output.{output}.mode.{mode}"))
                .output()
                .is_ok_and(|o| o.status.success()),
            Kind::Gnome => gnome_apply(session, output, mode).await,
            Kind::Other => false,
        }
    }

    // --- Dark style ----------------------------------------------------

    fn current_style(&self) -> Option<String> {
        match self.kind {
            Kind::Kde => ["kreadconfig6", "kreadconfig5"].iter().find_map(|tool| {
                let o = Command::new(tool)
                    .args([
                        "--file",
                        "kdeglobals",
                        "--group",
                        "General",
                        "--key",
                        "ColorScheme",
                    ])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())?;
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                Some(if s.is_empty() {
                    "BreezeLight".into()
                } else {
                    s
                })
            }),
            Kind::Gnome => {
                let o = Command::new("gsettings")
                    .args(["get", "org.gnome.desktop.interface", "color-scheme"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())?;
                Some(
                    String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .trim_matches('\'')
                        .to_string(),
                )
            }
            Kind::Other => None,
        }
    }

    fn set_style(&self, name: &str) -> bool {
        let r = match self.kind {
            Kind::Kde => Command::new("plasma-apply-colorscheme").arg(name).output(),
            Kind::Gnome => Command::new("gsettings")
                .args(["set", "org.gnome.desktop.interface", "color-scheme", name])
                .output(),
            Kind::Other => return false,
        };
        r.is_ok_and(|o| o.status.success())
    }

    fn dark_style(&mut self) {
        let Some(now) = self.current_style() else {
            return;
        };
        let dark = match self.kind {
            Kind::Kde => DARK_KDE,
            _ => "prefer-dark",
        };
        if now.to_ascii_lowercase().contains("dark") {
            return; // already dark
        }
        self.applied.style = Some(Changed {
            output: String::new(),
            before: now.clone(),
            set: dark.into(),
        });
        self.save();
        if self.set_style(dark) {
            self.say(&format!("style {now} → {dark}"));
        } else {
            self.applied.style = None;
            self.save();
        }
    }

    fn put_back_style(&mut self) {
        let Some(c) = self.applied.style.take() else {
            return;
        };
        self.save();
        if self.current_style().as_deref() == Some(c.set.as_str()) {
            self.set_style(&c.before);
            self.say(&format!("style back to {}", c.before));
        }
    }
}

// --- Helpers -------------------------------------------------------------

/// Baloo's `IndexerState::Suspended`.
const BALOO_SUSPENDED: i32 = 1;
const TRACKER_PATH: &str = "/org/freedesktop/Tracker3/Miner/Files";
const TRACKER_IFACE: &str = "org.freedesktop.Tracker3.Miner";

async fn baloo<T: serde::de::DeserializeOwned + zbus::zvariant::Type>(
    session: &Connection,
    method: &str,
) -> Option<T> {
    let m = session
        .call_method(
            Some("org.kde.baloo"),
            "/scheduler",
            Some("org.kde.baloo.scheduler"),
            method,
            &(),
        )
        .await
        .ok()?;
    m.body().deserialize::<T>().ok()
}

async fn has_name(session: &Connection, name: &str) -> bool {
    let Ok(dbus) = zbus::fdo::DBusProxy::new(session).await else {
        return false;
    };
    let Ok(n) = zbus::names::BusName::try_from(name) else {
        return false;
    };
    dbus.name_has_owner(n).await.unwrap_or(false)
}

/// The GNOME file indexer's bus name: LocalSearch (GNOME 47 and later) or
/// Tracker 3 before it.
async fn tracker_name(session: &Connection) -> Option<String> {
    for n in [
        "org.freedesktop.LocalSearch3",
        "org.freedesktop.Tracker3.Miner.Files",
    ] {
        if has_name(session, n).await {
            return Some(n.to_string());
        }
    }
    None
}

fn in_path(tool: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file()))
}

async fn call_json(service: &Connection, method: &str) -> Option<Value> {
    let m = service
        .call_method(
            Some(SERVICE),
            SERVICE_PATH,
            Some(SERVICE_IFACE),
            method,
            &(),
        )
        .await
        .ok()?;
    serde_json::from_str(&m.body().deserialize::<String>().ok()?).ok()
}

#[derive(Debug, Clone, PartialEq)]
struct Mode {
    id: String,
    w: i64,
    h: i64,
    hz: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct Builtin {
    output: String,
    current: String,
    modes: Vec<Mode>,
}

fn is_builtin_name(name: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|p| name.starts_with(p))
}

/// The slowest mode at the current size that is still 60 Hz or more, if it
/// is slower than now.
fn pick_low(modes: &[Mode], current: &str) -> Option<String> {
    let cur = modes.iter().find(|m| m.id == current)?;
    modes
        .iter()
        .filter(|m| m.w == cur.w && m.h == cur.h && m.hz >= MIN_HZ && m.hz < cur.hz - 1.0)
        .min_by(|a, b| a.hz.total_cmp(&b.hz))
        .map(|m| m.id.clone())
}

fn kscreen_json() -> Option<Value> {
    let o = Command::new("kscreen-doctor").arg("-j").output().ok()?;
    if !o.status.success() {
        return None;
    }
    serde_json::from_slice(&o.stdout).ok()
}

/// The built-in screen in `kscreen-doctor -j` output.
fn kde_builtin(j: &Value) -> Option<Builtin> {
    let id = |v: &Value| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    let out = j["outputs"].as_array()?.iter().find(|o| {
        o["enabled"].as_bool() == Some(true)
            && o["connected"].as_bool() != Some(false)
            && o["name"].as_str().is_some_and(is_builtin_name)
    })?;
    let modes = out["modes"]
        .as_array()?
        .iter()
        .filter_map(|m| {
            Some(Mode {
                id: id(&m["id"])?,
                w: m["size"]["width"].as_i64()?,
                h: m["size"]["height"].as_i64()?,
                hz: m["refreshRate"].as_f64()?,
            })
        })
        .collect();
    Some(Builtin {
        output: out["name"].as_str()?.to_string(),
        current: id(&out["currentModeId"])?,
        modes,
    })
}

// GNOME (Mutter's org.gnome.Mutter.DisplayConfig).

type Props = HashMap<String, OwnedVariant>;
type MonitorSpec = (String, String, String, String);
type MutterMode = (String, i32, i32, f64, f64, Vec<f64>, Props);
type MutterMonitor = (MonitorSpec, Vec<MutterMode>, Props);
type MutterLogical = (i32, i32, f64, u32, bool, Vec<MonitorSpec>, Props);
type MutterState = (u32, Vec<MutterMonitor>, Vec<MutterLogical>, Props);
/// One logical monitor for `ApplyMonitorsConfig`: x, y, scale, transform,
/// primary, and (connector, mode id, properties) for each monitor in it.
type ApplyLogical<'a> = (
    i32,
    i32,
    f64,
    u32,
    bool,
    Vec<(String, String, HashMap<String, Variant<'a>>)>,
);

const MUTTER: &str = "org.gnome.Mutter.DisplayConfig";
const MUTTER_PATH: &str = "/org/gnome/Mutter/DisplayConfig";

async fn mutter_state(session: &Connection) -> Option<MutterState> {
    let m = session
        .call_method(
            Some(MUTTER),
            MUTTER_PATH,
            Some(MUTTER),
            "GetCurrentState",
            &(),
        )
        .await
        .ok()?;
    m.body().deserialize::<MutterState>().ok()
}

fn prop_bool(p: &Props, k: &str) -> bool {
    p.get(k)
        .and_then(|v| bool::try_from(&**v).ok())
        .unwrap_or(false)
}

fn gnome_builtin(s: &MutterState) -> Option<Builtin> {
    let (spec, modes, props) =
        s.1.iter()
            .find(|(spec, _, props)| prop_bool(props, "is-builtin") || is_builtin_name(&spec.0))?;
    let _ = props;
    let current = modes
        .iter()
        .find(|m| prop_bool(&m.6, "is-current"))?
        .0
        .clone();
    Some(Builtin {
        output: spec.0.clone(),
        current,
        modes: modes
            .iter()
            .map(|m| Mode {
                id: m.0.clone(),
                w: m.1.into(),
                h: m.2.into(),
                hz: m.3,
            })
            .collect(),
    })
}

/// Apply the current layout with one connector's mode changed, as a
/// temporary configuration (method 1: not saved by Mutter).
async fn gnome_apply(session: &Connection, connector: &str, mode: &str) -> bool {
    let Some(s) = mutter_state(session).await else {
        return false;
    };
    let current_mode = |c: &str| {
        s.1.iter()
            .find(|(spec, _, _)| spec.0 == c)
            .and_then(|(_, modes, _)| modes.iter().find(|m| prop_bool(&m.6, "is-current")))
            .map(|m| m.0.clone())
    };
    let mut logical: Vec<ApplyLogical> = Vec::new();
    for (x, y, scale, transform, primary, specs, _) in &s.2 {
        let mut mons = Vec::new();
        for spec in specs {
            let m = if spec.0 == connector {
                Some(mode.to_string())
            } else {
                current_mode(&spec.0)
            };
            let Some(m) = m else {
                return false;
            };
            mons.push((spec.0.clone(), m, HashMap::new()));
        }
        logical.push((*x, *y, *scale, *transform, *primary, mons));
    }
    let props: HashMap<String, Variant> = HashMap::new();
    session
        .call_method(
            Some(MUTTER),
            MUTTER_PATH,
            Some(MUTTER),
            "ApplyMonitorsConfig",
            &(s.0, 1u32, logical, props),
        )
        .await
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, w: i64, h: i64, hz: f64) -> Mode {
        Mode {
            id: id.into(),
            w,
            h,
            hz,
        }
    }

    #[test]
    fn picks_the_slowest_mode_at_60_hz_or_more_at_the_same_size() {
        let modes = [
            m("1", 2880, 1800, 120.0),
            m("2", 2880, 1800, 90.0),
            m("3", 2880, 1800, 60.0),
            m("4", 2880, 1800, 48.0),
            m("5", 1920, 1200, 60.0),
        ];
        assert_eq!(pick_low(&modes, "1").as_deref(), Some("3"));
        assert_eq!(pick_low(&modes, "3"), None, "already 60 Hz");
        assert_eq!(
            pick_low(&[m("a", 1920, 1080, 60.02), m("b", 1920, 1080, 59.97)], "a"),
            None
        );
    }

    #[test]
    fn finds_the_built_in_screen_in_kscreen_output() {
        let j = json!({"outputs": [
            {"name": "HDMI-A-1", "enabled": true, "connected": true, "currentModeId": "9",
             "modes": [{"id": "9", "refreshRate": 60.0, "size": {"width": 1920, "height": 1080}}]},
            {"name": "eDP-1", "enabled": true, "connected": true, "currentModeId": "1",
             "modes": [{"id": "1", "refreshRate": 165.0, "size": {"width": 2560, "height": 1600}},
                       {"id": "2", "refreshRate": 60.0, "size": {"width": 2560, "height": 1600}}]}
        ]});
        let b = kde_builtin(&j).unwrap();
        assert_eq!(b.output, "eDP-1");
        assert_eq!(pick_low(&b.modes, &b.current).as_deref(), Some("2"));
    }
}
