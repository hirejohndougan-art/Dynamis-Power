//! The Dynamis Power app's native shell. It only forwards calls from the
//! window to the Dynamis Power service, through `dynamis-client`, plus one
//! local action: saving an exported log file. All the logic is in the
//! service and in the React code; keep this file thin.
//!
//! Rust note: `#[tauri::command]` turns a function into something the
//! React side can call with `invoke("name", { args })`. Argument names are
//! converted from camelCase (JavaScript) to snake_case (Rust).

use std::sync::Mutex;

use dynamis_client::Client;
use serde_json::Value;
use tauri::State;

/// One connection to the service, made on first use and reused.
#[derive(Default)]
struct Conn(Mutex<Option<Client>>);

async fn client(conn: &Conn) -> Result<Client, String> {
    if let Some(c) = conn.0.lock().map_err(|e| e.to_string())?.clone() {
        return Ok(c);
    }
    let c = Client::connect().await?;
    *conn.0.lock().map_err(|e| e.to_string())? = Some(c.clone());
    Ok(c)
}

#[tauri::command]
async fn status(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.status().await
}

#[tauri::command]
async fn journal(conn: State<'_, Conn>, n: u32) -> Result<Value, String> {
    client(&conn).await?.journal(n).await
}

#[tauri::command]
async fn settings(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.settings().await
}

#[tauri::command]
async fn inventory(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.inventory().await
}

#[tauri::command]
async fn learned(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.learned().await
}

#[tauri::command]
async fn set_profile(conn: State<'_, Conn>, profile: String) -> Result<(), String> {
    client(&conn).await?.set_profile(&profile).await
}

#[tauri::command]
async fn set_auto(conn: State<'_, Conn>, on: bool) -> Result<(), String> {
    client(&conn).await?.set_auto(on).await
}

#[tauri::command]
async fn set_learning(conn: State<'_, Conn>, on: bool) -> Result<(), String> {
    client(&conn).await?.set_learning(on).await
}

#[tauri::command]
async fn undo(conn: State<'_, Conn>, setting: String) -> Result<String, String> {
    client(&conn).await?.undo(&setting).await
}

#[tauri::command]
async fn restore(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.restore().await
}

#[tauri::command]
async fn set_setting(
    conn: State<'_, Conn>,
    setting: String,
    value: String,
    allow_risky: bool,
) -> Result<(), String> {
    client(&conn)
        .await?
        .set_setting(&setting, &value, allow_risky)
        .await
}

#[tauri::command]
async fn set_safe_mode(conn: State<'_, Conn>, on: bool) -> Result<String, String> {
    client(&conn).await?.set_safe_mode(on).await
}

#[tauri::command]
async fn forget(conn: State<'_, Conn>, setting: String) -> Result<bool, String> {
    client(&conn).await?.forget(&setting).await
}

#[tauri::command]
async fn conflicts(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.conflicts().await
}

#[tauri::command]
async fn switch_off_conflict(conn: State<'_, Conn>, unit: String) -> Result<(), String> {
    client(&conn).await?.switch_off_conflict(&unit).await
}

#[tauri::command]
async fn auto_prefs(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.auto_prefs().await
}

#[tauri::command]
async fn set_auto_prefs(conn: State<'_, Conn>, prefs: Value) -> Result<(), String> {
    client(&conn).await?.set_auto_prefs(&prefs).await
}

