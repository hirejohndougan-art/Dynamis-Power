//! End-to-end: a private D-Bus bus, `dynamisd` on a temporary copy of the
//! fixture, and a client talking the standard power-profiles interface.
//! Skipped (with a note) when `dbus-daemon` is not installed.

use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
async fn eventually<F: AsyncFnMut() -> bool>(what: &str, check: F) {
    eventually_within(10, what, check).await
}

async fn eventually_within<F: AsyncFnMut() -> bool>(secs: u64, what: &str, mut check: F) {
    let start = Instant::now();
    while !check().await {
        assert!(
            start.elapsed() < Duration::from_secs(secs),
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

/// A pretend laptop: reads the settings Dynamis Power changes and reports
/// battery power and stall to match, with noise. Known truth to test against.
/// A simulated laptop: watts and stall (percent) from nmi off, turbo off and
/// the speed limit.
type Model = fn(bool, bool, f64) -> (f64, f64);

/// Turbo off adds lag; each 10% of speed limit saves a little and adds
/// clear lag (2 points: well above the 0.5-point limit even when a slow test
/// machine blurs the switch between test blocks).
fn plain_laptop(nmi_off: bool, turbo_off: bool, max_perf: f64) -> (f64, f64) {
    let watts = 6.0
        - if nmi_off { 0.4 } else { 0.0 }
        - if turbo_off { 0.8 } else { 0.0 }
        - (100.0 - max_perf) / 20.0 * 0.3;
    let stall = 0.5 + if turbo_off { 5.0 } else { 0.0 } + (100.0 - max_perf) / 5.0;
    (watts, stall)
}

fn spawn_plant(root: std::path::PathBuf, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    spawn_model(root, stop, plain_laptop)
}

fn spawn_model(
    root: std::path::PathBuf,
    stop: Arc<AtomicBool>,
    model: Model,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let read = |rel: &str| {
            fs::read_to_string(root.join(rel))
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let mut seed: u64 = 42;
        let mut stall_us: f64 = 1_000_000.0;
        let mut last = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            let nmi_off = read("proc/sys/kernel/nmi_watchdog") == "0";
            let turbo_off = read("sys/devices/system/cpu/intel_pstate/no_turbo") == "1";
            let max_perf: f64 = read("sys/devices/system/cpu/intel_pstate/max_perf_pct")
                .parse()
                .unwrap_or(100.0);
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let noise = ((seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 0.2;
            let (watts, stall_pct) = model(nmi_off, turbo_off, max_perf);
            let watts = watts + noise;
            let dt = last.elapsed().as_secs_f64();
            last = Instant::now();
            stall_us += stall_pct / 100.0 * dt * 1e6;
            let _ = fs::write(
                root.join("sys/class/power_supply/BAT0/power_now"),
                format!("{}", (watts * 1e6) as u64),
            );
            let _ = fs::write(
                root.join("proc/pressure/cpu"),
                format!(
                    "some avg10=0.00 avg60=0.00 avg300=0.00 total={}\n",
                    stall_us as u64
                ),
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    })
}

#[test]
fn learning_keeps_only_what_saves_power_without_lag() {
    let Some(env) = Env::new("learn") else { return };
    let stop = Arc::new(AtomicBool::new(false));
    let plant = spawn_plant(env.dir.join("root"), stop.clone());
    let _d = Guard(env.daemon_with(
        "state",
        &[
            "--tick-secs",
            "0.02",
            "--calib-block-secs",
            "0.3",
            "--calib-settle-secs",
            "0.05",
            "--calib-cooldown-secs",
            "0",
        ],
    ));
    let read = |rel: &str| {
        fs::read_to_string(env.dir.join("root").join(rel))
            .unwrap()
            .trim()
            .to_string()
    };

    let learned = zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let own = zbus::Proxy::new(
            &conn,
            "net.dynamis.Power",
            "/net/dynamis/Power",
            "net.dynamis.Power1",
        )
        .await
        .unwrap();
        own.set_property("Learning", true).await.unwrap();
        let start = Instant::now();
        loop {
            let props =
                zbus::fdo::PropertiesProxy::new(&conn, "net.dynamis.Power", "/net/dynamis/Power")
                    .await
                    .unwrap();
            let status = String::try_from(
                props
                    .get("net.dynamis.Power1".try_into().unwrap(), "LearningStatus")
                    .await
                    .unwrap(),
            )
            .unwrap();
            if status.contains("Every setting has been tested") {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(120),
                "learning did not finish: {status}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let text: String = own.call("LearnedJson", &()).await.unwrap();
        serde_json::from_str::<serde_json::Value>(&text).unwrap()
    });

    let verdict = |setting: &str, value: &str| -> Vec<String> {
        learned["records"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["setting"] == setting && r["value"] == value)
            .map(|r| {
                format!(
                    "{}: {}",
                    r["verdict"].as_str().unwrap(),
                    r["reason"].as_str().unwrap()
                )
            })
            .collect()
    };
    eprintln!(
        "{}",
        serde_json::to_string_pretty(&learned["records"]).unwrap()
    );
    assert!(
        verdict("kernel.nmi_watchdog", "off")[0].starts_with("keep"),
        "{:?}",
        verdict("kernel.nmi_watchdog", "off")
    );
    assert!(
        verdict("cpu.turbo", "off")[0].contains("less responsive"),
        "{:?}",
        verdict("cpu.turbo", "off")
    );
    // Phase 5 tunes these two together, one step at a time from 100%.
    assert!(!verdict("cpu.max_perf_pct", "90").is_empty());
    assert!(verdict("cpu.max_perf_pct", "90")
        .iter()
        .all(|v| !v.starts_with("keep")));
    assert!(verdict("vm.dirty_writeback", "1500")
        .iter()
        .all(|v| !v.starts_with("keep")));
    let kept: Vec<&str> = learned["learned"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["setting"].as_str().unwrap())
        .collect();
    assert_eq!(
        kept,
        ["kernel.nmi_watchdog"],
        "only real, lag-free savings are kept"
    );

    // Applied on battery, original value on mains.
    assert_eq!(read("proc/sys/kernel/nmi_watchdog"), "0");
    assert_eq!(
        read("sys/devices/system/cpu/intel_pstate/no_turbo"),
        "0",
        "rejected change undone"
    );
    fs::write(env.dir.join("root/sys/class/power_supply/AC0/online"), "1").unwrap();
    let start = Instant::now();
    while read("proc/sys/kernel/nmi_watchdog") != "1" {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "learned setting not undone on mains"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    fs::write(env.dir.join("root/sys/class/power_supply/AC0/online"), "0").unwrap();
    let start = Instant::now();
    while read("proc/sys/kernel/nmi_watchdog") != "0" {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "learned setting not reapplied on battery"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // Forget puts the original back.
    zbus::block_on(async {
        let conn = env.client().await;
        let own = zbus::Proxy::new(
            &conn,
            "net.dynamis.Power",
            "/net/dynamis/Power",
            "net.dynamis.Power1",
        )
        .await
        .unwrap();
        assert!(own
            .call::<_, _, bool>("Forget", &("kernel.nmi_watchdog",))
            .await
            .unwrap());
    });
    assert_eq!(read("proc/sys/kernel/nmi_watchdog"), "1");
    let journal = fs::read_to_string(env.dir.join("state/journal.jsonl")).unwrap();
    assert!(journal.contains("calibration: step"));
    assert!(
        !journal.contains("\"event\":\"recover\""),
        "the service must never recover its own live experiment"
    );
    stop.store(true, Ordering::Relaxed);
    plant.join().unwrap();
}

#[test]
fn the_app_client_can_do_everything_it_needs() {
    let Some(env) = Env::new("client") else {
        return;
    };
    let _d = Guard(env.daemon("state"));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();

        let s = c.status().await.unwrap();
        assert_eq!(s["profile"], "balanced");
        assert_eq!(s["power"]["battery_pct"], 79);
        assert_eq!(s["power"]["on_ac"], false);
        assert!(
            s["power"]["hours_left"].as_f64().unwrap() > 1.0,
            "{}",
            s["power"]
        );
        assert_eq!(s["safe_mode"], serde_json::Value::Null);

        c.set_profile("power-saver").await.unwrap();
        assert_eq!(c.status().await.unwrap()["profile"], "power-saver");

        c.set_setting("kernel.nmi_watchdog", "off", false)
            .await
            .unwrap();
        assert_eq!(env.epp(), "power");
        let settings = c.settings().await.unwrap();
        let nmi = settings
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "kernel.nmi_watchdog")
            .unwrap()
            .clone();
        assert_eq!(
            (nmi["current"].as_str(), nmi["original"].as_str()),
            (Some("off"), Some("on"))
        );

        let msg = c.undo("kernel.nmi_watchdog").await.unwrap();
        assert!(msg.contains("back to on"), "{msg}");

        let journal = c.journal(50).await.unwrap();
        let first = &journal.as_array().unwrap()[0];
        assert_eq!(first["event"], "restore", "newest first: {first}");
        assert_eq!(first["label"], "Watchdog wakeups");

        // Risky settings are refused without an explicit yes.
        let err = c.set_setting("cpu.turbo", "off", false).await;
        assert!(err.is_ok(), "turbo is Safe, so this works: {err:?}");

        // Restore puts everything back and switches auto mode off.
        c.set_auto(true).await.unwrap();
        let restored = c.restore().await.unwrap();
        assert!(
            restored
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == "cpu.turbo"),
            "{restored}"
        );
        assert_eq!(env.epp(), "balance_performance");
        assert_eq!(c.status().await.unwrap()["auto"]["on"], false);

        // Safe mode blocks changes, with a sentence a person can read.
        assert!(c.set_safe_mode(true).await.unwrap().contains("app"));
        let err = c.set_profile("performance").await.unwrap_err();
        assert!(err.contains("safe mode"), "{err}");
        assert_eq!(c.set_safe_mode(false).await.unwrap(), "");

        let inv = c.inventory().await.unwrap();
        assert_eq!(inv["machine"]["model"], "Synthetic Laptop 14");
        assert!(c.learned().await.unwrap()["records"]
            .as_array()
            .unwrap()
            .is_empty());
    });
}

#[test]
fn the_client_explains_a_missing_service() {
    let Some(env) = Env::new("noservice") else {
        return;
    };
    zbus::block_on(async {
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        let err = c.status().await.unwrap_err();
        assert!(err.contains("not running"), "{err}");
    });
}

#[test]
fn auto_preferences_and_conflicts() {
    let Some(env) = Env::new("prefs") else { return };
    let _d = Guard(env.daemon("state"));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();

        let mut p = c.auto_prefs().await.unwrap();
        assert_eq!(p["on_battery"], "balanced");
        p["on_battery"] = "power-saver".into();
        p["low_battery_pct"] = 40.into();
        c.set_auto_prefs(&p).await.unwrap();
        assert_eq!(c.auto_prefs().await.unwrap()["low_battery_pct"], 40.0);

        // Out of range: refused with a reason, nothing changed.
        p["low_battery_pct"] = 95.into();
        let err = c.set_auto_prefs(&p).await.unwrap_err();
        assert!(err.contains("low battery level"), "{err}");
        assert_eq!(c.auto_prefs().await.unwrap()["low_battery_pct"], 40.0);

        // No systemd on a test bus: no conflicts, no error.
        assert_eq!(c.conflicts().await.unwrap(), serde_json::json!([]));
        // Only known tools can ever be switched off.
        let err = c.switch_off_conflict("sshd.service").await.unwrap_err();
        assert!(err.contains("knows how to switch off"), "{err}");
    });
    let saved = fs::read_to_string(env.dir.join("state/auto.json")).unwrap();
    assert!(saved.contains("\"low_battery_pct\":40"), "{saved}");
}

#[test]
fn apps_share_the_cpu_by_policy_and_restore_puts_it_back() {
    let Some(env) = Env::new("apps") else { return };
    let root = env.dir.join("root");
    let base = "/user.slice/user-1000.slice/user@1000.service/app.slice";
    let mut weights = Vec::new();
    for (pid, name) in [(1001, "firefox"), (1002, "thunderbird")] {
        let cg = format!("{base}/app-{name}-{pid}.scope");
        let dir = root.join(format!("sys/fs/cgroup{cg}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cpu.weight"), "100").unwrap();
        weights.push(dir.join("cpu.weight"));
        let p = root.join(format!("proc/{pid}"));
        fs::create_dir_all(&p).unwrap();
        fs::write(
            p.join("stat"),
            format!("{pid} ({name}) S 1 1 1 0 -1 0 0 0 0 0 100 10 0 0 20 0 1 0 100 0 0"),
        )
        .unwrap();
        fs::write(p.join("cgroup"), format!("0::{cg}\n")).unwrap();
    }
    let weight = |i: usize| fs::read_to_string(&weights[i]).unwrap().trim().to_string();
    let _d = Guard(env.daemon_with("state", &["--tick-secs", "0.2"]));

    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        eventually("apps listed", async || {
            c.apps().await.unwrap()["running"]
                .as_array()
                .is_some_and(|a| a.len() == 2)
        })
        .await;
        assert_eq!(weight(0), "100", "nothing changes by default");

        // Bad lists are refused with a reason.
        let mut p = c.apps().await.unwrap()["policy"].clone();
        p["never_slow"] = serde_json::json!(["firefox"]);
        p["always_slow"] = serde_json::json!(["firefox"]);
        let err = c.set_app_policy(&p).await.unwrap_err();
        assert!(err.contains("both"), "{err}");

        p["never_slow"] = serde_json::json!([]);
        p["always_slow"] = serde_json::json!([]);
        p["slow_background"] = true.into();
        p["boost_front"] = true.into();
        c.set_app_policy(&p).await.unwrap();
        // Without knowing the app in front, nothing is slowed.
        sleep(Duration::from_millis(600));
        assert_eq!(weight(1), "100");

        // The session agent says Firefox is in front.
        let agent = env.client().await;
        let ap = zbus::Proxy::new(
            &agent,
            "net.dynamis.Power",
            "/net/dynamis/Power",
            "net.dynamis.Power1",
        )
        .await
        .unwrap();
        let report =
            HashMap::from([("focused_pid".to_string(), zbus::OwnedVariant::from(1001u32))]);
        ap.call::<_, _, ()>("ReportSession", &(report,))
            .await
            .unwrap();
        eventually("front boosted, background slowed", async || {
            weight(0) == "400" && weight(1) == "25"
        })
        .await;
        let a = c.apps().await.unwrap();
        assert_eq!(a["front"], "firefox");
        let j = c.journal(10).await.unwrap();
        assert!(j.to_string().contains("app-weight"), "{j}");

        // Restore puts both back and stops changing shares.
        c.restore().await.unwrap();
        assert_eq!((weight(0).as_str(), weight(1).as_str()), ("100", "100"));
        sleep(Duration::from_millis(600));
        assert_eq!((weight(0).as_str(), weight(1).as_str()), ("100", "100"));
        let p = &c.apps().await.unwrap()["policy"];
        assert_eq!(p["slow_background"], false);
    });
}

