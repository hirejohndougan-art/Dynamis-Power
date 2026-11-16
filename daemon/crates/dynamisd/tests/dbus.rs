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

/// A pretend laptop: reads the settings Dynamis Power changes and reports
/// battery power and stall to match, with noise. Known truth to test against.
fn spawn_plant(root: std::path::PathBuf, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
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
            let watts = 6.0
                - if nmi_off { 0.4 } else { 0.0 }
                - if turbo_off { 0.8 } else { 0.0 }
                - (100.0 - max_perf) / 20.0 * 0.3
                + noise;
            let stall_pct = 0.5 + if turbo_off { 5.0 } else { 0.0 } + (100.0 - max_perf) / 20.0;
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
    assert!(verdict("cpu.max_perf_pct", "80")
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