/// Save an exported log into the person's Downloads folder (or home folder
/// if there is none). Returns the full path, to show to them.
#[tauri::command]
fn save_file(name: String, contents: String) -> Result<String, String> {
    // Only a plain file name: no folders, nothing hidden.
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim_start_matches('.').to_string();
    if safe.is_empty() {
        return Err("invalid file name".into());
    }
    let home = std::env::var_os("HOME").ok_or("no home folder")?;
    let downloads = std::path::Path::new(&home).join("Downloads");
    let dir = if downloads.is_dir() {
        downloads
    } else {
        home.into()
    };
    let path = dir.join(safe);
    std::fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

#[tauri::command]
async fn apps(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.apps().await
}

#[tauri::command]
async fn set_app_policy(conn: State<'_, Conn>, policy: Value) -> Result<(), String> {
    client(&conn).await?.set_app_policy(&policy).await
}

#[tauri::command]
async fn sleep_test(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.sleep_test().await
}

#[tauri::command]
async fn start_sleep_test(conn: State<'_, Conn>, minutes: f64) -> Result<(), String> {
    client(&conn).await?.start_sleep_test(minutes).await
}

#[tauri::command]
async fn stop_sleep_test(conn: State<'_, Conn>) -> Result<(), String> {
    client(&conn).await?.stop_sleep_test().await
}

#[tauri::command]
async fn diagnostics(conn: State<'_, Conn>) -> Result<String, String> {
    client(&conn).await?.diagnostics().await
}

#[tauri::command]
async fn devices(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.devices().await
}

#[tauri::command]
async fn set_device_policy(conn: State<'_, Conn>, policy: Value) -> Result<(), String> {
    client(&conn).await?.set_device_policy(&policy).await
}

#[tauri::command]
async fn recalibration(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.recalibration().await
}

#[tauri::command]
async fn start_recalibration(conn: State<'_, Conn>) -> Result<(), String> {
    client(&conn).await?.start_recalibration().await
}

#[tauri::command]
async fn stop_recalibration(conn: State<'_, Conn>) -> Result<(), String> {
    client(&conn).await?.stop_recalibration().await
}

#[tauri::command]
async fn tlp_import(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.tlp_import().await
}

#[tauri::command]
async fn apply_tlp_import(conn: State<'_, Conn>, lines: Vec<u32>) -> Result<Value, String> {
    client(&conn).await?.apply_tlp_import(&lines).await
}

#[tauri::command]
async fn learn_session(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.learn_session().await
}

#[tauri::command]
async fn start_learn_session(conn: State<'_, Conn>, workload: bool) -> Result<(), String> {
    client(&conn).await?.start_learn_session(workload).await
}

#[tauri::command]
async fn stop_learn_session(conn: State<'_, Conn>) -> Result<(), String> {
    client(&conn).await?.stop_learn_session().await
}

#[tauri::command]
async fn brightness(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.brightness().await
}

#[tauri::command]
async fn set_brightness_prefs(conn: State<'_, Conn>, prefs: Value) -> Result<(), String> {
    client(&conn).await?.set_brightness_prefs(&prefs).await
}

#[tauri::command]
async fn forget_app_usage(conn: State<'_, Conn>) -> Result<(), String> {
    client(&conn).await?.forget_app_usage().await
}

#[tauri::command]
async fn predict(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.predict().await
}

#[tauri::command]
async fn set_target(conn: State<'_, Conn>, until: f64) -> Result<(), String> {
    client(&conn).await?.set_target(until).await
}

#[tauri::command]
async fn charge_fully_once(conn: State<'_, Conn>, on: bool) -> Result<(), String> {
    client(&conn).await?.charge_fully_once(on).await
}

#[tauri::command]
async fn set_adaptive_charging(conn: State<'_, Conn>, on: bool) -> Result<(), String> {
    client(&conn).await?.set_adaptive_charging(on).await
}

#[tauri::command]
async fn tray_shown() -> Result<bool, String> {
    Client::tray_shown().await
}

#[tauri::command]
async fn set_tray(on: bool) -> Result<(), String> {
    Client::set_tray(on).await
}

#[tauri::command]
async fn profiles(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.profiles().await
}

#[tauri::command]
async fn set_profile_overrides(conn: State<'_, Conn>, overrides: Value) -> Result<(), String> {
    client(&conn).await?.set_profile_overrides(&overrides).await
}

#[tauri::command]
async fn sensors(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.sensors().await
}

#[tauri::command]
async fn system(conn: State<'_, Conn>) -> Result<Value, String> {
    client(&conn).await?.system().await
}

#[tauri::command]
async fn advise(conn: State<'_, Conn>, format: String) -> Result<String, String> {
    client(&conn).await?.advise(&format).await
}

#[tauri::command]
async fn set_dry_run(conn: State<'_, Conn>, on: bool) -> Result<(), String> {
    client(&conn).await?.set_dry_run(on).await
}

#[tauri::command]
async fn set_kept(conn: State<'_, Conn>, setting: String, keep: bool) -> Result<(), String> {
    client(&conn).await?.set_kept(&setting, keep).await
}

fn main() {
    tauri::Builder::default()
        .manage(Conn::default())
        .invoke_handler(tauri::generate_handler![
            status,
            journal,
            settings,
            inventory,
            learned,
            set_profile,
            set_auto,
            set_learning,
            undo,
            restore,
            set_setting,
            set_safe_mode,
            forget,
            conflicts,
            switch_off_conflict,
            auto_prefs,
            set_auto_prefs,
            apps,
            set_app_policy,
            forget_app_usage,
            devices,
            set_device_policy,
            brightness,
            set_brightness_prefs,
            tlp_import,
            apply_tlp_import,
            recalibration,
            start_recalibration,
            stop_recalibration,
            learn_session,
            start_learn_session,
            stop_learn_session,
            diagnostics,
            sleep_test,
            start_sleep_test,
            stop_sleep_test,
            predict,
            set_target,
            set_adaptive_charging,
            charge_fully_once,
            tray_shown,
            set_tray,
            profiles,
            set_profile_overrides,
            sensors,
            system,
            advise,
            set_dry_run,
            set_kept,
            save_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Dynamis Power app");
}