/// Phase 5: turbo off alone makes this laptop laggy, but once the speed
/// limit is at 80% or lower it costs no lag and saves power. Tuning them
/// together, one step at a time, finds that; testing each once does not.
fn interacting_laptop(_nmi_off: bool, turbo_off: bool, max_perf: f64) -> (f64, f64) {
    let watts = 6.0 - if turbo_off { 0.5 } else { 0.0 } - (100.0 - max_perf) / 10.0 * 0.25;
    let stall =
        0.5 + if turbo_off && max_perf > 80.0 {
            5.0
        } else {
            0.0
        } + if max_perf < 70.0 { 3.0 } else { 0.0 };
    (watts, stall)
}

#[test]
fn tuning_finds_settings_that_only_help_together() {
    let Some(env) = Env::new("tune") else { return };
    let stop = Arc::new(AtomicBool::new(false));
    let plant = spawn_model(env.dir.join("root"), stop.clone(), interacting_laptop);
    let _d = Guard(env.daemon_with(
        "state",
        &[
            "--tick-secs",
            "0.02",
            "--calib-block-secs",
            "0.3",
            "--calib-settle-secs",
            "0.05",
            "--calib-cooldown-secs",
            "0",
        ],
    ));
    let read = |rel: &str| {
        fs::read_to_string(env.dir.join("root").join(rel))
            .unwrap()
            .trim()
            .to_string()
    };
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        let p = c.status().await.unwrap()["learning"]["progress"].clone();
        assert_eq!(p["pct"], 0.0, "nothing tested yet: {p}");
        assert_eq!(p["stage"], "single");
        assert!(p["left"].as_u64().unwrap() >= 4, "{p}");
        assert!(p["hours_left"].as_f64().unwrap() > 0.0);
        c.set_learning(true).await.unwrap();
        // It rises as tests finish (it may step back a little when a step
        // together is kept and opens new ones).
        let mut seen = Vec::new();
        eventually_within(180, "tuning to finish", async || {
            let st = c.status().await.unwrap();
            seen.push(st["learning"]["progress"]["pct"].as_f64().unwrap_or(0.0));
            st["learning"]["status"]
                .as_str()
                .is_some_and(|s| s.contains("Every setting has been tested"))
        })
        .await;
        assert!(seen.iter().any(|p| *p > 0.0 && *p < 100.0), "{seen:?}");
        let p = &c.status().await.unwrap()["learning"]["progress"];
        assert_eq!(p["pct"], 100.0, "{p}");
        assert_eq!(p["stage"], "done");
        assert_eq!(p["left"], 0);
        assert!(p["done"].as_u64().unwrap() >= 8, "{p}");
        let l = c.learned().await.unwrap();
        let turbo: Vec<&str> = l["records"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["setting"] == "cpu.turbo")
            .map(|r| r["verdict"].as_str().unwrap())
            .collect();
        assert_eq!(turbo.first(), Some(&"reject"), "alone, turbo off adds lag");
        assert!(turbo.contains(&"keep"), "{turbo:?}");
        let learned: Vec<(String, String)> = l["learned"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["setting"].as_str().unwrap().to_string(),
                    r["value"].as_str().unwrap().to_string(),
                )
            })
            .filter(|(s, _)| s.starts_with("cpu."))
            .collect();
        assert!(
            learned.contains(&("cpu.turbo".into(), "off".into())),
            "{learned:?}"
        );
        assert!(
            learned.contains(&("cpu.max_perf_pct".into(), "70".into())),
            "{learned:?}"
        );
    });
    assert_eq!(read("sys/devices/system/cpu/intel_pstate/no_turbo"), "1");
    assert_eq!(
        read("sys/devices/system/cpu/intel_pstate/max_perf_pct"),
        "70"
    );

    // On mains both go back to where they started.
    fs::write(env.dir.join("root/sys/class/power_supply/AC0/online"), "1").unwrap();
    wait_for(|| {
        read("sys/devices/system/cpu/intel_pstate/no_turbo") == "0"
            && read("sys/devices/system/cpu/intel_pstate/max_perf_pct") == "100"
    });
    fs::write(env.dir.join("root/sys/class/power_supply/AC0/online"), "0").unwrap();
    wait_for(|| read("sys/devices/system/cpu/intel_pstate/max_perf_pct") == "70");

    // Forgetting one of them forgets the set.
    zbus::block_on(async {
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        assert!(c.forget("cpu.turbo").await.unwrap());
    });
    assert_eq!(read("sys/devices/system/cpu/intel_pstate/no_turbo"), "0");
    assert_eq!(
        read("sys/devices/system/cpu/intel_pstate/max_perf_pct"),
        "100"
    );
    stop.store(true, Ordering::Relaxed);
    plant.join().unwrap();
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[test]
fn last_until_caps_the_mode_and_ends_by_itself() {
    let Some(env) = Env::new("target") else {
        return;
    };
    let root = env.dir.join("root");
    let bat = root.join("sys/class/power_supply/BAT0");
    let _d = Guard(env.daemon_with("state", &["--tick-secs", "0.2"]));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        c.set_profile("performance").await.unwrap();
        assert_eq!(env.epp(), "performance");

        let err = c.set_target(unix_now() + 60.0).await.unwrap_err();
        assert!(err.contains("ten minutes"), "{err}");

        // 30 Wh must last 5 hours: about 5.6 W on average. Using 9 W is too
        // much, so full speed is the first thing to go.
        fs::write(bat.join("power_now"), "9000000").unwrap();
        c.set_target(unix_now() + 5.0 * 3600.0).await.unwrap();
        eventually("full speed capped", async || {
            env.epp() == "balance_performance"
        })
        .await;
        let s = c.status().await.unwrap();
        assert_eq!(s["last_until"]["level"], "no_boost", "{s}");
        assert_eq!(s["last_until"]["on_track"], false);
        let p = c.predict().await.unwrap();
        assert!(p["target"]["budget_w"].as_f64().unwrap() < 6.0, "{p}");

        // Ending it lifts the cap at once.
        c.set_target(0.0).await.unwrap();
        assert_eq!(env.epp(), "performance");

        // Plugging in ends it by itself.
        c.set_target(unix_now() + 5.0 * 3600.0).await.unwrap();
        eventually("capped again", async || env.epp() == "balance_performance").await;
        fs::write(root.join("sys/class/power_supply/AC0/online"), "1").unwrap();
        eventually("target ended", async || {
            c.predict().await.unwrap()["target_ended"] == "the charger was plugged in"
        })
        .await;
        assert_eq!(env.epp(), "performance");
        assert!(c.status().await.unwrap()["last_until"].is_null());
    });
}

