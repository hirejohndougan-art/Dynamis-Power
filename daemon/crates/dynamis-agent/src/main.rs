//! `dynamis-agent` — tells Dynamis Power what the desktop session is doing.
//!
//! Runs as the logged-in person (a systemd user service), never as root. The
//! root service cannot see inside a desktop session, so this small program
//! watches the session bus and reports these facts:
//!
//! | Fact | Source |
//! | --- | --- |
//! | `media_playing` | Any media player (MPRIS) is playing |
//! | `screen_locked` | `org.freedesktop.ScreenSaver.GetActive` |
//! | `screen_inhibited` | An app asked the desktop not to dim the screen (KDE PolicyAgent, or GNOME's idle inhibit) |
//! | `game_running` | A game registered with Feral GameMode |
//! | `user_idle` | Not reported yet: the screen lock covers walking away |
//! | `focused_pid` | Process ID of the window in front (KDE Plasma: a tiny KWin script tells the agent; GNOME: the "Dynamis Power: window in front" extension, once switched on; 0 elsewhere) |
//!
//! It never polls. It waits for D-Bus signals, re-reads the facts when one
//! arrives, and reports only when something changed. Every source is
//! optional: a desktop without one simply reports `false` (or 0) for that fact.
//! It sends nothing else: no window titles, no app names, no media titles.
//! The service turns the process ID into an app name itself, on this
//! computer, for the Apps screen and per-app modes.
//!
//! It also shows the **panel icon** (see `tray.rs`): the power mode at a
//! glance, and a menu to change it, switch auto mode, make the battery last,
//! or open the app. It starts at every login, so the icon is always there
//! without the app running. "Hide this icon" in its menu (or the app's
//! setting) turns it off; that choice is kept in
//! `~/.config/dynamis-power/agent.json`.
//!
//! It also passes the brightness the service set for a mode on to KDE
//! Plasma, which keeps its own (see `brightness.rs`).
//!
//! Only one agent runs per session: a second copy (for example started both
//! by systemd and by the desktop's autostart) sees the first and exits.

mod brightness;
mod strings;
mod tray;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use clap::Parser;
use futures_lite::{Stream, StreamExt};
use zbus::message::Type;
use zbus::{Connection, MatchRule, MessageStream, OwnedVariant};

const SERVICE: &str = "net.dynamis.Power";
const SERVICE_PATH: &str = "/net/dynamis/Power";
const SERVICE_IFACE: &str = "net.dynamis.Power1";

/// The agent's own name on the session bus, for the KWin script and the
/// GNOME extension to call.
const AGENT_NAME: &str = "net.dynamis.Agent";
const AGENT_PATH: &str = "/net/dynamis/Agent";
const AGENT_IFACE: &str = "net.dynamis.Agent1";
const KWIN_SCRIPT_NAME: &str = "dynamis-focus";

/// Runs inside KWin. Works with Plasma 6 (`windowActivated`) and 5
/// (`clientActivated`). Sends only the process ID.
const KWIN_SCRIPT: &str = r#"
function dynamisReport(w) {
    callDBus("net.dynamis.Agent", "/net/dynamis/Agent", "net.dynamis.Agent1",
             "FocusChanged", w && w.pid ? w.pid : 0);
}
var dynamisSignal = workspace.windowActivated || workspace.clientActivated;
dynamisSignal.connect(dynamisReport);
dynamisReport(workspace.activeWindow || workspace.activeClient);
"#;

#[derive(Parser)]
#[command(
    name = "dynamis-agent",
    version,
    about = "Reports desktop session activity to Dynamis Power"
)]
struct Args {
    /// Report on the session bus instead of the system bus (testing only).
    #[arg(long, hide = true)]
    report_on_session_bus: bool,
    /// Print each report.
    #[arg(long)]
    verbose: bool,
    /// Do not show the panel icon, whatever the saved choice says.
    #[arg(long)]
    no_tray: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Config {
    v: u32,
    tray: bool,
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("dynamis-power").join("agent.json"))
}

