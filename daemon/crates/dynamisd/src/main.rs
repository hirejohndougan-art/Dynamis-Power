//! `dynamisd` — the Dynamis Power system service.
//!
//! What it does today (phase 2):
//!
//! 1. At start, finishes or undoes any change an earlier run left unfinished
//!    (a crash, a freeze, a power cut) and blocks that setting.
//! 2. Applies the profile the user last chose.
//! 3. Serves the standard power-profiles D-Bus interface, so the desktop's
//!    power slider and `powerprofilesctl` work with it.
//!
//! It never wakes up on its own: with nobody calling it, it sleeps until a
//! D-Bus message arrives. It has no network access (see the systemd unit).

mod api;
mod apps;
mod bus;
mod conflicts;
mod core;
mod crash;
mod devices;
mod kept;
mod learn;
mod outside;
mod predict;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::Parser;
use dynamis_core::engine::DEFAULT_STATE_DIR;
use dynamis_core::SysRoot;
use futures_lite::StreamExt;

use crate::bus::{
    DynamisPower, HadessProfiles, Service, Ticker, UPowerProfiles, BUS_NAMES, OBJECTS, OWN_NAME,
    OWN_PATH,
};
use crate::core::Core;

#[derive(Parser)]
#[command(name = "dynamisd", version, about = "Dynamis Power system service")]
struct Args {
    /// Read and write under this directory instead of the real system (testing).
    #[arg(long, value_name = "DIR", hide = true)]
    root: Option<PathBuf>,
    /// Keep state here instead of /var/lib/dynamis-power.
    #[arg(long, value_name = "DIR", hide = true)]
    state_dir: Option<PathBuf>,
    /// Seconds between auto-mode readings (four times longer while idle).
    #[arg(long, default_value_t = 15.0, hide = true)]
    tick_secs: f64,
    /// Calibration timing (testing only): block length, settle time, rest.
    #[arg(long, default_value_t = 60.0, hide = true)]
    calib_block_secs: f64,
    #[arg(long, default_value_t = 10.0, hide = true)]
    calib_settle_secs: f64,
    #[arg(long, default_value_t = 300.0, hide = true)]
    calib_cooldown_secs: f64,
    /// Use the session bus and skip polkit (testing only; needs --root, so it
    /// can never change the real system without polkit).
    #[arg(long, hide = true)]
    session: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    if args.session && args.root.is_none() {
        eprintln!("dynamisd: --session is for tests and needs --root pointing at a test copy");
        return ExitCode::FAILURE;
    }
    match zbus::block_on(run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dynamisd: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), String> {
    let root = args.root.map(SysRoot::at).unwrap_or_else(SysRoot::system);
    let state = args
        .state_dir
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_DIR));

    std::fs::create_dir_all(&state).map_err(|e| format!("{}: {e}", state.display()))?;
    let boot = dynamis_core::boots::record_start(&root, &state);
    let after_unclean = boot.as_ref().is_ok_and(|b| b.unclean_in_a_row > 0);
    match &boot {
        Ok(b) if b.safe_mode_now_on => eprintln!(
            "dynamisd: {} unclean shutdowns in a row; SAFE MODE is now on and stays on until \
             `sudo dynamis safe-mode off`",
            b.unclean_in_a_row
        ),
        Ok(b) if b.unclean_in_a_row > 0 => eprintln!(
            "dynamisd: the last {} shutdown(s) were not clean (freeze, crash or power cut)",
            b.unclean_in_a_row
        ),
        Ok(_) => {}
        Err(e) => eprintln!("dynamisd: could not record this boot: {e}"),
    }

    // Recovery happens here, before anything else touches a setting.
    // App shares left from a run that did not stop cleanly go back too.
    let mut engine = crate::core::open_engine(&root, &state)?;
    if let Err(e) = engine.restore_app_weights() {
        eprintln!("dynamisd: could not put app shares back: {e}");
    }
    // Devices left changed with the switches off (a crash, or switched off
    // while the service was stopped) go back too.
    if !crate::devices::DevicesState::load(&state).enabled() {
        if let Err(e) = engine.restore_device_power() {
            eprintln!("dynamisd: could not put devices back: {e}");
        }
    }
    // After an unclean shutdown: what was happening, for the app.
    if let Ok(b) = &boot {
        let in_progress = engine.recovered().map(|r| r.id.clone());
        if let Some(d) = crash::diagnose(
            &root,
            &state,
            b,
            in_progress,
            &engine.journal(500),
            crate::core::unix_now(),
        ) {
            eprintln!(
                "dynamisd: the last boot ended uncleanly; kernel log: {:?}{}; changed in the ten minutes before: {:?}",
                d.kernel,
                d.detail.as_deref().map(|t| format!(" ({t})")).unwrap_or_default(),
                d.recent
            );
        }
    }
    drop(engine);
    let core = Core::with_timing(
        root,
        state,
        crate::learn::Timing {
            block_s: args.calib_block_secs,
            settle_s: args.calib_settle_secs,
            cooldown_s: args.calib_cooldown_secs,
        },
    );
    let svc = Service {
        core: Arc::new(Mutex::new(core)),
        polkit: !args.session,
        ticker: Arc::new(Ticker::default()),
        epoch: std::time::Instant::now(),
    };

    // Claim the bus names *before* applying anything, so a second copy (or
    // power-profiles-daemon) is detected before it can change a setting.
    // zbus's default is to take over an existing owner and to let others take
    // over from us; both are turned off.
    let builder = if args.session {
        zbus::connection::Builder::session()
    } else {
        zbus::connection::Builder::system()
    };
    let conn = builder
        .serve_at(OBJECTS[0].0, UPowerProfiles(svc.clone()))
        .serve_at(OBJECTS[1].0, HadessProfiles(svc.clone()))
        .serve_at(OWN_PATH, DynamisPower(svc.clone()))
        .name(BUS_NAMES[0])
        .name(BUS_NAMES[1])
        .name(OWN_NAME)
        .replace_existing_names(false)
        .allow_name_replacements(false)
        .build()
        .await
        .map_err(|e| match e {
            zbus::Error::NameTaken => {
                "another power profile service (power-profiles-daemon, tuned-ppd \
                 or a second dynamisd) is running. To switch to Dynamis Power: \
                 sudo systemctl disable --now power-profiles-daemon"
                    .to_string()
            }
            other => format!("could not connect to D-Bus: {other}"),
        })?;

    {
        let mut core = svc.core.lock().unwrap_or_else(|p| p.into_inner());
        let names: Vec<&str> = core.available().iter().map(|p| p.as_str()).collect();
        eprintln!("dynamisd: profiles here: {}", names.join(", "));
        match core.apply_active("startup") {
            Ok(()) => eprintln!("dynamisd: active profile {}", core.active().as_str()),
            Err(e) => eprintln!("dynamisd: {e}"),
        }
        // Settings the person chose by hand (charge limit and so on).
        let (root, dir) = (core.root().clone(), core.state_dir.clone());
        let n = core
            .kept
            .apply_all(&root, &dir, after_unclean, "at startup");
        if n > 0 {
            eprintln!("dynamisd: applied {n} kept setting(s)");
        }
    }
    eprintln!("dynamisd: ready");
    spawn_auto_thread(conn.clone(), svc.clone(), args.tick_secs);
    spawn_resume_watch(conn.clone(), svc.clone());

    // Holds end when the app that made them leaves the bus. This loop is also
    // what keeps the service running; it sleeps between messages.
    let dbus = zbus::fdo::DBusProxy::new(&conn)
        .await
        .map_err(|e| e.to_string())?;
    let mut gone = dbus
        .receive_name_owner_changed()
        .await
        .map_err(|e| e.to_string())?;
    while let Some(signal) = gone.next().await {
        let Ok(args) = signal.args() else { continue };
        if args.new_owner().is_some() || !args.name().starts_with(':') {
            continue;
        }
        let released = {
            let mut core = svc.core.lock().unwrap_or_else(|p| p.into_inner());
            core.session_gone(args.name());
            core.release_owner(args.name())
        };
        if !released.is_empty() {
            for cookie in released {
                bus::emit_released(&conn, cookie).await;
            }
            bus::notify(&conn, &svc).await;
        }
    }
    Err("lost the D-Bus connection".into())
}