#[test]
fn charging_follows_when_you_usually_unplug() {
    let Some(env) = Env::new("charge") else {
        return;
    };
    let bat = env.dir.join("root/sys/class/power_supply/BAT0");
    // Four weeks of unplugging every day about five hours from now.
    let state = env.dir.join("state");
    fs::create_dir_all(&state).unwrap();
    let soon = unix_now() + 5.0 * 3600.0;
    let unplugs: Vec<u64> = (1..=28)
        .map(|d| (soon - d as f64 * 86400.0) as u64)
        .collect();
    fs::write(
        state.join("habits.json"),
        serde_json::json!({"v": 1, "hours": [], "unplugs": unplugs}).to_string(),
    )
    .unwrap();
    let _d = Guard(env.daemon_with("state", &["--tick-secs", "0.2"]));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        // No charge limit control on this battery yet: refused.
        let err = c.set_adaptive_charging(true).await.unwrap_err();
        assert!(err.contains("no charge limit"), "{err}");

        fs::write(bat.join("charge_control_end_threshold"), "100").unwrap();
        let p = c.predict().await.unwrap();
        let usual = p["usual_unplug"].as_f64().unwrap();
        assert!((usual - soon).abs() < 120.0, "{p}");

        c.set_adaptive_charging(true).await.unwrap();
        eventually("held at 80%", async || {
            fs::read_to_string(bat.join("charge_control_end_threshold"))
                .unwrap()
                .trim()
                == "80"
        })
        .await;
        let p = c.predict().await.unwrap();
        assert_eq!(p["charge"]["limit_pct"], 80, "{p}");
        assert!(c
            .journal(5)
            .await
            .unwrap()
            .to_string()
            .contains("follows your habits"));

        // Off: the original limit comes back.
        c.set_adaptive_charging(false).await.unwrap();
        assert_eq!(
            fs::read_to_string(bat.join("charge_control_end_threshold"))
                .unwrap()
                .trim(),
            "100"
        );
    });
}

