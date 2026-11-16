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

mod bus;
mod core;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::Parser;
use dynamis_core::engine::DEFAULT_STATE_DIR;
use dynamis_core::SysRoot;
use futures_lite::StreamExt;

use crate::bus::{HadessProfiles, Service, UPowerProfiles, BUS_NAMES, OBJECTS};
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
    match dynamis_core::boots::record_start(&root, &state) {
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
    crate::core::open_engine(&root, &state)?;
    let core = Core::new(root, state);
    let svc = Service {
        core: Arc::new(Mutex::new(core)),
        polkit: !args.session,
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
        .name(BUS_NAMES[0])
        .name(BUS_NAMES[1])
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
        let core = svc.core.lock().unwrap_or_else(|p| p.into_inner());
        let names: Vec<&str> = core.available().iter().map(|p| p.as_str()).collect();
        eprintln!("dynamisd: profiles here: {}", names.join(", "));
        match core.apply_active("startup") {
            Ok(()) => eprintln!("dynamisd: active profile {}", core.active().as_str()),
            Err(e) => eprintln!("dynamisd: {e}"),
        }
    }
    eprintln!("dynamisd: ready");

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
        let released = svc
            .core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .release_owner(args.name());
        if !released.is_empty() {
            for cookie in released {
                bus::emit_released(&conn, cookie).await;
            }
            bus::notify(&conn, &svc).await;
        }
    }
    Err("lost the D-Bus connection".into())
}
