//! End-to-end: a private D-Bus bus, `dynamisd` on a temporary copy of the
//! fixture, and a client talking the standard power-profiles interface.
//! Skipped (with a note) when `dbus-daemon` is not installed.

use std::collections::HashMap;
use std::fs;

use futures_lite::StreamExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const DEST: &str = "org.freedesktop.UPower.PowerProfiles";
const PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const OLD_DEST: &str = "net.hadess.PowerProfiles";
const OLD_PATH: &str = "/net/hadess/PowerProfiles";

/// Kills its process when dropped, so a failing test never leaves strays.
struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Env {
    dir: PathBuf,
    address: String,
    _bus: Guard,
}

impl Env {
    fn new(name: &str) -> Option<Env> {
        if Command::new("dbus-daemon")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: dbus-daemon is not installed");
            return None;
        }
        let dir = std::env::temp_dir().join(format!("dynamisd-it-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/fixtures/sysfs/synthetic-laptop");
        assert!(Command::new("cp")
            .arg("-a")
            .arg(&src)
            .arg(dir.join("root"))
            .status()
            .unwrap()
            .success());
        let address = format!("unix:path={}", dir.join("bus").display());
        let bus = Command::new("dbus-daemon")
            .args([
                "--session",
                "--nofork",
                "--nopidfile",
                &format!("--address={address}"),
            ])
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        wait_for(|| dir.join("bus").exists());
        Some(Env {
            dir,
            address,
            _bus: Guard(bus),
        })
    }

    fn daemon(&self, state: &str) -> Child {
        self.daemon_with(state, &[])
    }

    fn daemon_with(&self, state: &str, extra: &[&str]) -> Child {
        Command::new(env!("CARGO_BIN_EXE_dynamisd"))
            .args(extra)
            .args(["--session", "--root"])
            .arg(self.dir.join("root"))
            .arg("--state-dir")
            .arg(self.dir.join(state))
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address)
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn epp(&self) -> String {
        fs::read_to_string(
            self.dir
                .join("root/sys/devices/system/cpu/cpufreq/policy0/energy_performance_preference"),
        )
        .unwrap()
        .trim()
        .into()
    }

    async fn client(&self) -> zbus::Connection {
        zbus::connection::Builder::address(self.address.as_str())
            .build()
            .await
            .unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn wait_for(mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting"
        );
        sleep(Duration::from_millis(20));
    }
}

async fn proxy<'a>(conn: &zbus::Connection, dest: &'a str, path: &'a str) -> zbus::Proxy<'a> {
    zbus::Proxy::new(conn, dest, path, dest).await.unwrap()
}

async fn wait_ready(conn: &zbus::Connection) {
    let dbus = zbus::fdo::DBusProxy::new(conn).await.unwrap();
    let start = Instant::now();
    while !dbus.name_has_owner(DEST.try_into().unwrap()).await.unwrap() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "dynamisd did not start"
        );
        sleep(Duration::from_millis(20));
    }
}

#[test]
fn power_profiles_interface_end_to_end() {
    let Some(env) = Env::new("e2e") else { return };
    let _d = Guard(env.daemon("state"));

    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let p = proxy(&conn, DEST, PATH).await;
        let old = proxy(&conn, OLD_DEST, OLD_PATH).await;

        assert_eq!(
            p.get_property::<String>("ActiveProfile").await.unwrap(),
            "balanced"
        );
        let profiles: Vec<std::collections::HashMap<String, zbus::OwnedVariant>> =
            old.get_property("Profiles").await.unwrap();
        assert_eq!(profiles.len(), 3);

        // Switching on one name is seen on the other and on the machine.
        p.set_property("ActiveProfile", "power-saver")
            .await
            .unwrap();
        assert_eq!(env.epp(), "power");
        assert_eq!(
            old.get_property::<String>("ActiveProfile").await.unwrap(),
            "power-saver"
        );
        assert!(p.set_property("ActiveProfile", "turbo").await.is_err());

        // A hold from another client ends when that client disconnects.
        let mut changes = p.receive_property_changed::<String>("ActiveProfile").await;
        {
            let app = env.client().await;
            let ap = proxy(&app, DEST, PATH).await;
            let cookie: u32 = ap
                .call("HoldProfile", &("performance", "build", "test-app"))
                .await
                .unwrap();
            assert!(cookie > 0);
            assert_eq!(env.epp(), "performance");
            // Another client may not release it.
            assert!(p
                .call::<_, _, ()>("ReleaseProfile", &(cookie,))
                .await
                .is_err());
        }
        wait_for(|| env.epp() == "power");
        // Wait for the change *signal* (not just the file): desktops rely on it.
        // Awaiting the stream keeps the client's connection running; a thread
        // sleep here would freeze it.
        loop {
            let change = changes.next().await.expect("signal stream ended");
            if change.get().await.unwrap() == "power-saver" {
                break;
            }
        }
    });

    // A second copy must refuse to start, and must not touch any setting.
    let before = env.epp();
    let second = env.daemon("state2").wait_with_output().unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("another power profile service"));
    assert_eq!(env.epp(), before);
    assert!(!env.dir.join("state2/journal.jsonl").exists());
}