/// A StatusNotifierItem tooltip: icon name, pixmaps, title, text.
type ToolTip = (String, Vec<(i32, i32, Vec<u8>)>, String, String);

/// Stands in for the panel: records which icons registered.
struct Watcher(Arc<std::sync::Mutex<Vec<String>>>);

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&self, service: String) {
        self.0.lock().unwrap().push(service);
    }
}

#[test]
fn the_panel_icon_shows_and_changes_the_mode() {
    let Some(env) = Env::new("tray") else { return };
    // The agent is another package's program; it is built by a workspace
    // `cargo test` but not by `cargo test -p dynamisd` alone.
    let agent = Path::new(env!("CARGO_BIN_EXE_dynamisd")).with_file_name("dynamis-agent");
    if !agent.exists() {
        eprintln!("skipping: dynamis-agent is not built (run cargo test from daemon/)");
        return;
    }
    let _d = Guard(env.daemon("state"));
    let registered = Arc::new(std::sync::Mutex::new(Vec::new()));
    zbus::block_on(async {
        let panel = zbus::connection::Builder::address(env.address.as_str())
            .serve_at("/StatusNotifierWatcher", Watcher(registered.clone()))
            .name("org.kde.StatusNotifierWatcher")
            .build()
            .await
            .unwrap();
        wait_ready(&panel).await;
        let _agent = Guard(
            Command::new(&agent)
                .arg("--report-on-session-bus")
                .env("DBUS_SESSION_BUS_ADDRESS", &env.address)
                .env("XDG_RUNTIME_DIR", &env.dir)
                .env("XDG_CONFIG_HOME", env.dir.join("config"))
                .spawn()
                .unwrap(),
        );
        let dbus = zbus::fdo::DBusProxy::new(&panel).await.unwrap();
        eventually("icon registered", async || {
            // A call on this connection keeps it serving the watcher.
            let _ = dbus.get_id().await;
            !registered.lock().unwrap().is_empty()
        })
        .await;
        let name = registered.lock().unwrap()[0].clone();
        assert!(name.starts_with("org.kde.StatusNotifierItem-"), "{name}");

        let item = zbus::Proxy::new(
            &panel,
            name.as_str(),
            "/StatusNotifierItem",
            "org.kde.StatusNotifierItem",
        )
        .await
        .unwrap();
        let menu_path: zbus::OwnedObjectPath = item.get_property("Menu").await.unwrap();
        let menu = zbus::Proxy::new(&panel, name.as_str(), menu_path, "com.canonical.dbusmenu")
            .await
            .unwrap();
        let tooltip: ToolTip = item.get_property("ToolTip").await.unwrap();
        assert!(tooltip.3.starts_with("Balanced"), "{tooltip:?}");
        let icon: Vec<(i32, i32, Vec<u8>)> = item.get_property("IconPixmap").await.unwrap();
        assert!(icon.iter().any(|(w, _, _)| *w == 22));

        // The menu: modes as radio items, the current one ticked.
        let props: Vec<(i32, HashMap<String, zbus::OwnedVariant>)> = menu
            .call(
                "GetGroupProperties",
                &(vec![10i32, 11, 12], Vec::<String>::new()),
            )
            .await
            .unwrap();
        let state = |id: i32| -> i32 {
            props
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, p)| i32::try_from(&*p["toggle-state"]).unwrap())
                .unwrap()
        };
        assert_eq!((state(10), state(11), state(12)), (0, 1, 0));
        type Node = (
            i32,
            HashMap<String, zbus::OwnedVariant>,
            Vec<zbus::OwnedVariant>,
        );
        let (_rev, root): (u32, Node) = menu
            .call("GetLayout", &(0i32, -1i32, Vec::<String>::new()))
            .await
            .unwrap();
        assert!(root.2.len() >= 8, "header, modes, auto, last, open, hide…");

        // Clicking "Longer battery" switches the mode through the service.
        menu.call::<_, _, ()>(
            "Event",
            &(10i32, "clicked", zbus::Variant::from(0i32), 0u32),
        )
        .await
        .unwrap();
        wait_for(|| env.epp() == "power");
        // Panels read it again on NewToolTip; read it straight, uncached.
        let props = zbus::fdo::PropertiesProxy::new(&panel, name.as_str(), "/StatusNotifierItem")
            .await
            .unwrap();
        eventually("tooltip follows", async || {
            let v = props
                .get("org.kde.StatusNotifierItem".try_into().unwrap(), "ToolTip")
                .await
                .unwrap();
            let t: ToolTip = v.try_into().unwrap();
            t.3.starts_with("Longer battery")
        })
        .await;

        // "Hide this icon" removes it from the panel and is remembered.
        menu.call::<_, _, ()>(
            "Event",
            &(41i32, "clicked", zbus::Variant::from(0i32), 0u32),
        )
        .await
        .unwrap();
        eventually("icon gone", async || {
            !dbus
                .name_has_owner(name.as_str().try_into().unwrap())
                .await
                .unwrap()
        })
        .await;
        let saved = fs::read_to_string(env.dir.join("config/dynamis-power/agent.json")).unwrap();
        assert!(saved.contains("\"tray\":false"), "{saved}");
    });
}

