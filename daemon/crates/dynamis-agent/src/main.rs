//! `dynamis-agent` — tells Dynamis Power what the desktop session is doing.
//!
//! Runs as the logged-in person (a systemd user service), never as root. The
//! root service cannot see inside a desktop session, so this small program
//! watches the session bus and reports five facts:
//!
//! | Fact | Source |
//! | --- | --- |
//! | `media_playing` | Any media player (MPRIS) is playing |
//! | `screen_locked` | `org.freedesktop.ScreenSaver.GetActive` |
//! | `screen_inhibited` | An app asked the desktop not to dim the screen (KDE PolicyAgent, or GNOME's idle inhibit) |
//! | `game_running` | A game registered with Feral GameMode |
//! | `user_idle` | Not reported yet: the screen lock covers walking away |
//!
//! It never polls. It waits for D-Bus signals, re-reads the facts when one
//! arrives, and reports only when something changed. Every source is
//! optional: a desktop without one simply reports `false` for that fact.
//! It sends nothing else: no window titles, no app names, no media titles.

use std::collections::HashMap;
use std::process::ExitCode;

use clap::Parser;
use futures_lite::{Stream, StreamExt};
use zbus::message::Type;
use zbus::{Connection, MatchRule, MessageStream, OwnedVariant};

const SERVICE: &str = "net.dynamis.Power";
const SERVICE_PATH: &str = "/net/dynamis/Power";
const SERVICE_IFACE: &str = "net.dynamis.Power1";

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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Facts {
    media_playing: bool,
    screen_locked: bool,
    screen_inhibited: bool,
    game_running: bool,
}

impl Facts {
    fn as_dict(&self) -> HashMap<String, OwnedVariant> {
        HashMap::from([
            ("media_playing".into(), self.media_playing.into()),
            ("screen_locked".into(), self.screen_locked.into()),
            ("screen_inhibited".into(), self.screen_inhibited.into()),
            ("game_running".into(), self.game_running.into()),
            ("user_idle".into(), false.into()),
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
    let mut events = Box::pin(events.or(service_appeared));

    let mut last: Option<Facts> = None;
    let mut force = true;
    loop {
        let now = read_facts(&session).await;
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
            Some(Event::ServiceStarted) => force = true,
            Some(Event::Session) => force = false,
            None => return Ok(()),
        }
    }
}

enum Event {
    Session,
    ServiceStarted,
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
    }
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