#[test]
fn choice_survives_restart() {
    let Some(env) = Env::new("restart") else {
        return;
    };
    {
        let _d = Guard(env.daemon("state"));
        zbus::block_on(async {
            let conn = env.client().await;
            wait_ready(&conn).await;
            proxy(&conn, DEST, PATH)
                .await
                .set_property("ActiveProfile", "performance")
                .await
                .unwrap();
        });
    }
    // Simulate a reboot putting the kernel back to its default, on both CPUs.
    for policy in ["policy0", "policy1"] {
        fs::write(
            env.dir.join(format!(
                "root/sys/devices/system/cpu/cpufreq/{policy}/energy_performance_preference"
            )),
            "balance_performance",
        )
        .unwrap();
    }
    let _d = Guard(env.daemon("state"));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        assert_eq!(
            proxy(&conn, DEST, PATH)
                .await
                .get_property::<String>("ActiveProfile")
                .await
                .unwrap(),
            "performance"
        );
    });
    assert_eq!(env.epp(), "performance");
}

/// Wait (without blocking the client's connection) until `check` passes.
async fn eventually<F: AsyncFnMut() -> bool>(what: &str, mut check: F) {
    let start = Instant::now();
    while !check().await {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        // A short pause; replies keep arriving because each check is a call.
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn auto_mode_follows_the_situation() {
    let Some(env) = Env::new("auto") else { return };
    let _d = Guard(env.daemon_with("state", &["--tick-secs", "0.2"]));
    let root = env.dir.join("root");
    let set = |rel: &str, v: &str| fs::write(root.join(rel), v).unwrap();

    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let own = proxy(&conn, "net.dynamis.Power", "/net/dynamis/Power").await;
        let own = zbus::Proxy::new(
            &conn,
            "net.dynamis.Power",
            "/net/dynamis/Power",
            "net.dynamis.Power1",
        )
        .await
        .unwrap_or(own);
        let ppd = proxy(&conn, DEST, PATH).await;
        let reason = || async {
            // Read without the cache, straight from the daemon.
            let props =
                zbus::fdo::PropertiesProxy::new(&conn, "net.dynamis.Power", "/net/dynamis/Power")
                    .await
                    .unwrap();
            let v = props
                .get("net.dynamis.Power1".try_into().unwrap(), "AutoReason")
                .await
                .unwrap();
            String::try_from(v).unwrap()
        };

        assert!(!own.get_property::<bool>("AutoMode").await.unwrap());
        own.set_property("AutoMode", true).await.unwrap();
        eventually("first decision", async || {
            reason().await.contains("On battery")
        })
        .await;

        // The session agent reports a locked screen: power-saver.
        let agent = env.client().await;
        let report = |locked: bool| {
            HashMap::from([(
                "screen_locked".to_string(),
                zbus::OwnedVariant::from(locked),
            )])
        };
        let ap = zbus::Proxy::new(
            &agent,
            "net.dynamis.Power",
            "/net/dynamis/Power",
            "net.dynamis.Power1",
        )
        .await
        .unwrap();
        ap.call::<_, _, ()>("ReportSession", &(report(true),))
            .await
            .unwrap();
        eventually("power-saver when locked", async || env.epp() == "power").await;

        // Unlocking is urgent: straight back.
        ap.call::<_, _, ()>("ReportSession", &(report(false),))
            .await
            .unwrap();
        eventually("balanced after unlock", async || {
            env.epp() == "balance_performance"
        })
        .await;

        // Battery critical: power-saver at once.
        set("sys/class/power_supply/BAT0/capacity", "10");
        eventually("power-saver at 10%", async || env.epp() == "power").await;

        // Picking a mode by hand pauses auto mode...
        ppd.set_property("ActiveProfile", "performance")
            .await
            .unwrap();
        eventually("manual performance", async || env.epp() == "performance").await;
        eventually("paused", async || reason().await.contains("by hand")).await;
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(
            env.epp(),
            "performance",
            "auto mode must not undo a manual choice"
        );

        // ...until the charger is plugged in.
        set("sys/class/power_supply/AC0/online", "1");
        set("sys/class/power_supply/BAT0/status", "Charging");
        eventually("resumed on mains", async || {
            env.epp() == "balance_performance"
        })
        .await;
        eventually("plugged-in reason", async || {
            reason().await.contains("Plugged in")
        })
        .await;

        // Locking again right after a switch is not urgent: auto mode waits
        // out its dwell time instead of flapping.
        ap.call::<_, _, ()>("ReportSession", &(report(true),))
            .await
            .unwrap();
        eventually("dwell", async || {
            let r = reason().await;
            r.contains("Nobody is using") && r.contains("waiting")
        })
        .await;
        assert_eq!(env.epp(), "balance_performance");

        // The agent leaving the bus clears what it said.
        drop(ap);
        drop(agent);
        eventually("agent gone", async || reason().await.contains("Plugged in")).await;

        own.set_property("AutoMode", false).await.unwrap();
        eventually("off", async || reason().await.contains("off")).await;
    });
    assert!(env.dir.join("state/auto.json").exists());
}