#[test]
fn advanced_profiles_sensors_system_and_developer() {
    let Some(env) = Env::new("advanced") else {
        return;
    };
    let _d = Guard(env.daemon("state"));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();

        // Profiles: Balanced uses balance_performance by default; choose
        // balance_power instead, and it applies at once.
        let p = c.profiles().await.unwrap();
        let bal = p["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["profile"] == "balanced")
            .unwrap()
            .clone();
        let epp = bal["settings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "cpu.epp")
            .unwrap()
            .clone();
        assert_eq!(epp["default"], "balance_performance");
        assert!(epp["choices"]
            .as_array()
            .unwrap()
            .contains(&"balance_power".into()));
        c.set_profile("balanced").await.unwrap();
        let ov =
            serde_json::json!({"v": 1, "profiles": {"balanced": {"cpu.epp": "balance_power"}}});
        c.set_profile_overrides(&ov).await.unwrap();
        assert_eq!(env.epp(), "balance_power");
        let bad = serde_json::json!({"v": 1, "profiles": {"balanced": {"cpu.epp": "turbo-max"}}});
        assert!(c
            .set_profile_overrides(&bad)
            .await
            .unwrap_err()
            .contains("not offered"));
        let bad = serde_json::json!({"v": 1, "profiles": {"balanced": {"vm.swappiness": "1"}}});
        assert!(c.set_profile_overrides(&bad).await.is_err());

        // Sensors and System: readings only.
        let s = c.sensors().await.unwrap();
        assert!(s["battery_w"].as_f64().is_some(), "{s}");
        assert!(s["thermal"].is_array());
        let sys = c.system().await.unwrap();
        assert!(sys["memory_mb"].as_f64().is_some() || sys["memory_mb"].is_null());
        assert_eq!(sys["version"], c.status().await.unwrap()["version"]);

        // Developer: advisor text, and dry-run journals instead of writing.
        let tlp = c.advise("tlp").await.unwrap();
        assert!(
            tlp.contains("CPU_ENERGY_PERF_POLICY_ON_AC=balance_power"),
            "{tlp}"
        );
        assert!(c.advise("windows").await.is_err());
        c.set_dry_run(true).await.unwrap();
        assert_eq!(c.status().await.unwrap()["dry_run"], true);
        c.set_profile("power-saver").await.unwrap();
        assert_eq!(env.epp(), "balance_power", "dry run writes nothing");
        let j = c.journal(5).await.unwrap().to_string();
        assert!(j.contains("\"event\":\"would\""), "{j}");
        c.set_dry_run(false).await.unwrap();
        c.set_profile("balanced").await.unwrap();
        c.set_profile("power-saver").await.unwrap();
        assert_eq!(env.epp(), "power");
    });
    let saved = fs::read_to_string(env.dir.join("state/profiles.json")).unwrap();
    assert!(saved.contains("balance_power"));
}

