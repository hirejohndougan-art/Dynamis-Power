//! The agent against fake desktop services on a private bus: a media player,
//! a screen locker, and a stand-in for the Dynamis Power service that records
//! what it is told. Skipped when `dbus-daemon` is not installed.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use zbus::object_server::SignalEmitter;
use zbus::{Connection, OwnedVariant};

type Reports = Arc<Mutex<Vec<HashMap<String, bool>>>>;

struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Recorder(Reports);

#[zbus::interface(name = "net.dynamis.Power1")]
impl Recorder {
    fn report_session(&self, report: HashMap<String, OwnedVariant>) {
        let flat = report
            .into_iter()
            .map(|(k, v)| (k, bool::try_from(v).unwrap_or(false)))
            .collect();
        self.0.lock().unwrap().push(flat);
    }
}

struct Player {
    status: String,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.status.clone()
    }
    #[zbus(property)]
    fn set_playback_status(&mut self, s: String) {
        self.status = s;
    }
}

struct Locker {
    active: bool,
}

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl Locker {
    fn get_active(&self) -> bool {
        self.active
    }
    /// Test helper: lock or unlock, announcing it like a real locker.
    async fn set_active(&mut self, on: bool, #[zbus(signal_emitter)] e: SignalEmitter<'_>) {
        self.active = on;
        let _ = Locker::active_changed(&e, on).await;
    }
    #[zbus(signal)]
    async fn active_changed(e: &SignalEmitter<'_>, on: bool) -> zbus::Result<()>;
}

/// Run fake services on their own thread so they keep answering while the
/// test thread waits.
fn spawn_fakes(address: String, reports: Reports) {
    std::thread::spawn(move || {
        zbus::block_on(async move {
            let _rec = zbus::connection::Builder::address(address.as_str())
                .name("net.dynamis.Power")
                .serve_at("/net/dynamis/Power", Recorder(reports))
                .build()
                .await
                .unwrap();
            let _player = zbus::connection::Builder::address(address.as_str())
                .name("org.mpris.MediaPlayer2.fake")
                .serve_at(
                    "/org/mpris/MediaPlayer2",
                    Player {
                        status: "Paused".into(),
                    },
                )
                .build()
                .await
                .unwrap();
            let _locker = zbus::connection::Builder::address(address.as_str())
                .name("org.freedesktop.ScreenSaver")
                .serve_at("/org/freedesktop/ScreenSaver", Locker { active: false })
                .build()
                .await
                .unwrap();
            std::future::pending::<()>().await;
        });
    });
}

fn wait_for(what: &str, reports: &Reports, check: impl Fn(&HashMap<String, bool>) -> bool) {
    let start = Instant::now();
    loop {
        if reports.lock().unwrap().last().is_some_and(&check) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}: {:?}",
            reports.lock().unwrap().last()
        );
        sleep(Duration::from_millis(20));
    }
}

#[test]
fn reports_session_changes_and_nothing_else() {
    if Command::new("dbus-daemon")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: dbus-daemon is not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("dynamis-agent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let address = format!("unix:path={}", dir.join("bus").display());
    let _bus = Guard(
        Command::new("dbus-daemon")
            .args([
                "--session",
                "--nofork",
                "--nopidfile",
                &format!("--address={address}"),
            ])
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    while !dir.join("bus").exists() {
        assert!(start.elapsed() < Duration::from_secs(5));
        sleep(Duration::from_millis(20));
    }

    let reports: Reports = Arc::default();
    spawn_fakes(address.clone(), reports.clone());
    sleep(Duration::from_millis(300));

    let _agent = Guard(
        Command::new(env!("CARGO_BIN_EXE_dynamis-agent"))
            .arg("--report-on-session-bus")
            .env("DBUS_SESSION_BUS_ADDRESS", &address)
            .spawn()
            .unwrap(),
    );
    wait_for("first report", &reports, |r| {
        !r["media_playing"] && !r["screen_locked"]
    });
    assert_eq!(
        r_keys(&reports),
        [
            "game_running",
            "media_playing",
            "screen_inhibited",
            "screen_locked",
            "user_idle"
        ]
    );

    let client = zbus::block_on(async { Connection::session_at(&address).await });
    let client = client.unwrap();
    zbus::block_on(async {
        zbus::Proxy::new(
            &client,
            "org.mpris.MediaPlayer2.fake",
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        )
        .await
        .unwrap()
        .set_property("PlaybackStatus", "Playing")
        .await
        .unwrap();
    });
    wait_for("media playing", &reports, |r| r["media_playing"]);

    zbus::block_on(async {
        client
            .call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "SetActive",
                &(true,),
            )
            .await
            .unwrap();
    });
    wait_for("locked", &reports, |r| {
        r["screen_locked"] && r["media_playing"]
    });

    // Unrelated bus activity (a new client) triggers a re-read but no report.
    let before = reports.lock().unwrap().len();
    let _other = zbus::block_on(async { Connection::session_at(&address).await }).unwrap();
    sleep(Duration::from_millis(400));
    assert_eq!(
        reports.lock().unwrap().len(),
        before,
        "reported without any change"
    );

    std::fs::remove_dir_all(&dir).ok();
}

fn r_keys(reports: &Reports) -> Vec<String> {
    let mut k: Vec<String> = reports
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    k.sort();
    k
}

trait SessionAt {
    async fn session_at(address: &str) -> zbus::Result<Connection>;
}

impl SessionAt for Connection {
    async fn session_at(address: &str) -> zbus::Result<Connection> {
        zbus::connection::Builder::address(address).build().await
    }
}