/// Whether the person wants the panel icon (yes unless they hid it).
fn tray_wanted() -> bool {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Config>(&t).ok())
        .is_none_or(|c| c.tray)
}

/// The panel icon, once started.
static TRAY: OnceLock<tray::Tray> = OnceLock::new();

/// Show or hide the panel icon, and remember the choice.
pub(crate) async fn set_tray_enabled(session: &Connection, on: bool) {
    if let Some(p) = config_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = serde_json::to_string(&Config { v: 1, tray: on }).unwrap_or_default();
        let _ = std::fs::write(p, text);
    }
    if let Some(t) = TRAY.get() {
        if on {
            t.show(session).await;
        } else {
            t.hide(session).await;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Facts {
    media_playing: bool,
    screen_locked: bool,
    screen_inhibited: bool,
    game_running: bool,
    focused_pid: u32,
}

/// Receives focus changes from the KWin script.
struct Focus(Arc<AtomicU32>);

#[zbus::interface(name = "net.dynamis.Agent1")]
impl Focus {
    fn focus_changed(&self, pid: u32) {
        self.0.store(pid, Ordering::Relaxed);
    }

    /// Show or hide the panel icon (the app's setting).
    async fn set_tray(&self, on: bool, #[zbus(connection)] conn: &Connection) {
        set_tray_enabled(conn, on).await;
    }

    /// Whether the panel icon is on.
    #[zbus(property)]
    fn tray(&self) -> bool {
        tray_wanted()
    }
}

impl Facts {
    fn as_dict(&self) -> HashMap<String, OwnedVariant> {
        HashMap::from([
            ("media_playing".into(), self.media_playing.into()),
            ("screen_locked".into(), self.screen_locked.into()),
            ("screen_inhibited".into(), self.screen_inhibited.into()),
            ("game_running".into(), self.game_running.into()),
            ("user_idle".into(), false.into()),
            ("focused_pid".into(), self.focused_pid.into()),
        ])
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    match zbus::block_on(run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dynamis-agent: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> zbus::Result<()> {
    let session = Connection::session().await?;
    let focus = Arc::new(AtomicU32::new(0));
    session
        .object_server()
        .at(AGENT_PATH, Focus(focus.clone()))
        .await?;
    // A second agent in the same session would only duplicate reports and
    // icons: leave it to the first.
    if session.request_name(AGENT_NAME).await.is_err() {
        if args.verbose {
            eprintln!("dynamis-agent: already running in this session");
        }
        return Ok(());
    }
    if let Err(e) = load_kwin_script(&session).await {
        if args.verbose {
            eprintln!("dynamis-agent: no focus tracking ({e})");
        }
    }
    let report_bus = if args.report_on_session_bus {
        session.clone()
    } else {
        Connection::system().await?
    };

    // Every signal that might change a fact. Each one only triggers a re-read.
    let rules = [
        rule(
            Some("org.freedesktop.DBus.Properties"),
            Some("PropertiesChanged"),
            Some("/org/mpris/MediaPlayer2"),
        ),
        rule(Some("org.freedesktop.DBus"), Some("NameOwnerChanged"), None),
        rule(
            Some("org.freedesktop.ScreenSaver"),
            Some("ActiveChanged"),
            None,
        ),
        rule(
            Some("org.kde.Solid.PowerManagement.PolicyAgent"),
            Some("InhibitionsChanged"),
            None,
        ),
        rule(Some("org.gnome.SessionManager"), None, None),
        rule(
            Some("org.freedesktop.DBus.Properties"),
            Some("PropertiesChanged"),
            Some("/com/feralinteractive/GameMode"),
        ),
        // The KWin script calling us: wakes the loop to report the change.
        MatchRule::builder()
            .msg_type(Type::MethodCall)
            .interface(AGENT_IFACE)
            .member("FocusChanged")
            .build(),
    ];
    let mut events: std::pin::Pin<Box<dyn Stream<Item = Event> + Send>> =
        Box::pin(futures_lite::stream::pending());
    for r in rules {
        let s = MessageStream::for_match_rule(r?, &session, None)
            .await?
            .map(|_| Event::Session);
        events = Box::pin(events.or(s));
    }
    // When the service (re)starts, tell it everything again.
    let service_appeared = MessageStream::for_match_rule(
        rule(Some("org.freedesktop.DBus"), Some("NameOwnerChanged"), None)?,
        &report_bus,
        None,
    )
    .await?
    .filter_map(|m| {
        let m = m.ok()?;
        let (name, _, new): (String, String, String) = m.body().deserialize().ok()?;
        (name == SERVICE && !new.is_empty()).then_some(Event::ServiceStarted)
    });
    // The panel icon follows the service: mode, auto mode, Last until.
    let service_changed = MessageStream::for_match_rule(
        MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .member("PropertiesChanged")
            .sender(SERVICE)
            .build()?,
        &report_bus,
        None,
    )
    .await?
    .map(|_| Event::ServiceChanged);
    // A panel that starts after the agent needs telling about the icon.
    let panel_appeared = MessageStream::for_match_rule(
        rule(Some("org.freedesktop.DBus"), Some("NameOwnerChanged"), None)?,
        &session,
        None,
    )
    .await?
    .filter_map(|m| {
        let m = m.ok()?;
        let (name, _, new): (String, String, String) = m.body().deserialize().ok()?;
        (name == "org.kde.StatusNotifierWatcher" && !new.is_empty()).then_some(Event::PanelStarted)
    });
    let mut events = Box::pin(
        events
            .or(service_appeared)
            .or(service_changed)
            .or(panel_appeared),
    );

    if !args.no_tray {
        match tray::Tray::start(&session, report_bus.clone(), args.verbose).await {
            Ok(t) => {
                if !tray_wanted() {
                    t.hide(&session).await;
                }
                let _ = TRAY.set(t);
            }
            Err(e) => eprintln!("dynamis-agent: no panel icon: {e}"),
        }
    }

    let mut last: Option<Facts> = None;
    let mut force = true;
    let mut bright = brightness::Follower::default();
    bright.check(&report_bus, &session, args.verbose).await;
    loop {
        let now = Facts {
            focused_pid: focus.load(Ordering::Relaxed),
            ..read_facts(&session).await
        };
        if force || last != Some(now) {
            match report(&report_bus, now).await {
                Ok(()) => {
                    if args.verbose {
                        eprintln!("dynamis-agent: reported {now:?}");
                    }
                    last = Some(now);
                }
                // The service is not running yet; report when it appears.
                Err(e) if args.verbose => eprintln!("dynamis-agent: {e}"),
                Err(_) => {}
            }
        }
        match events.next().await {
            Some(Event::ServiceStarted) => {
                force = true;
                if let Some(t) = TRAY.get() {
                    tray::refresh(&session, &t.shared).await;
                }
            }
            Some(Event::ServiceChanged) => {
                force = false;
                bright.check(&report_bus, &session, args.verbose).await;
                if let Some(t) = TRAY.get() {
                    tray::refresh(&session, &t.shared).await;
                }
            }
            Some(Event::PanelStarted) => {
                force = false;
                if let Some(t) = TRAY.get().filter(|_| tray_wanted()) {
                    t.show(&session).await;
                }
            }
            Some(Event::Session) => force = false,
            None => return Ok(()),
        }
    }
}

enum Event {
    Session,
    ServiceStarted,
    ServiceChanged,
    PanelStarted,
}

fn rule(
    iface: Option<&'static str>,
    member: Option<&'static str>,
    path: Option<&'static str>,
) -> zbus::Result<MatchRule<'static>> {
    let mut b = MatchRule::builder().msg_type(Type::Signal);
    if let Some(i) = iface {
        b = b.interface(i);
    }
    if let Some(m) = member {
        b = b.member(m);
    }
    if let Some(p) = path {
        b = b.path(p);
    }
    b.build()
}

async fn report(bus: &Connection, f: Facts) -> zbus::Result<()> {
    bus.call_method(
        Some(SERVICE),
        SERVICE_PATH,
        Some(SERVICE_IFACE),
        "ReportSession",
        &(f.as_dict(),),
    )
    .await
    .map(|_| ())
}

async fn read_facts(c: &Connection) -> Facts {
    Facts {
        media_playing: media_playing(c).await,
        screen_locked: screen_locked(c).await,
        screen_inhibited: screen_inhibited(c).await,
        game_running: game_running(c).await,
        focused_pid: 0,
    }
}

/// Load the focus script into KWin (KDE Plasma). Elsewhere this fails
/// quietly and `focused_pid` stays 0.
async fn load_kwin_script(c: &Connection) -> Result<(), String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or("no XDG_RUNTIME_DIR")?;
    let path = std::path::Path::new(&dir).join("dynamis-focus.js");
    std::fs::write(&path, KWIN_SCRIPT).map_err(|e| e.to_string())?;
    let path = path.to_string_lossy().into_owned();
    let call = |method: &'static str, body: Vec<String>| async move {
        c.call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            method,
            &body,
        )
        .await
        .map_err(|e| e.to_string())
    };
    // An earlier agent's copy is replaced, not doubled.
    let _ = c
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "unloadScript",
            &(KWIN_SCRIPT_NAME,),
        )
        .await;
    c.call_method(
        Some("org.kde.KWin"),
        "/Scripting",
        Some("org.kde.kwin.Scripting"),
        "loadScript",
        &(path.as_str(), KWIN_SCRIPT_NAME),
    )
    .await
    .map_err(|e| e.to_string())?;
    call("start", Vec::new()).await.map(|_| ())
}