#[test]
fn settings_set_by_hand_survive_sleep_and_restarts() {
    let Some(env) = Env::new("kept") else { return };
    let root = env.dir.join("root");
    let bat = root.join("sys/class/power_supply/BAT0");
    fs::write(bat.join("charge_control_end_threshold"), "100").unwrap();
    let limit = || {
        fs::read_to_string(bat.join("charge_control_end_threshold"))
            .unwrap()
            .trim()
            .to_string()
    };
    let swappiness = || {
        fs::read_to_string(root.join("proc/sys/vm/swappiness"))
            .unwrap()
            .trim()
            .to_string()
    };
    let mut d = Guard(env.daemon_with("state", &["--tick-secs", "0.2"]));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        // The charge limit is always kept; swappiness when asked.
        c.set_setting("battery.charge_limit", "80", false)
            .await
            .unwrap();
        c.set_setting("vm.swappiness", "20", false).await.unwrap();
        c.set_kept("vm.swappiness", true).await.unwrap();
        let kept = c.status().await.unwrap()["kept"]["settings"].clone();
        assert_eq!(kept.as_array().unwrap().len(), 2, "{kept}");

        // Firmware resets the limit during sleep; waking applies it again.
        fs::write(bat.join("charge_control_end_threshold"), "100").unwrap();
        let login = zbus::connection::Builder::address(env.address.as_str())
            .build()
            .await
            .unwrap();
        zbus::object_server::SignalEmitter::new(&login, "/org/freedesktop/login1")
            .unwrap()
            .emit(
                "org.freedesktop.login1.Manager",
                "PrepareForSleep",
                &(false,),
            )
            .await
            .unwrap();
        eventually("limit applied after waking", async || {
            let _ = c.status().await;
            limit() == "80"
        })
        .await;
    });

    // A crash (killed, so not a clean stop) while the settings are new: on
    // probation, they are not applied again at the next start.
    drop(d);
    fs::write(bat.join("charge_control_end_threshold"), "100").unwrap();
    fs::write(root.join("proc/sys/vm/swappiness"), "60").unwrap();
    // A new boot, as far as the service can tell.
    fs::create_dir_all(root.join("proc/sys/kernel/random")).unwrap();
    fs::write(root.join("proc/sys/kernel/random/boot_id"), "second-boot").unwrap();
    // The previous boot's kernel log ends just after waking, with a USB reset.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let log = root.join("var/log/dynamis-test");
    fs::create_dir_all(&log).unwrap();
    fs::write(
        log.join("previous-boot-kernel.log"),
        format!(
            "{:.6} laptop kernel: PM: suspend exit\n{:.6} laptop kernel: usb 1-2: reset high-speed USB device number 3 using xhci_hcd\n",
            now - 20.0,
            now - 5.0
        ),
    )
    .unwrap();
    d = Guard(env.daemon_with("state", &["--tick-secs", "0.2"]));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        let k = c.status().await.unwrap()["kept"].clone();
        assert_eq!(k["held_back"].as_array().unwrap().len(), 2, "{k}");
        // What was happening, for Home.
        let u = c.status().await.unwrap()["unclean"].clone();
        assert_eq!(u["last"]["kernel"], "waking", "{u}");
        assert!(u["last"]["detail"].as_str().unwrap().contains("reset"));
        let recent: Vec<&str> = u["last"]["recent"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert!(
            recent.contains(&"battery.charge_limit") && recent.contains(&"vm.swappiness"),
            "{u}"
        );
        assert_eq!(u["suspects"].as_array().unwrap().len(), 2, "{u}");
    });
    assert_eq!((limit().as_str(), swappiness().as_str()), ("100", "60"));

    // A clean stop (the marker removed, as systemd does): applied at start.
    drop(d);
    fs::remove_file(env.dir.join("state/running")).unwrap();
    fs::write(root.join("proc/sys/kernel/random/boot_id"), "third-boot").unwrap();
    let _d = Guard(env.daemon_with("state", &["--tick-secs", "0.2"]));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
    });
    assert_eq!((limit().as_str(), swappiness().as_str()), ("80", "20"));

    // Plugged in and charging past the limit: the firmware ignores it.
    fs::write(root.join("sys/class/power_supply/AC0/online"), "1").unwrap();
    fs::write(bat.join("status"), "Charging").unwrap();
    fs::write(bat.join("capacity"), "90").unwrap();
    zbus::block_on(async {
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        eventually("limit seen as ignored", async || {
            c.status().await.unwrap()["kept"]["charge_limit_ignored"] == true
        })
        .await;
        // Restore stops keeping anything.
        c.restore().await.unwrap();
        assert!(c.status().await.unwrap()["kept"]["settings"]
            .as_array()
            .unwrap()
            .is_empty());
    });
}

/// Another tool changing a setting Dynamis Power set: journaled with the
/// likely tool, and after the second time in a day left to that tool.
#[test]
fn settings_changed_by_another_tool_are_noticed_and_left_alone() {
    let Some(env) = Env::new("outside") else {
        return;
    };
    let root = env.dir.join("root");
    let swappiness = root.join("proc/sys/vm/swappiness");
    let _d = Guard(env.daemon_with("state", &["--tick-secs", "0.2"]));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        // Readings run while auto mode is on.
        c.set_auto(true).await.unwrap();
        c.set_setting("vm.swappiness", "20", false).await.unwrap();
        c.set_kept("vm.swappiness", true).await.unwrap();
        // Let a few readings see our own change first.
        std::thread::sleep(std::time::Duration::from_millis(1500));

        // TuneD is running and sets it back.
        let p = root.join("proc/4321");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("comm"), "tuned\n").unwrap();
        fs::write(&swappiness, "60").unwrap();
        let outside = async || {
            c.journal(50)
                .await
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["event"] == "outside" && e["id"] == "vm.swappiness")
                .count()
        };
        eventually("first outside change journaled", async || {
            outside().await == 1
        })
        .await;
        let j = c.journal(50).await.unwrap();
        let e = j
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["event"] == "outside")
            .unwrap();
        assert_eq!(
            (e["from"].as_str(), e["to"].as_str()),
            (Some("20"), Some("60"))
        );
        assert!(e["reason"].as_str().unwrap().contains("TuneD"), "{e}");
        // Not blocked yet: setting it again works.
        c.set_setting("vm.swappiness", "20", false).await.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1500));

        // The second time in a day: left to TuneD.
        fs::write(&swappiness, "60").unwrap();
        eventually("second outside change blocks it", async || {
            let rows = c.settings().await.unwrap();
            rows.as_array().unwrap().iter().any(|r| {
                r["id"] == "vm.swappiness"
                    && r["blocked"].as_str().is_some_and(|b| b.contains("TuneD"))
            })
        })
        .await;
        assert!(c.set_setting("vm.swappiness", "20", false).await.is_err());
        let kept = c.status().await.unwrap()["kept"]["settings"].clone();
        assert!(
            kept.as_array().unwrap().is_empty(),
            "no longer kept: {kept}"
        );
        assert_eq!(
            fs::read_to_string(&swappiness).unwrap().trim(),
            "60",
            "never fights"
        );
    });
}