/// Waking from sleep: logind says `PrepareForSleep(false)`. Firmware may
/// have reset settings and devices may have changed, so apply everything
/// again. Listens on the service's own bus (the system bus; a test's private
/// bus under `--session`).
fn spawn_resume_watch(conn: zbus::Connection, svc: Service) {
    std::thread::spawn(move || {
        zbus::block_on(async move {
            let rule = match zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("org.freedesktop.login1.Manager")
                .member("PrepareForSleep")
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("dynamisd: cannot watch for sleep: {e}");
                    return;
                }
            };
            let Ok(mut stream) = zbus::MessageStream::for_match_rule(rule, &conn, None).await
            else {
                eprintln!("dynamisd: cannot watch for sleep");
                return;
            };
            while let Some(Ok(msg)) = stream.next().await {
                let Ok(going) = msg.body().deserialize::<bool>() else {
                    continue;
                };
                if going {
                    continue;
                }
                eprintln!("dynamisd: woke from sleep; applying settings again");
                svc.core
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .after_resume();
                svc.ticker.poke();
                bus::notify(&conn, &svc).await;
            }
        });
    });
}

/// Auto mode's readings run on their own thread. While auto mode is off the
/// thread sleeps until woken, so the daemon still never wakes up on its own.
fn spawn_auto_thread(conn: zbus::Connection, svc: Service, tick_secs: f64) {
    let base = std::time::Duration::from_secs_f64(tick_secs.clamp(0.2, 600.0));
    std::thread::spawn(move || {
        let mut last_said = String::new();
        loop {
            let (enabled, interval) = {
                let c = svc.core.lock().unwrap_or_else(|p| p.into_inner());
                (c.needs_ticks(), c.auto_interval(base))
            };
            svc.ticker.wait(enabled.then_some(interval));
            let now = svc.epoch.elapsed().as_secs_f64();
            let (changed, said) = {
                let mut c = svc.core.lock().unwrap_or_else(|p| p.into_inner());
                let changed = c.tick(now);
                (
                    changed,
                    format!("{}|{}", c.auto_status().1, c.learn.status()),
                )
            };
            // Signal only real changes, so listeners are not woken every tick.
            if changed {
                zbus::block_on(bus::notify(&conn, &svc));
            }
            if changed || said != last_said {
                zbus::block_on(bus::notify_auto(&conn, &svc));
                last_said = said;
            }
        }
    });
}