/// A property read straight from its owner (no caching).
async fn get_prop(
    c: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    name: &str,
) -> Option<OwnedVariant> {
    let m = c
        .call_method(
            Some(dest),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(iface, name),
        )
        .await
        .ok()?;
    m.body().deserialize::<OwnedVariant>().ok()
}

async fn call_bool<B>(
    c: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    method: &str,
    body: &B,
) -> Option<bool>
where
    B: serde::Serialize + zbus::wire::DynamicType,
{
    let m = c
        .call_method(Some(dest), path, Some(iface), method, body)
        .await
        .ok()?;
    m.body().deserialize::<bool>().ok()
}

async fn media_playing(c: &Connection) -> bool {
    let Ok(dbus) = zbus::fdo::DBusProxy::new(c).await else {
        return false;
    };
    let Ok(names) = dbus.list_names().await else {
        return false;
    };
    for n in names
        .iter()
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2."))
    {
        let status = get_prop(
            c,
            n,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
            "PlaybackStatus",
        )
        .await;
        if status.and_then(|v| String::try_from(v).ok()).as_deref() == Some("Playing") {
            return true;
        }
    }
    false
}

async fn screen_locked(c: &Connection) -> bool {
    for path in ["/org/freedesktop/ScreenSaver", "/ScreenSaver"] {
        if let Some(v) = call_bool(
            c,
            "org.freedesktop.ScreenSaver",
            path,
            "org.freedesktop.ScreenSaver",
            "GetActive",
            &(),
        )
        .await
        {
            return v;
        }
    }
    false
}

async fn screen_inhibited(c: &Connection) -> bool {
    // KDE: 4 = "change screen settings" (dimming, screen off).
    if let Some(v) = call_bool(
        c,
        "org.kde.Solid.PowerManagement.PolicyAgent",
        "/org/kde/Solid/PowerManagement/PolicyAgent",
        "org.kde.Solid.PowerManagement.PolicyAgent",
        "HasInhibition",
        &(4u32,),
    )
    .await
    {
        return v;
    }
    // GNOME: 8 = "idle".
    call_bool(
        c,
        "org.gnome.SessionManager",
        "/org/gnome/SessionManager",
        "org.gnome.SessionManager",
        "IsInhibited",
        &(8u32,),
    )
    .await
    .unwrap_or(false)
}

async fn game_running(c: &Connection) -> bool {
    get_prop(
        c,
        "com.feralinteractive.GameMode",
        "/com/feralinteractive/GameMode",
        "com.feralinteractive.GameMode",
        "ClientCount",
    )
    .await
    .and_then(|v| i32::try_from(v).ok())
    .is_some_and(|n| n > 0)
}