/// Idle devices sleep on battery once switched on; sensitive ones never;
/// newly plugged ones follow; the charger and Restore put them back.
#[test]
fn idle_devices_sleep_on_battery() {
    use dynamis_core::devices::fixture;
    let Some(env) = Env::new("devices") else {
        return;
    };
    let root = env.dir.join("root");
    fixture::usb(&root, "1-1", "Webcam", "on", &[("0e", "uvcvideo")]);
    fixture::usb(&root, "1-2", "Mouse", "on", &[("03", "usbhid")]);
    fixture::pci(&root, "0000:00:02.0", "0x030000", "on", "i915");
    fixture::pci(&root, "0000:00:14.0", "0x0c0330", "on", "xhci_hcd");
    let control = |p: &str| {
        fs::read_to_string(root.join(p.trim_start_matches('/')).join("power/control"))
            .unwrap()
            .trim()
            .to_string()
    };
    let cam = "sys/bus/usb/devices/1-1";
    let mouse = "sys/bus/usb/devices/1-2";
    let gpu = "sys/bus/pci/devices/0000:00:02.0";
    let xhci = "sys/bus/pci/devices/0000:00:14.0";
    let _d = Guard(env.daemon_with("state", &["--tick-secs", "0.2"]));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        let info = c.devices().await.unwrap();
        assert_eq!(info["devices"].as_array().unwrap().len(), 4, "{info}");
        c.set_device_policy(&serde_json::json!({"usb": true, "pci": true}))
            .await
            .unwrap();
        // On battery: applied at once.
        assert_eq!(
            [control(cam), control(mouse), control(gpu), control(xhci)],
            ["auto", "on", "on", "auto"]
        );
        let info = c.devices().await.unwrap();
        let mouse_info = info["devices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == "usb:1-2")
            .unwrap()
            .clone();
        assert_eq!(mouse_info["excluded"], "input");

        // The person keeps the webcam awake: it goes back to its own value.
        let cam_model = info["devices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == "usb:1-1")
            .unwrap()["model"]
            .clone();
        assert!(cam_model.is_string(), "{info}");
        c.set_device_policy(&serde_json::json!({"usb": true, "pci": true, "never": [cam_model]}))
            .await
            .unwrap();
        assert_eq!(control(cam), "on");
        c.set_device_policy(&serde_json::json!({"usb": true, "pci": true}))
            .await
            .unwrap();
        assert_eq!(control(cam), "auto");

        // A new device plugged in follows at the next reading.
        fixture::usb(&root, "1-3", "Card reader", "on", &[("08", "usb-storage")]);
        eventually("new device sleeps", async || {
            control("sys/bus/usb/devices/1-3") == "auto"
        })
        .await;

        // Plugged in: back to their own values.
        fs::write(root.join("sys/class/power_supply/AC0/online"), "1").unwrap();
        eventually("back on mains", async || {
            control(cam) == "on" && control(xhci) == "on"
        })
        .await;
        fs::write(root.join("sys/class/power_supply/AC0/online"), "0").unwrap();
        eventually("battery again", async || control(cam) == "auto").await;

        // The diagnostic report shows the service and the devices.
        let r = c.diagnostics().await.unwrap();
        assert!(r.contains("Running:        yes"), "{r}");
        assert!(
            r.contains("usb:1-2") && r.contains("kept awake: input"),
            "{r}"
        );

        // Restore: everything back and the switches off.
        c.restore().await.unwrap();
        assert_eq!([control(cam), control(xhci)], ["on", "on"]);
        let info = c.devices().await.unwrap();
        assert_eq!(
            info["policy"],
            serde_json::json!({"usb": false, "pci": false, "never": []})
        );
    });
}

/// "Charge fully once" lifts the limit until the battery is full, or the
/// charger is unplugged, then puts it back.
#[test]
fn charge_fully_once_then_back_to_the_limit() {
    let Some(env) = Env::new("fullcharge") else {
        return;
    };
    let root = env.dir.join("root");
    let bat = root.join("sys/class/power_supply/BAT0");
    let ac = root.join("sys/class/power_supply/AC0/online");
    fs::write(bat.join("charge_control_end_threshold"), "100").unwrap();
    let limit = || {
        fs::read_to_string(bat.join("charge_control_end_threshold"))
            .unwrap()
            .trim()
            .to_string()
    };
    let _d = Guard(env.daemon_with("state", &["--tick-secs", "0.2"]));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        c.set_setting("battery.charge_limit", "80", false)
            .await
            .unwrap();
        // Plugged in at 85%.
        fs::write(&ac, "1").unwrap();
        fs::write(bat.join("status"), "Charging").unwrap();
        fs::write(bat.join("capacity"), "85").unwrap();
        c.charge_fully_once(true).await.unwrap();
        assert_eq!(limit(), "100");
        assert_eq!(c.status().await.unwrap()["full_charge"]["active"], true);

        // Full: back to 80.
        fs::write(bat.join("capacity"), "100").unwrap();
        fs::write(bat.join("status"), "Full").unwrap();
        eventually("limit back after full", async || limit() == "80").await;
        let fc = c.status().await.unwrap()["full_charge"].clone();
        assert_eq!(fc["active"], false);
        assert_eq!(fc["ended"], "the battery is full");

        // Again, then unplugged before full.
        fs::write(bat.join("capacity"), "82").unwrap();
        fs::write(bat.join("status"), "Charging").unwrap();
        c.charge_fully_once(true).await.unwrap();
        assert_eq!(limit(), "100");
        fs::write(&ac, "0").unwrap();
        fs::write(bat.join("status"), "Discharging").unwrap();
        eventually("limit back after unplugging", async || limit() == "80").await;

        // Setting the limit by hand ends it without fighting.
        fs::write(&ac, "1").unwrap();
        c.charge_fully_once(true).await.unwrap();
        c.set_setting("battery.charge_limit", "90", false)
            .await
            .unwrap();
        assert_eq!(c.status().await.unwrap()["full_charge"]["active"], false);
        std::thread::sleep(std::time::Duration::from_millis(600));
        assert_eq!(limit(), "90");
    });
}

/// A charger that reports wrongly can be ignored (TLP's `TLP_PS_IGNORE`).
#[test]
fn a_wrong_charger_report_can_be_ignored() {
    let Some(env) = Env::new("pssource") else {
        return;
    };
    let root = env.dir.join("root");
    fs::write(root.join("sys/class/power_supply/AC0/online"), "1").unwrap();
    let _d = Guard(env.daemon_with("state", &[]));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        assert_eq!(c.status().await.unwrap()["power"]["on_ac"], true);
        let mut prefs = c.auto_prefs().await.unwrap();
        assert_eq!(prefs["power_source"], "detect");
        prefs["power_source"] = "battery".into();
        c.set_auto_prefs(&prefs).await.unwrap();
        assert_eq!(c.status().await.unwrap()["power"]["on_ac"], false);
        assert!(c
            .diagnostics()
            .await
            .unwrap()
            .contains("always treated as on battery"));
        prefs["power_source"] = "detect".into();
        c.set_auto_prefs(&prefs).await.unwrap();
        assert_eq!(c.status().await.unwrap()["power"]["on_ac"], true);
    });
}

/// Stands in for logind: counts requests to sleep.
struct FakeLogin(Arc<std::sync::atomic::AtomicU32>);

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl FakeLogin {
    fn suspend(&self, _interactive: bool) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// The guided sleep test sleeps with each type, measures, notices a type
/// that does not wake cleanly, recommends the other and puts things back.
#[test]
fn the_sleep_test_compares_sleep_types() {
    let Some(env) = Env::new("sleeptest") else {
        return;
    };
    let root = env.dir.join("root");
    let bat = root.join("sys/class/power_supply/BAT0");
    let rtc = root.join("sys/class/rtc/rtc0");
    fs::create_dir_all(&rtc).unwrap();
    fs::write(rtc.join("wakealarm"), "").unwrap();
    fs::create_dir_all(root.join("dev")).unwrap();
    fs::write(root.join("dev/kmsg"), "").unwrap();
    let mem_sleep = || fs::read_to_string(root.join("sys/power/mem_sleep")).unwrap();
    let _d = Guard(env.daemon_with(
        "state",
        &["--tick-secs", "0.2", "--sleep-test-minute-secs", "0.5"],
    ));
    let sleeps = Arc::new(std::sync::atomic::AtomicU32::new(0));
    zbus::block_on(async {
        let login = zbus::connection::Builder::address(env.address.as_str())
            .serve_at("/org/freedesktop/login1", FakeLogin(sleeps.clone()))
            .name("org.freedesktop.login1")
            .build()
            .await
            .unwrap();
        wait_ready(&login).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        let wake = async |energy_uwh: u64| {
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(1100) {
                let _ = zbus::fdo::DBusProxy::new(&login)
                    .await
                    .unwrap()
                    .get_id()
                    .await;
                std::thread::sleep(Duration::from_millis(50));
            }
            fs::write(bat.join("energy_now"), energy_uwh.to_string()).unwrap();
            // The kernel lists every type, bracketing the active one; a
            // fixture file holds only what was written.
            let active = fs::read_to_string(root.join("sys/power/mem_sleep")).unwrap();
            let shown = if active.contains("s2idle") {
                "[s2idle] deep"
            } else {
                "s2idle [deep]"
            };
            fs::write(root.join("sys/power/mem_sleep"), shown).unwrap();
            zbus::object_server::SignalEmitter::new(&login, "/org/freedesktop/login1")
                .unwrap()
                .emit(
                    "org.freedesktop.login1.Manager",
                    "PrepareForSleep",
                    &(false,),
                )
                .await
                .unwrap();
        };

        // Plugged in: refused.
        fs::write(root.join("sys/class/power_supply/AC0/online"), "1").unwrap();
        assert!(c.start_sleep_test(2.0).await.is_err());
        fs::write(root.join("sys/class/power_supply/AC0/online"), "0").unwrap();

        c.start_sleep_test(2.0).await.unwrap();
        // First type: s2idle, with the alarm set.
        eventually("first sleep", async || {
            // Keep the stand-in serving while waiting.
            let _ = zbus::fdo::DBusProxy::new(&login)
                .await
                .unwrap()
                .get_id()
                .await;
            sleeps.load(Ordering::SeqCst) == 1
        })
        .await;
        assert!(mem_sleep().contains("s2idle"), "{}", mem_sleep());
        assert_ne!(
            fs::read_to_string(rtc.join("wakealarm")).unwrap().trim(),
            "0"
        );
        let st = c.sleep_test().await.unwrap();
        assert_eq!(
            (st["phase"].as_str(), st["current"].as_str()),
            (Some("sleeping"), Some("s2idle"))
        );
        wake(29_990_000).await;

        // Second type: deep, which logs a failed resume.
        eventually("second sleep", async || {
            // Keep the stand-in serving while waiting.
            let _ = zbus::fdo::DBusProxy::new(&login)
                .await
                .unwrap()
                .get_id()
                .await;
            sleeps.load(Ordering::SeqCst) == 2
        })
        .await;
        assert!(mem_sleep().contains("deep"));
        fs::write(
            root.join("dev/kmsg"),
            "3,900,123,-;PM: Device 0000:00:14.0 failed to resume async: error -110\n",
        )
        .unwrap();
        wake(29_985_000).await;

        eventually("finished", async || {
            c.sleep_test().await.unwrap()["phase"] == "done"
        })
        .await;
        let st = c.sleep_test().await.unwrap();
        let results = st["results"].as_array().unwrap();
        assert_eq!(results.len(), 2, "{st}");
        assert_eq!(results[0]["kind"], "s2idle");
        assert_eq!(results[0]["clean"], true, "{st}");
        assert!(results[0]["wh_per_h"].as_f64().unwrap() > 0.0, "{st}");
        assert_eq!(results[1]["clean"], false, "{st}");
        assert!(results[1]["problems"][0]
            .as_str()
            .unwrap()
            .contains("failed to resume"));
        assert_eq!(st["recommended"], "s2idle");
        // The original type is back, and the alarm is cleared.
        assert!(mem_sleep().contains("deep"), "{}", mem_sleep());
        assert_eq!(
            fs::read_to_string(rtc.join("wakealarm")).unwrap().trim(),
            "0"
        );
    });
}

/// The brightness chosen for a mode is set when that mode starts, once:
/// the person's own changes in between are left alone.
#[test]
fn brightness_follows_the_mode() {
    let Some(env) = Env::new("brightness") else {
        return;
    };
    let file = env
        .dir
        .join("root/sys/class/backlight/intel_backlight/brightness");
    let level = || fs::read_to_string(&file).unwrap().trim().to_string();
    let _d = Guard(env.daemon_with("state", &[]));
    zbus::block_on(async {
        let conn = env.client().await;
        wait_ready(&conn).await;
        let c = dynamis_client::Client::connect_to(&env.address)
            .await
            .unwrap();
        let b = c.brightness().await.unwrap();
        assert_eq!(b["supported"], true, "{b}");
        assert_eq!(b["now_pct"], 50.0, "{b}");
        c.set_brightness_prefs(&serde_json::json!({"levels": {"power-saver": 30, "balanced": 60}}))
            .await
            .unwrap();
        // Balanced is in use: set at once.
        assert_eq!(level(), "28800");
        c.set_profile("power-saver").await.unwrap();
        assert_eq!(level(), "14400");
        // The person brightens by hand; Faster has no level: left alone.
        fs::write(&file, "40000").unwrap();
        c.set_profile("performance").await.unwrap();
        assert_eq!(level(), "40000");
        assert!(c
            .set_brightness_prefs(&serde_json::json!({"levels": {"balanced": 0}}))
            .await
            .is_err());
        // Restore forgets the choices and leaves the screen as it is.
        c.restore().await.unwrap();
        assert_eq!(
            c.brightness().await.unwrap()["levels"],
            serde_json::json!({})
        );
        c.set_profile("balanced").await.unwrap();
        assert_eq!(level(), "40000");
        let j = c.journal(20).await.unwrap();
        assert!(
            j.as_array()
                .unwrap()
                .iter()
                .any(|e| e["event"] == "brightness"),
            "{j}"
        );
    });
}
